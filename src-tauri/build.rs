use std::env;
use std::path::PathBuf;

/// The app manifest goes in through the linker, not through tauri-build.
///
/// Its one job is a dependency on Common Controls v6 (`windows-app-manifest.xml`
/// is tauri-build's own default, copied verbatim). Without it Windows binds
/// `comctl32.dll` v5, which has no `TaskDialogIndirect` — and since the dialog
/// code imports that function, the loader refuses the executable outright:
/// `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`, before `main`.
///
/// tauri-build embeds the manifest in its `.rc` resource, which it hands to
/// `rustc-link-arg-bins` — the app binary only. Test executables are not bins,
/// so any test that pulls the dialog code in (every `tauri::test::mock_app`
/// does) builds a binary that cannot start. That was the wall the first
/// `MockRuntime` attempt hit. `rustc-link-arg` reaches every linked target,
/// tests included, so the manifest now does too — and the shipped executable
/// ends up with the same manifest it always had, by a different route.
///
/// MSVC only: `/MANIFEST:EMBED` is a `link.exe` flag, and MSVC is the only
/// Windows toolchain this project builds with.
fn main() {
    let mut windows = tauri_build::WindowsAttributes::new();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();

        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run build script");
}
