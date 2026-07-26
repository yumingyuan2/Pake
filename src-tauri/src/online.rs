use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri_plugin_http::reqwest::{redirect::Policy, Client, Response, Url};

const MANIFEST_SCHEMA_VERSION: u32 = 2;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_RELEASE_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED_PAYLOAD_BYTES: u64 = 128 * 1024 * 1024;
const STALE_UPDATE_LOCK_AGE: Duration = Duration::from_secs(30 * 60);
const USER_AGENT: &str = "pake-online-bootstrap/1";
const CHINA_TRACE_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
const GITHUB_PROXY_PREFIX: &str = "https://v4.gh-proxy.org/";

#[derive(Debug, Clone)]
struct ReleaseChannel {
    repository: String,
    release_tag: String,
    config_id: String,
    os: String,
}

impl ReleaseChannel {
    fn embedded() -> Result<Self, String> {
        let channel = Self {
            repository: option_env!("PAKE_ONLINE_REPOSITORY")
                .unwrap_or_default()
                .trim()
                .to_string(),
            release_tag: option_env!("PAKE_ONLINE_RELEASE_TAG")
                .unwrap_or_default()
                .trim()
                .to_string(),
            config_id: option_env!("PAKE_ONLINE_CONFIG_ID")
                .unwrap_or_default()
                .trim()
                .to_string(),
            os: option_env!("PAKE_ONLINE_OS")
                .unwrap_or_default()
                .trim()
                .to_string(),
        };
        channel.validate()?;
        Ok(channel)
    }

    fn validate(&self) -> Result<(), String> {
        let mut repository = self.repository.split('/');
        let owner = repository.next().unwrap_or_default();
        let name = repository.next().unwrap_or_default();
        if owner.is_empty()
            || name.is_empty()
            || repository.next().is_some()
            || !owner.chars().all(is_github_name_character)
            || !name.chars().all(is_github_name_character)
        {
            return Err("The embedded online repository is invalid.".into());
        }
        if !is_safe_channel_value(&self.release_tag) || !is_safe_channel_value(&self.config_id) {
            return Err("The embedded online release channel is invalid.".into());
        }
        if self.os != current_os() {
            return Err("The embedded online channel targets another operating system.".into());
        }
        Ok(())
    }

    fn release_api_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            self.repository, self.release_tag
        )
    }
}

fn is_github_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn is_safe_channel_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

const fn current_os() -> &'static str {
    #[cfg(target_os = "windows")]
    return "windows";
    #[cfg(target_os = "macos")]
    return "macos";
    #[cfg(target_os = "linux")]
    return "linux";
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    id: u64,
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

impl GithubRelease {
    fn manifest_assets(&self) -> Vec<&GithubAsset> {
        let mut assets: Vec<_> = self
            .assets
            .iter()
            .filter(|asset| {
                asset.name.starts_with("pake-online-manifest-")
                    && asset.name.ends_with(".json")
                    && asset.size > 0
                    && asset.size <= MAX_MANIFEST_BYTES
            })
            .collect();
        assets.sort_by_key(|asset| std::cmp::Reverse(asset.id));
        assets
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnlineManifest {
    schema_version: u32,
    config_id: String,
    repository: String,
    release_tag: String,
    source: SourceBuild,
    platform: Platform,
    artifacts: Vec<PayloadArtifact>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceBuild {
    sha: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Platform {
    os: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadArtifact {
    name: String,
    format: String,
    size: u64,
    sha256: String,
    download_url: String,
    entrypoint: String,
    launch_kind: String,
}

impl OnlineManifest {
    fn validate(&self, channel: &ReleaseChannel) -> Result<(), String> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported online manifest schema: {}.",
                self.schema_version
            ));
        }
        if self.config_id != channel.config_id
            || self.repository != channel.repository
            || self.release_tag != channel.release_tag
            || self.platform.os != channel.os
        {
            return Err("The online manifest belongs to another release channel.".into());
        }
        if !matches!(self.source.sha.len(), 40 | 64)
            || !self
                .source
                .sha
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err("The online manifest has an invalid source commit.".into());
        }
        if self.artifacts.len() != 1 {
            return Err("The online manifest must contain exactly one payload.".into());
        }
        self.artifacts[0].validate(channel)
    }
}

