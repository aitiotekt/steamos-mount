//! SteamOS Mount Tauri Application
//!
//! Desktop application for managing NTFS/exFAT drive mounts on SteamOS.

mod commands;
mod context;
mod types;

use commands::{
    check_dirty_volume, copy_to_clipboard, deconfigure_device, detect_steam_library_vdf,
    get_default_mount_point, get_device_info, get_mount_config_suggestion, get_steam_state,
    inject_steam_library, list_devices, mount_device, preview_mount_options, repair_dirty_volume,
    unmount_device,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_devices,
            get_device_info,
            get_default_mount_point,
            preview_mount_options,
            mount_device,
            unmount_device,
            deconfigure_device,
            check_dirty_volume,
            repair_dirty_volume,
            inject_steam_library,
            detect_steam_library_vdf,
            get_steam_state,
            get_mount_config_suggestion,
            copy_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
