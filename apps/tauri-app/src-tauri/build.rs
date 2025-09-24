use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Determine if this is a release build
    let target_profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let target_os = env::var("CARGO_CFG_TARGET_OS")
        .expect("CARGO_CFG_TARGET_OS environment variable is not set");
    let target_triple = env::var("TARGET").expect("TARGET environment variable is not set");
    let exe_suffix = if target_os == "windows" { ".exe" } else { "" };
    let tauri_sidecar_binary_name = format!("steamos-mount-cli-{}{}", target_triple, exe_suffix);
    let target_binary_name = format!("steamos-mount-cli{}", exe_suffix);

    // Determine target directory
    // CARGO_MANIFEST_DIR is apps/tauri-app/src-tauri
    // We need to go up to workspace root
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent() // apps/tauri-app
        .and_then(|p| p.parent()) // apps
        .and_then(|p| p.parent()) // workspace root
        .unwrap_or_else(|| manifest_dir.parent().unwrap_or(&manifest_dir));

    let target_dir = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("target"));

    let target_binary_path = target_dir.join(&target_profile).join(target_binary_name);

    // Copy to binaries/ - for Tauri sidecar (externalBin) - must use target triple naming
    let tauri_sidecar_binaries_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    let tauri_sidecard_binary_path = tauri_sidecar_binaries_dir.join(&tauri_sidecar_binary_name);

    // Build CLI if it doesn't exist
    if !target_binary_path.exists() {
        eprintln!("Building steamos-mount-cli...");
        let workspace_manifest = workspace_root.join("Cargo.toml");
        let mut cmd = Command::new("cargo");
        cmd.args([
            "build",
            "--bin",
            "steamos-mount-cli",
            "--manifest-path",
            "--profile",
            &target_profile as &str,
            workspace_manifest.to_str().unwrap(),
        ]);
        let build_status = cmd.status();

        match build_status {
            Ok(status) if status.success() => {
                eprintln!("Successfully built steamos-mount-cli");
            }
            Ok(status) => {
                eprintln!(
                    "Warning: Failed to build steamos-mount-cli (exit code: {})",
                    status.code().unwrap_or(-1)
                );
            }
            Err(e) => {
                eprintln!("Warning: Failed to execute cargo build: {}", e);
            }
        }
    }

    // Create destination directories
    if let Err(e) = std::fs::create_dir_all(&tauri_sidecar_binaries_dir) {
        eprintln!("Warning: Failed to create binaries directory: {}", e);
    }

    // Copy CLI binary to both locations
    if target_binary_path.exists() {
        // Copy to binaries/ (for Tauri sidecar)
        if let Err(e) = std::fs::copy(&target_binary_path, &tauri_sidecard_binary_path) {
            eprintln!("Error: Failed to copy CLI binary to binaries/: {}", e);
            eprintln!("  Source: {:?}", target_binary_path);
            eprintln!("  Dest: {:?}", tauri_sidecard_binary_path);
            panic!("Cannot proceed without CLI binary");
        } else {
            eprintln!("Copied CLI binary to {:?}", tauri_sidecard_binary_path);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) = std::fs::set_permissions(
                    &tauri_sidecard_binary_path,
                    std::fs::Permissions::from_mode(0o755),
                ) {
                    eprintln!("Warning: Failed to set executable permissions: {}", e);
                }
            }
        }
    } else {
        eprintln!("Error: CLI binary not found at {:?}", target_binary_path);
        eprintln!("Please ensure steamos-mount-cli is built before building the Tauri app:");
        eprintln!(
            "cargo build --profile {} --bin steamos-mount-cli",
            target_profile
        );
        panic!("CLI binary is required for bundling");
    }

    // Tell Cargo to rerun this build script if CLI source changes
    println!("cargo:rerun-if-changed=../../apps/cli");
    println!("cargo:rerun-if-changed=../../packages/core");

    tauri_build::build()
}