impl PayloadArtifact {
    fn validate(&self, channel: &ReleaseChannel) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.contains('/')
            || self.name.contains('\\')
            || self.size == 0
            || self.size > MAX_PAYLOAD_BYTES
            || self.sha256.len() != 64
            || !self
                .sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
            || !is_safe_relative_path(Path::new(&self.entrypoint))
        {
            return Err("The online payload metadata is invalid.".into());
        }

        let expected = match current_os() {
            "windows" => ("exe.zip", "executable"),
            "macos" => ("app.zip", "appBundle"),
            "linux" => ("appimage", "executable"),
            _ => return Err("This platform is not supported by online mode.".into()),
        };
        if (self.format.as_str(), self.launch_kind.as_str()) != expected {
            return Err("The online payload format does not match this platform.".into());
        }

        let url = Url::parse(&self.download_url)
            .map_err(|error| format!("The online payload URL is invalid: {error}"))?;
        let expected_path = format!(
            "/{}/releases/download/{}/{}",
            channel.repository, channel.release_tag, self.name
        );
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || url.path() != expected_path
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("The online payload points outside its GitHub release.".into());
        }
        Ok(())
    }
}

fn is_safe_relative_path(path: &Path) -> bool {
    let mut has_component = false;
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return false;
        }
        has_component = true;
    }
    has_component
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveState {
    build_id: String,
    source_sha: String,
    entrypoint: String,
    launch_kind: String,
}

struct UpdateLock {
    path: PathBuf,
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[Pake Online] Failed to remove update lock: {error}");
            }
        }
    }
}

pub fn run() {
    let channel = match ReleaseChannel::embedded() {
        Ok(channel) => channel,
        Err(error) => {
            log_fallback(&error);
            return;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log_error(
                &channel,
                &format!("Failed to initialize the updater: {error}"),
            );
            return;
        }
    };
    if let Err(error) = runtime.block_on(run_async(&channel)) {
        log_error(&channel, &error);
    }
}

async fn run_async(channel: &ReleaseChannel) -> Result<(), String> {
    let root = cache_root(channel)?;
    fs::create_dir_all(root.join("versions"))
        .map_err(|error| format!("Failed to create the online cache: {error}"))?;

    let current = load_state(&root).filter(|state| state_is_usable(&root, state));
    let launched = if let Some(state) = &current {
        match launch(&root, state) {
            Ok(()) => true,
            Err(error) => {
                log_error(channel, &error);
                false
            }
        }
    } else {
        false
    };

    let Some(_lock) = acquire_lock(&root)? else {
        if current.is_none() {
            wait_for_first_install(&root).await?;
        }
        return Ok(());
    };

    let client = Client::builder()
        .user_agent(USER_AGENT)
        .redirect(Policy::limited(10))
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(20 * 60))
        .build()
        .map_err(|error| format!("Failed to initialize HTTPS: {error}"))?;
    let use_china_proxy = is_mainland_china(&client).await;
    let manifest = resolve_manifest(&client, channel, use_china_proxy).await?;

    if current
        .as_ref()
        .is_some_and(|state| state.build_id == manifest.artifacts[0].sha256)
    {
        if !launched {
            let state = current
                .as_ref()
                .ok_or_else(|| "The active online build state disappeared.".to_string())?;
            launch(&root, state)?;
        }
        cleanup_versions(&root, &[manifest.artifacts[0].sha256.as_str()]);
        return Ok(());
    }

    let next = install_payload(&client, channel, &manifest, &root, use_china_proxy).await?;
    write_state(&root, &next)?;

    if current.is_none() || !launched {
        launch(&root, &next)?;
    }
    let mut keep = vec![next.build_id.as_str()];
    if let Some(state) = &current {
        keep.push(state.build_id.as_str());
    }
    cleanup_versions(&root, &keep);
    Ok(())
}

