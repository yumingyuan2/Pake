use std::env;
use std::path::PathBuf;

fn numeric_version(value: &str) -> Option<u64> {
    let core = value.split(['-', '+']).next()?;
    let mut parts = [0_u16; 4];
    let values = core.split('.').collect::<Vec<_>>();
    if values.is_empty() || values.len() > parts.len() {
        return None;
    }
    for (index, value) in values.into_iter().enumerate() {
        parts[index] = value.parse().ok()?;
    }
    Some(
        (u64::from(parts[0]) << 48)
            | (u64::from(parts[1]) << 32)
            | (u64::from(parts[2]) << 16)
            | u64::from(parts[3]),
    )
}

fn main() {
    println!("cargo:rerun-if-env-changed=PAKE_OFFLINE_MSI");
    println!("cargo:rerun-if-env-changed=PAKE_OFFLINE_ICON");
    println!("cargo:rerun-if-env-changed=PAKE_OFFLINE_APP_NAME");
    println!("cargo:rerun-if-env-changed=PAKE_INSTALLER_KIND");
    println!("cargo:rerun-if-env-changed=PAKE_INSTALLER_VERSION");
    if let Some(msi) = env::var_os("PAKE_OFFLINE_MSI") {
        println!("cargo:rerun-if-changed={}", PathBuf::from(msi).display());
    }
    if let Some(icon) = env::var_os("PAKE_OFFLINE_ICON") {
        println!("cargo:rerun-if-changed={}", PathBuf::from(icon).display());
    }

    #[cfg(windows)]
    {
        let manifest_directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let default_icon = manifest_directory
            .join("..")
            .join("..")
            .join("src-tauri")
            .join("png")
            .join("icon_256.ico");
        let icon = env::var_os("PAKE_OFFLINE_ICON")
            .map(PathBuf::from)
            .unwrap_or(default_icon);
        let app_name =
            env::var("PAKE_OFFLINE_APP_NAME").unwrap_or_else(|_| "Pake Application".into());
        let installer_kind = env::var("PAKE_INSTALLER_KIND").unwrap_or_else(|_| "Offline".into());

        let mut resource = winres::WindowsResource::new();
        resource.set_icon(icon.to_string_lossy().as_ref());
        resource.set("ProductName", &app_name);
        resource.set(
            "FileDescription",
            &format!("{app_name} {installer_kind} Installer"),
        );
        if let Some(version) = env::var("PAKE_INSTALLER_VERSION")
            .ok()
            .as_deref()
            .and_then(numeric_version)
        {
            resource.set_version_info(winres::VersionInfo::FILEVERSION, version);
            resource.set_version_info(winres::VersionInfo::PRODUCTVERSION, version);
        }
        resource
            .compile()
            .expect("failed to compile the offline installer resources");
    }
}
