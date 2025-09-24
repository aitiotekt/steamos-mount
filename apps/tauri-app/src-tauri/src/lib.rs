//! SteamOS Mount Tauri Application
//!
//! Desktop application for managing NTFS/exFAT drive mounts on SteamOS.

mod commands;
mod types;

use commands::{
    check_dirty_volume, copy_to_clipboard, deconfigure_device, detect_steam_library_vdf,
    get_default_mount_point, get_device_info, get_presets, inject_steam_library, list_devices,
    mount_device, repair_dirty_volume, unmount_device,
};
use snafu::ResultExt;
use tauri::{path::BaseDirectory, App, Manager};

fn prepare_appdata(app: &App) -> Result<(), steamos_mount_core::Error> {
    use std::fs;
    // Resolve sidecar path using Tauri's resource system
    let target_triple = tauri::utils::platform::target_triple()
        .with_whatever_context(|e| format!("Failed to get target triple: {}", e))?;
    let exe_suffix = std::env::consts::EXE_SUFFIX;
    let sidecar_rel_name = format!("bin/steamos-mount-cli-{}{}", target_triple, exe_suffix);
    let sidecar_resource_path = app
        .path()
        .resolve(&sidecar_rel_name, BaseDirectory::Resource)
        .with_whatever_context(|e| {
            format!(
                "Failed to resolve resource path of '{}': {}",
                &sidecar_rel_name, e
            )
        })?;
    let sidecar_appdata_path = app
        .path()
        .resolve(&sidecar_rel_name, BaseDirectory::AppData)
        .with_whatever_context(|e| {
            format!(
                "Failed to resolve appdata path of '{}': {}",
                &sidecar_rel_name, e
            )
        })?;

    if sidecar_appdata_path.exists() {
        return Ok(());
    }

    if !sidecar_resource_path.exists() {
        return Err(steamos_mount_core::Error::SidecarNotFound {
            path: sidecar_resource_path.to_string_lossy().to_string(),
        });
    }

    if let Some(sidecar_appdata_dir) = sidecar_appdata_path.parent() {
        fs::create_dir_all(sidecar_appdata_dir).with_whatever_context(|e| {
            format!(
                "Failed to create directory '{}': {}",
                sidecar_appdata_dir.display(),
                e
            )
        })?;
    }

    fs::copy(&sidecar_resource_path, &sidecar_appdata_path).with_whatever_context(|e| {
        format!("Failed to copy sidecar from resource to appdata: {}", e)
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&sidecar_appdata_path)
            .with_whatever_context(|e| {
                format!(
                    "Failed to get metadata of '{}': {}",
                    sidecar_appdata_path.display(),
                    e
                )
            })?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&sidecar_appdata_path, perms).with_whatever_context(|e| {
            format!(
                "Failed to set permissions of '{}': {}",
                sidecar_appdata_path.display(),
                e
            )
        })?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            prepare_appdata(app).boxed_local()?;
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_devices,
            get_device_info,
            get_default_mount_point,
            mount_device,
            unmount_device,
            deconfigure_device,
            check_dirty_volume,
            repair_dirty_volume,
            inject_steam_library,
            detect_steam_library_vdf,
            get_presets,
            commands::get_steam_state,
            copy_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