fn cache_root(channel: &ReleaseChannel) -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"));
    #[cfg(target_os = "linux")]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local").join("share"))
        });

    base.map(|base| {
        base.join("Pake")
            .join("Online")
            .join(channel.config_id.as_str())
    })
    .ok_or_else(|| "The user data directory is unavailable.".to_string())
}

fn acquire_lock(root: &Path) -> Result<Option<UpdateLock>, String> {
    let path = root.join("update.lock");
    match create_update_lock(&path) {
        Ok(lock) => Ok(Some(lock)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= STALE_UPDATE_LOCK_AGE);
            if !stale {
                return Ok(None);
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("Failed to clear a stale update lock: {error}"));
                }
            }
            match create_update_lock(&path) {
                Ok(lock) => Ok(Some(lock)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
                Err(error) => Err(format!("Failed to acquire the update lock: {error}")),
            }
        }
        Err(error) => Err(format!("Failed to acquire the update lock: {error}")),
    }
}

fn create_update_lock(path: &Path) -> std::io::Result<UpdateLock> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = writeln!(file, "{}", std::process::id()) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(UpdateLock {
        path: path.to_path_buf(),
    })
}

async fn wait_for_first_install(root: &Path) -> Result<(), String> {
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(state) = load_state(root).filter(|state| state_is_usable(root, state)) {
            return launch(root, &state);
        }
    }
    Err("Another bootstrap instance did not finish the first download.".into())
}

