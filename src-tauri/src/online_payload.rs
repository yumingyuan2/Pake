use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const BOOTSTRAP_MARKER: &str = "bootstrap.json";
const PAYLOAD_LAUNCH_ENV: &str = "PAKE_ONLINE_PAYLOAD_LAUNCH";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapMarker {
    executable: PathBuf,
}

pub fn redirect_to_bootstrap() -> bool {
    if std::env::var_os(PAYLOAD_LAUNCH_ENV).is_some() {
        return false;
    }
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let Some(bootstrap) = resolve_bootstrap(&executable) else {
        return false;
    };

    let mut command = Command::new(bootstrap);
    command.args(std::env::args_os().skip(1));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn().is_ok()
}

fn resolve_bootstrap(payload: &Path) -> Option<PathBuf> {
    for ancestor in payload.ancestors() {
        let marker_path = ancestor.join(BOOTSTRAP_MARKER);
        let Ok(bytes) = fs::read(&marker_path) else {
            continue;
        };
        let Ok(marker) = serde_json::from_slice::<BootstrapMarker>(&bytes) else {
            continue;
        };
        let versions = ancestor.join("versions");
        if payload.starts_with(&versions)
            && marker.executable.is_absolute()
            && marker.executable.is_file()
        {
            return Some(marker.executable);
        }
    }
    None
}

pub fn set_app_user_model_id() {
    let Some(bundle_id) = option_env!("PAKE_ONLINE_BUNDLE_ID") else {
        return;
    };
    let wide: Vec<u16> = bundle_id.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide.as_ptr())
    };
    if result < 0 {
        eprintln!("[Pake] Failed to set the Windows AppUserModelID: HRESULT {result:#x}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_a_normal_direct_launch_unchanged() {
        assert!(!redirect_to_bootstrap());
        set_app_user_model_id();
    }

    #[test]
    fn ignores_markers_outside_the_online_versions_tree() {
        let root =
            std::env::temp_dir().join(format!("pake-online-payload-test-{}", std::process::id()));
        let bootstrap = root.join("bootstrap.exe");
        let unrelated = root.join("unrelated").join("app.exe");
        fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        fs::write(&bootstrap, b"bootstrap").unwrap();
        fs::write(
            root.join(BOOTSTRAP_MARKER),
            format!(
                "{{\"executable\":{}}}",
                serde_json::to_string(&bootstrap).unwrap()
            ),
        )
        .unwrap();
        assert!(resolve_bootstrap(&unrelated).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_the_installed_bootstrap_for_a_cached_payload() {
        let root = std::env::temp_dir().join(format!(
            "pake-online-payload-resolve-test-{}",
            std::process::id()
        ));
        let bootstrap = root.join("bootstrap.exe");
        let payload = root.join("versions").join("digest").join("app.exe");
        fs::create_dir_all(payload.parent().unwrap()).unwrap();
        fs::write(&bootstrap, b"bootstrap").unwrap();
        fs::write(&payload, b"payload").unwrap();
        fs::write(
            root.join(BOOTSTRAP_MARKER),
            format!(
                "{{\"executable\":{}}}",
                serde_json::to_string(&bootstrap).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(resolve_bootstrap(&payload), Some(bootstrap));
        fs::remove_dir_all(root).unwrap();
    }
}
