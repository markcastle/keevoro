use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // On Windows the app needs a manifest asking for the modern common
    // controls, or it will not start. Tauri normally adds one to the app
    // only, which leaves the test program without it, and a test that opens
    // Tauri's stand-in window then dies before it runs. So the manifest is
    // given to the linker here instead, for the app and its tests alike.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))?;
    } else {
        tauri_build::build();
    }
    Ok(())
}