fn load_state(root: &Path) -> Option<ActiveState> {
    let bytes = fs::read(root.join("active.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn state_is_usable(root: &Path, state: &ActiveState) -> bool {
    state.build_id.len() == 64
        && state
            .build_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        && matches!(state.source_sha.len(), 40 | 64)
        && state
            .source_sha
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        && is_safe_relative_path(Path::new(&state.entrypoint))
        && matches!(state.launch_kind.as_str(), "executable" | "appBundle")
        && root
            .join("versions")
            .join(&state.build_id)
            .join(&state.entrypoint)
            .exists()
}

fn write_state(root: &Path, state: &ActiveState) -> Result<(), String> {
    let temporary = root.join("active.json.tmp");
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("Failed to serialize the active build: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("Failed to stage the active build state: {error}"))?;
    let destination = root.join("active.json");
    if destination.exists() {
        fs::remove_file(&destination)
            .map_err(|error| format!("Failed to replace the active build state: {error}"))?;
    }
    fs::rename(&temporary, &destination)
        .map_err(|error| format!("Failed to activate the downloaded build: {error}"))
}

async fn resolve_manifest(
    client: &Client,
    channel: &ReleaseChannel,
    use_china_proxy: bool,
) -> Result<OnlineManifest, String> {
    let response = client
        .get(channel.release_api_url())
        .send()
        .await
        .map_err(|error| format!("Failed to query the online release: {error}"))?
        .error_for_status()
        .map_err(|error| format!("The online release lookup failed: {error}"))?;
    let release_bytes = read_bounded_response(response, MAX_RELEASE_METADATA_BYTES).await?;
    let release: GithubRelease = serde_json::from_slice(&release_bytes)
        .map_err(|error| format!("GitHub returned invalid release metadata: {error}"))?;

    for asset in release.manifest_assets() {
        let candidate = download_manifest(client, channel, asset, use_china_proxy).await;
        if let Ok(manifest) = candidate {
            if manifest.validate(channel).is_ok() {
                return Ok(manifest);
            }
        }
    }
    Err("No valid completed online build manifest was found.".into())
}

async fn download_manifest(
    client: &Client,
    channel: &ReleaseChannel,
    asset: &GithubAsset,
    use_china_proxy: bool,
) -> Result<OnlineManifest, String> {
    validate_release_asset_url(channel, asset)?;
    let expected_digest = asset
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"));
    let allow_proxy = use_china_proxy && expected_digest.is_some();
    let (response, used_proxy) =
        download_response(client, &asset.browser_download_url, allow_proxy).await?;
    let first = read_manifest(response, asset, expected_digest).await;
    if first.is_ok() || !used_proxy {
        return first;
    }
    let (response, _) = download_response(client, &asset.browser_download_url, false).await?;
    read_manifest(response, asset, expected_digest).await
}

fn validate_release_asset_url(channel: &ReleaseChannel, asset: &GithubAsset) -> Result<(), String> {
    let url = Url::parse(&asset.browser_download_url)
        .map_err(|error| format!("The release asset URL is invalid: {error}"))?;
    let expected_path = format!(
        "/{}/releases/download/{}/{}",
        channel.repository, channel.release_tag, asset.name
    );
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.path() != expected_path
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The release asset points outside its GitHub channel.".into());
    }
    Ok(())
}

async fn read_manifest(
    response: Response,
    asset: &GithubAsset,
    expected_digest: Option<&str>,
) -> Result<OnlineManifest, String> {
    let bytes = read_bounded_response(response, MAX_MANIFEST_BYTES).await?;
    if bytes.len() as u64 != asset.size {
        return Err("The online manifest size does not match GitHub metadata.".into());
    }
    if let Some(expected) = expected_digest {
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            return Err("The online manifest failed GitHub digest verification.".into());
        }
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("The online manifest is invalid: {error}"))
}

async fn read_bounded_response(mut response: Response, maximum: u64) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|size| size > maximum) {
        return Err("The download exceeds its safety limit.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Failed to read the download: {error}"))?
    {
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > maximum {
            return Err("The download exceeds its safety limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn install_payload(
    client: &Client,
    channel: &ReleaseChannel,
    manifest: &OnlineManifest,
    root: &Path,
    use_china_proxy: bool,
) -> Result<ActiveState, String> {
    let artifact = &manifest.artifacts[0];
    let versions = root.join("versions");
    let destination = versions.join(&artifact.sha256);
    let state = ActiveState {
        build_id: artifact.sha256.clone(),
        source_sha: manifest.source.sha.clone(),
        entrypoint: artifact.entrypoint.clone(),
        launch_kind: artifact.launch_kind.clone(),
    };
    if destination.join(&artifact.entrypoint).exists() {
        return Ok(state);
    }

    let staging = versions.join(format!(
        ".staging-{}-{}",
        &artifact.sha256[..12],
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("Failed to clear stale update staging: {error}"))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create update staging: {error}"))?;

    let download = staging.join("payload.download");
    let result = async {
        download_verified(client, artifact, &download, use_china_proxy, channel).await?;
        activate_download(artifact, &download, &staging)?;
        if !staging.join(&artifact.entrypoint).exists() {
            return Err("The downloaded payload is missing its entrypoint.".into());
        }
        if destination.exists() {
            fs::remove_dir_all(&destination)
                .map_err(|error| format!("Failed to replace an invalid cached build: {error}"))?;
        }
        fs::rename(&staging, &destination)
            .map_err(|error| format!("Failed to activate the downloaded build: {error}"))?;
        Ok(state)
    }
    .await;

    if result.is_err() && staging.exists() {
        if let Err(error) = fs::remove_dir_all(&staging) {
            eprintln!("[Pake Online] Failed to clean update staging: {error}");
        }
    }
    result
}

async fn download_verified(
    client: &Client,
    artifact: &PayloadArtifact,
    destination: &Path,
    use_china_proxy: bool,
    channel: &ReleaseChannel,
) -> Result<(), String> {
    artifact.validate(channel)?;
    let (response, used_proxy) =
        download_response(client, &artifact.download_url, use_china_proxy).await?;
    let first = write_verified_response(response, artifact, destination).await;
    if first.is_ok() || !used_proxy {
        return first;
    }
    let (response, _) = download_response(client, &artifact.download_url, false).await?;
    write_verified_response(response, artifact, destination).await
}

async fn write_verified_response(
    mut response: Response,
    artifact: &PayloadArtifact,
    destination: &Path,
) -> Result<(), String> {
    if response
        .content_length()
        .is_some_and(|size| size > artifact.size)
    {
        return Err("The payload response exceeds the manifest size.".into());
    }
    let mut output = File::create(destination)
        .map_err(|error| format!("Failed to create the payload file: {error}"))?;
    let mut hash = Sha256::new();
    let mut downloaded = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Failed to download the payload: {error}"))?
    {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > artifact.size {
            return Err("The payload download exceeded the manifest size.".into());
        }
        output
            .write_all(&chunk)
            .map_err(|error| format!("Failed to save the payload: {error}"))?;
        hash.update(&chunk);
    }
    output
        .flush()
        .map_err(|error| format!("Failed to flush the payload: {error}"))?;
    let digest = format!("{:x}", hash.finalize());
    if downloaded != artifact.size || !digest.eq_ignore_ascii_case(&artifact.sha256) {
        return Err("The payload failed size or SHA-256 verification.".into());
    }
    Ok(())
}

async fn download_response(
    client: &Client,
    original_url: &str,
    use_china_proxy: bool,
) -> Result<(Response, bool), String> {
    if use_china_proxy {
        let proxy = github_proxy_url(original_url)?;
        if let Ok(response) = client.get(proxy).send().await {
            if response.status().is_success() {
                return Ok((response, true));
            }
        }
    }
    let response = client
        .get(original_url)
        .send()
        .await
        .map_err(|error| format!("The GitHub download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("The GitHub download failed: {error}"))?;
    Ok((response, false))
}

async fn is_mainland_china(client: &Client) -> bool {
    let Ok(response) = client
        .get(CHINA_TRACE_URL)
        .timeout(Duration::from_secs(4))
        .send()
        .await
    else {
        return false;
    };
    let Ok(trace) = response.text().await else {
        return false;
    };
    country_from_cloudflare_trace(&trace) == Some("CN")
}

fn country_from_cloudflare_trace(trace: &str) -> Option<&str> {
    trace
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(key, value)| (key == "loc").then_some(value.trim()))
        .filter(|country| country.len() == 2)
}

fn github_proxy_url(original: &str) -> Result<String, String> {
    let url = Url::parse(original)
        .map_err(|error| format!("The GitHub download URL is invalid: {error}"))?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return Err("Only HTTPS github.com downloads may use the proxy.".into());
    }
    Ok(format!("{GITHUB_PROXY_PREFIX}{original}"))
}

fn activate_download(
    artifact: &PayloadArtifact,
    download: &Path,
    staging: &Path,
) -> Result<(), String> {
    if artifact.launch_kind == "executable" {
        if artifact.format == "exe.zip" {
            return extract_single_executable(artifact, download, staging);
        }
        let entrypoint = staging.join(&artifact.entrypoint);
        if let Some(parent) = entrypoint.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create the payload directory: {error}"))?;
        }
        fs::rename(download, &entrypoint)
            .map_err(|error| format!("Failed to activate the payload executable: {error}"))?;
        set_executable(&entrypoint)?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(download)
            .arg(staging)
            .status()
            .map_err(|error| format!("Failed to extract the application bundle: {error}"))?;
        if !status.success() {
            return Err(format!(
                "The application bundle extractor exited with code {}.",
                status.code().unwrap_or(-1)
            ));
        }
        fs::remove_file(download)
            .map_err(|error| format!("Failed to remove the payload archive: {error}"))?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    Err("Application bundle payloads are only supported on macOS.".into())
}

fn extract_single_executable(
    artifact: &PayloadArtifact,
    download: &Path,
    staging: &Path,
) -> Result<(), String> {
    let archive_file = File::open(download)
        .map_err(|error| format!("Failed to open the payload archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|error| format!("The payload ZIP is invalid: {error}"))?;
    if archive.len() != 1 {
        return Err("The payload ZIP must contain exactly one executable.".into());
    }
    let mut member = archive
        .by_index(0)
        .map_err(|error| format!("Failed to inspect the payload ZIP: {error}"))?;
    let member_path = member
        .enclosed_name()
        .ok_or_else(|| "The payload ZIP contains an unsafe path.".to_string())?;
    if member.is_dir()
        || member_path != Path::new(&artifact.entrypoint)
        || member.size() == 0
        || member.size() > MAX_EXPANDED_PAYLOAD_BYTES
    {
        return Err("The payload ZIP executable metadata is invalid.".into());
    }
    let entrypoint = staging.join(&artifact.entrypoint);
    if let Some(parent) = entrypoint.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create the payload directory: {error}"))?;
    }
    let mut output = File::create(&entrypoint)
        .map_err(|error| format!("Failed to create the payload executable: {error}"))?;
    io::copy(&mut member, &mut output)
        .map_err(|error| format!("Failed to extract the payload executable: {error}"))?;
    output
        .flush()
        .map_err(|error| format!("Failed to flush the payload executable: {error}"))?;
    fs::remove_file(download)
        .map_err(|error| format!("Failed to remove the payload archive: {error}"))?;
    set_executable(&entrypoint)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|error| format!("Failed to inspect the payload permissions: {error}"))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("Failed to mark the payload executable: {error}"))
}

#[cfg(windows)]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn launch(root: &Path, state: &ActiveState) -> Result<(), String> {
    let entrypoint = root
        .join("versions")
        .join(&state.build_id)
        .join(&state.entrypoint);
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();

    #[cfg(target_os = "macos")]
    let mut command = if state.launch_kind == "appBundle" {
        let mut command = Command::new("/usr/bin/open");
        command.arg("-n").arg(&entrypoint);
        if !arguments.is_empty() {
            command.arg("--args").args(&arguments);
        }
        command
    } else {
        let mut command = Command::new(&entrypoint);
        command.args(&arguments);
        command
    };

    #[cfg(not(target_os = "macos"))]
    let mut command = {
        let mut command = Command::new(&entrypoint);
        command.args(&arguments);
        command
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map_err(|error| format!("Failed to launch the cached application: {error}"))?;
    Ok(())
}

fn cleanup_versions(root: &Path, keep: &[&str]) {
    let Ok(entries) = fs::read_dir(root.join("versions")) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if keep.iter().any(|value| *value == name) || name.starts_with(".staging-") {
            continue;
        }
        let path = entry.path();
        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        if let Err(error) = result {
            eprintln!("[Pake Online] Failed to clean an old build: {error}");
        }
    }
}

fn log_error(channel: &ReleaseChannel, message: &str) {
    if let Ok(root) = cache_root(channel) {
        if fs::create_dir_all(&root).is_ok() {
            if let Ok(mut log) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("bootstrap.log"))
            {
                let _ = writeln!(log, "{message}");
                return;
            }
        }
    }
    log_fallback(message);
}

