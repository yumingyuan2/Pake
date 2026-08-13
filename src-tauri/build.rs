fn main() {
    println!("cargo:rerun-if-changed=.pake/pake.json");
    println!("cargo:rerun-if-changed=.pake/tauri.conf.json");
    println!("cargo:rerun-if-env-changed=PAKE_ONLINE_REPOSITORY");
    println!("cargo:rerun-if-env-changed=PAKE_ONLINE_RELEASE_TAG");
    println!("cargo:rerun-if-env-changed=PAKE_ONLINE_CONFIG_ID");
    println!("cargo:rerun-if-env-changed=PAKE_ONLINE_OS");
    println!("cargo:rerun-if-env-changed=PAKE_ONLINE_PRODUCT_NAME");
    println!("cargo:rerun-if-env-changed=PAKE_ONLINE_BUNDLE_ID");
    tauri_build::build()
}