fn log_fallback(message: &str) {
    let path = std::env::temp_dir().join("pake-online-bootstrap.log");
    if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(log, "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn channel() -> ReleaseChannel {
        ReleaseChannel {
            repository: "owner/repo".into(),
            release_tag: "pake-online-example".into(),
            config_id: "example-windows-123".into(),
            os: current_os().into(),
        }
    }

    fn artifact() -> PayloadArtifact {
        let (format, entrypoint, launch_kind) = match current_os() {
            "windows" => ("exe.zip", "app.exe", "executable"),
            "macos" => ("app.zip", "Example.app", "appBundle"),
            "linux" => ("appimage", "app.AppImage", "executable"),
            _ => unreachable!(),
        };
        PayloadArtifact {
            name: format!("payload.{format}"),
            format: format.into(),
            size: 42,
            sha256: "a".repeat(64),
            download_url: format!(
                "https://github.com/owner/repo/releases/download/pake-online-example/payload.{format}"
            ),
            entrypoint: entrypoint.into(),
            launch_kind: launch_kind.into(),
        }
    }

    #[test]
    fn validates_payload_bound_to_the_release_channel() {
        assert!(artifact().validate(&channel()).is_ok());
    }

    #[test]
    fn rejects_cross_repository_and_unsafe_entrypoints() {
        let mut value = artifact();
        value.download_url = value.download_url.replace("owner/repo", "other/repo");
        assert!(value.validate(&channel()).is_err());

        let mut value = artifact();
        value.entrypoint = "../outside".into();
        assert!(value.validate(&channel()).is_err());
    }

    #[test]
    fn parses_cloudflare_country_without_using_the_locale() {
        assert_eq!(
            country_from_cloudflare_trace("fl=1\nloc=CN\ntls=TLSv1.3"),
            Some("CN")
        );
        assert_eq!(country_from_cloudflare_trace("loc=US"), Some("US"));
        assert_eq!(country_from_cloudflare_trace("ip=192.0.2.1"), None);
    }

    #[test]
    fn proxies_only_https_github_downloads() {
        let source = "https://github.com/yumingyuan2/Pake/releases/download/channel/payload.exe";
        assert_eq!(
            github_proxy_url(source).unwrap(),
            format!("https://v4.gh-proxy.org/{source}")
        );
        assert!(github_proxy_url("https://example.com/payload.exe").is_err());
        assert!(github_proxy_url("http://github.com/owner/repo/payload.exe").is_err());
    }

    #[test]
    fn selects_newest_bounded_manifest_first() {
        let release = GithubRelease {
            assets: vec![
                GithubAsset {
                    id: 1,
                    name: "pake-online-manifest-old.json".into(),
                    size: 10,
                    browser_download_url: "https://example.invalid/old".into(),
                    digest: None,
                },
                GithubAsset {
                    id: 2,
                    name: "pake-online-manifest-new.json".into(),
                    size: 10,
                    browser_download_url: "https://example.invalid/new".into(),
                    digest: None,
                },
            ],
        };
        assert_eq!(release.manifest_assets()[0].id, 2);
    }

    #[test]
    fn validates_manifest_channel_and_schema() {
        let mut manifest = OnlineManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            config_id: channel().config_id,
            repository: channel().repository,
            release_tag: channel().release_tag,
            source: SourceBuild {
                sha: "1234567890abcdef1234567890abcdef12345678".into(),
            },
            platform: Platform {
                os: current_os().into(),
            },
            artifacts: vec![artifact()],
        };
        assert!(manifest.validate(&channel()).is_ok());
        manifest.schema_version += 1;
        assert!(manifest.validate(&channel()).is_err());
    }

    #[test]
    fn extracts_only_the_declared_zip_executable() -> Result<(), Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("pake-online-zip-test-{}", std::process::id()));
        let archive_path = root.join("payload.zip");
        let staging = root.join("staging");
        fs::create_dir_all(&staging)?;
        let archive_file = File::create(&archive_path)?;
        let mut writer = zip::ZipWriter::new(archive_file);
        writer.start_file("app.exe", SimpleFileOptions::default())?;
        writer.write_all(b"test executable")?;
        writer.finish()?;

        let value = PayloadArtifact {
            format: "exe.zip".into(),
            entrypoint: "app.exe".into(),
            ..artifact()
        };
        assert!(extract_single_executable(&value, &archive_path, &staging).is_ok());
        assert_eq!(
            fs::read(staging.join("app.exe")).ok().as_deref(),
            Some(b"test executable".as_slice())
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn serializes_updates_with_a_recoverable_lock() -> Result<(), Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("pake-online-lock-test-{}", std::process::id()));
        fs::create_dir_all(&root)?;
        let first = acquire_lock(&root);
        assert!(matches!(first, Ok(Some(_))));
        assert!(matches!(acquire_lock(&root), Ok(None)));
        drop(first);
        assert!(matches!(acquire_lock(&root), Ok(Some(_))));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
