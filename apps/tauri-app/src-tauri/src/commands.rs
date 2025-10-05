//! Tauri commands for the steamos-mount desktop application.
//!
//! These commands bridge the frontend UI with the core library functionality.
//!
//! ## Authorization Model
//!
//! Each command that requires privilege escalation creates its own authorization session.
//! Commands do not share sessions between invocations, ensuring explicit user consent
//! for each privileged operation. This means each command will prompt for authorization
//! when it needs to perform privileged actions.

use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use snafu::OptionExt;
use snafu::ResultExt;
use tauri::AppHandle;
use tauri::command;

use steamos_mount_core::{fstab, mount, preset, steam};

use crate::types::{
    DeviceInfo, FstabPreview, MountConfig, SteamInjectionConfig, SteamInjectionMode,
};

use crate::context::{command_in_non_privileged_context, command_in_privileged_context};

// ============================================================================
// Tauri commands
// ============================================================================

/// Lists all mountable devices (NTFS/exFAT partitions), including offline managed entries.
#[command]
pub async fn list_devices() -> Result<Vec<DeviceInfo>, String> {
    command_in_non_privileged_context(|_| {
        let config = steamos_mount_core::ListDevicesConfig::new();
        let devices = steamos_mount_core::list_devices(&config)?;

        Ok(devices.iter().map(DeviceInfo::from).collect())
    })
}

/// Gets detailed information about a specific device by UUID.
#[command]
pub async fn get_device_info(uuid: String) -> Result<Option<DeviceInfo>, String> {
    command_in_non_privileged_context(|_| {
        let config = steamos_mount_core::ListDevicesConfig::new();
        let devices = steamos_mount_core::list_devices(&config)?;

        let device = steamos_mount_core::device::find_device_by_uuid(&devices, &uuid);

        match device {
            Some(d) => Ok(Some(DeviceInfo::from(d))),
            None => Ok(None),
        }
    })
}

/// Gets the default mount point for a device.
#[command]
pub async fn get_default_mount_point(uuid: String) -> Result<String, String> {
    command_in_non_privileged_context(|_| {
        let device = steamos_mount_core::find_online_block_device_by_uuid(&uuid)?
            .with_whatever_context(|| format!("Device with UUID {} not found", uuid))?;

        let mount_name = device.suggested_mount_name();
        let mount_point = fstab::generate_mount_point(&mount_name)?;

        Ok(mount_point.display().to_string())
    })
}

/// Previews mount options and fstab entry for a device configuration.
///
/// This command generates a preview of the fstab entry without actually mounting,
/// allowing the UI to show real-time updates as the user changes options.
#[command]
pub async fn preview_mount_options(config: MountConfig) -> Result<FstabPreview, String> {
    command_in_non_privileged_context(|_| {
        // Find the device to get filesystem type and fs_spec
        let device = steamos_mount_core::find_online_block_device_by_uuid(&config.uuid)?
            .with_whatever_context(|| format!("Device with UUID {} not found", config.uuid))?;

        // Get filesystem type
        let fstype = device
            .fstype
            .as_ref()
            .with_whatever_context(|| "Device has no filesystem type")?;
        let fs = preset::SupportedFilesystem::try_from(fstype.as_str())
            .with_whatever_context(|e| format!("Invalid filesystem type: {}", e))?;

        // Get fs_spec for fstab
        let fs_spec = device
            .fstab_spec()
            .with_whatever_context(|| "Could not determine device identifier")?;

        // Build preset config
        let preset_config = config.to_preset_config(fs);

        // Generate options
        let uid = preset::current_uid();
        let gid = preset::current_gid();
        let options = preset_config.generate_options(uid, gid);

        // Generate full fstab line
        let mount_point = std::path::Path::new(&config.mount_point);
        let fstab_line = preset_config.preview_fstab_line(&fs_spec, mount_point, uid, gid);

        Ok(FstabPreview {
            options,
            fstab_line,
        })
    })
}

/// Mounts a device with the specified configuration.
#[command]
pub async fn mount_device(app: AppHandle, config: MountConfig) -> Result<(), String> {
    // All privileged operations are wrapped in command_in_privileged_context
    command_in_privileged_context(&app, |ctx, _| {
        // Find the device by UUID using the new API
        let device = steamos_mount_core::find_online_block_device_by_uuid(&config.uuid)?
            .with_whatever_context(|| format!("Device with UUID {} not found", config.uuid))?;

        // Determine filesystem type (doesn't require privilege)
        let fstype = device
            .fstype
            .as_ref()
            .with_whatever_context(|| "Device has no filesystem type")?;
        let fs = preset::SupportedFilesystem::try_from(fstype.as_str())
            .with_whatever_context(|e| format!("Invalid filesystem type: {}", e))?;

        // Build preset config using orthogonal options from MountConfig
        let preset_config = config.to_preset_config(fs);

        // Generate mount options (doesn't require privilege)
        let uid = preset::current_uid();
        let gid = preset::current_gid();
        let options = preset_config.generate_options(uid, gid);

        // Generate mount point (doesn't require privilege)
        let mount_point = std::path::PathBuf::from(&config.mount_point);
        let force_root_creation = &config.force_root_creation;

        // Get fstab spec (doesn't require privilege)
        let fs_spec = device
            .fstab_spec()
            .with_whatever_context(|| "Could not determine device identifier for fstab")?;

        // Validate that the UUID/PARTUUID path exists (doesn't require privilege)
        device
            .validate_fstab_spec()
            .with_whatever_context(|e| format!("Device identifier validation failed: {}", e))?;

        // Create fstab entry (doesn't require privilege)
        let entry = fstab::FstabEntry::new(fs_spec, &mount_point, fs.driver_name(), options, 0, 0);

        // Check for dirty volume first
        if device.is_ntfs() && !device.is_mounted() {
            let is_dirty = mount::detect_dirty_volume_with_ctx(&device, ctx)?;
            if is_dirty {
                return Err(steamos_mount_core::Error::DirtyVolume {
                    device: device.path.display().to_string(),
                });
            }
        }

        // Create mount point with smart privilege handling
        mount::create_mount_point_smart(&mount_point, ctx, !force_root_creation)?;

        // Backup fstab with privilege escalation
        let fstab_path = std::path::Path::new(fstab::FSTAB_PATH);
        fstab::backup_fstab_with_ctx(fstab_path, ctx)?;

        // Write fstab with privilege escalation
        fstab::add_managed_entries_with_ctx(fstab_path, &[entry], ctx)?;

        // Reload systemd daemon
        mount::reload_systemd_daemon_with_ctx(ctx)?;

        // Mount the device
        mount::mount_device_with_ctx(&device, &mount_point, ctx)?;

        Ok(())
    })
}

/// Unmounts a device from the specified mount point.
///
/// If the device has a managed fstab entry, it will also be deconfigured
/// (removed from fstab) after unmounting.
#[command]
pub async fn unmount_device(app: AppHandle, mount_point: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(&mount_point);
    command_in_privileged_context(&app, |privileged_ctx, _| {
        // First, unmount the device
        mount::unmount_device_with_ctx(&path, privileged_ctx)?;

        // Use the Device API to find the device by mount point
        let config = steamos_mount_core::ListDevicesConfig::new();
        let devices = steamos_mount_core::list_devices(&config)?;

        // Find device by fstab entry mount point and deconfigure if managed
        if let Some(device) = devices.iter().find(|d| {
            d.fstab_entry
                .as_ref()
                .is_some_and(|e| e.mount_point == path)
        }) {
            // Device has a managed fstab entry, deconfigure it using the unified API
            steamos_mount_core::device::deconfigure_device_with_ctx(device, privileged_ctx)?;
        }

        Ok(())
    })
}

/// Removes the fstab configuration for a device (online or offline).
///
/// Uses fs_spec + mount_point for precise matching, supporting scenarios where
/// a single block device has multiple mount points configured.
#[command]
#[allow(dead_code)] // Used by Tauri invoke handler
pub async fn deconfigure_device(
    app: AppHandle,
    fs_spec: String,
    mount_point: String,
) -> Result<(), String> {
    let mount_path = std::path::PathBuf::from(&mount_point);
    command_in_privileged_context(&app, |ctx, _| {
        // Find the device using the unified Device API
        let config = steamos_mount_core::ListDevicesConfig::new();
        let devices = steamos_mount_core::list_devices(&config)?;

        // Find device by fs_spec + mount_point for precise matching
        let device = devices
            .iter()
            .find(|d| {
                d.fstab_entry
                    .as_ref()
                    .is_some_and(|e| e.fs_spec == fs_spec && e.mount_point == mount_path)
            })
            .with_whatever_context(|| {
                format!(
                    "Device with fs_spec {} and mount_point {} not found",
                    fs_spec, mount_point
                )
            })?;

        // Deconfigure using the unified API
        steamos_mount_core::device::deconfigure_device_with_ctx(device, ctx)
    })
}

/// Checks if a device has a dirty NTFS volume.
#[command]
pub async fn check_dirty_volume(app: AppHandle, uuid: String) -> Result<bool, String> {
    command_in_privileged_context(&app, |privileged_ctx, _| {
        let device = steamos_mount_core::find_online_block_device_by_uuid(&uuid)?
            .with_whatever_context(|| format!("Device with UUID {} not found", uuid))?;

        // Check dirty volume requires privilege
        mount::detect_dirty_volume_with_ctx(&device, privileged_ctx)
    })
}

/// Attempts to repair a dirty NTFS volume.
#[command]
pub async fn repair_dirty_volume(app: AppHandle, uuid: String) -> Result<(), String> {
    command_in_privileged_context(&app, |privileged_ctx, _| {
        let device = steamos_mount_core::find_online_block_device_by_uuid(&uuid)?
            .with_whatever_context(|| format!("Device with UUID {} not found", uuid))?;

        mount::repair_dirty_volume_with_ctx(&device, privileged_ctx)
    })
}

/// Detects the default Steam libraryfolders.vdf path.
#[command]
pub async fn detect_steam_library_vdf() -> Result<String, String> {
    command_in_non_privileged_context(|_| {
        steam::steam_library_vdf_path().map(|p| p.display().to_string())
    })
}

/// Injects a Steam library folder.
#[command]
pub async fn inject_steam_library(config: SteamInjectionConfig) -> Result<(), String> {
    command_in_non_privileged_context(|_| {
        let library_path = match &config.library_path {
            Some(path) => config.mount_point.join(path),
            None => config.mount_point.join("SteamLibrary"),
        };

        match config.mode {
            SteamInjectionMode::Auto => {
                // Record Steam running state
                let was_running = steam::is_steam_running();

                // Shutdown Steam if running
                if was_running {
                    steam::shutdown_steam()?;
                }

                // Get VDF path (custom or detected) and inject
                let vdf_path = if let Some(path) = &config.steam_vdf_path {
                    std::path::PathBuf::from(path)
                } else {
                    steam::steam_library_vdf_path()?
                };

                steam::inject_library_folder(&vdf_path, &library_path, "")?;

                // Restart Steam if it was running
                if was_running {
                    Command::new("steam")
                        .spawn()
                        .with_whatever_context(|e| format!("Failed to restart Steam: {}", e))?;
                }

                Ok(())
            }
            SteamInjectionMode::Semi => {
                // Open Steam storage settings
                Command::new("steam")
                    .arg("steam://open/settings/storage")
                    .spawn()
                    .with_whatever_context(|e| format!("Failed to open Steam settings: {}", e))?;

                Ok(())
            }
            SteamInjectionMode::Manual => {
                // Nothing to do, UI will show instructions
                Ok(())
            }
        }
    })
}

/// Checks the state of Steam library configuration.
#[command]
pub async fn get_steam_state(
    steam_vdf_path: Option<String>,
) -> Result<crate::types::SteamState, String> {
    command_in_non_privileged_context(|_| {
        // Determine path
        let path = if let Some(p) = steam_vdf_path {
            std::path::PathBuf::from(p)
        } else {
            match steam::steam_library_vdf_path() {
                Ok(p) => p,
                Err(e) => {
                    return Ok(crate::types::SteamState {
                        is_valid: false,
                        vdf_path: "".to_string(),
                        libraries: vec![],
                        error: Some(e.to_string()),
                    });
                }
            }
        };

        let path_str = path.display().to_string();

        // Try to parse
        match steam::parse_library_folders(&path) {
            Ok(folders) => Ok(crate::types::SteamState {
                is_valid: true,
                vdf_path: path_str,
                libraries: folders
                    .into_iter()
                    .map(|(_, f)| f.path.display().to_string())
                    .collect(),
                error: None,
            }),
            Err(e) => Ok(crate::types::SteamState {
                is_valid: false,
                vdf_path: path_str,
                libraries: vec![],
                error: Some(e.to_string()),
            }),
        }
    })
}

/// Gets a recommended mount configuration for a device.
#[command]
pub async fn get_mount_config_suggestion(
    uuid: String,
) -> Result<crate::types::MountConfigSuggestion, String> {
    command_in_non_privileged_context(|_| {
        let device = steamos_mount_core::find_online_block_device_by_uuid(&uuid)?
            .with_whatever_context(|| format!("Device with UUID {} not found", uuid))?;

        let fstype = device
            .fstype
            .as_ref()
            .with_whatever_context(|| "Device has no filesystem type")?;
        let fs = preset::SupportedFilesystem::try_from(fstype.as_str())
            .with_whatever_context(|e| format!("Invalid filesystem type: {}", e))?;

        let suggestion = steamos_mount_core::preset::suggest_preset_config(
            fs,
            Some(device.rota),
            Some(device.removable),
            device.transport.as_deref(),
        );

        Ok(crate::types::MountConfigSuggestion::from(suggestion))
    })
}

/// Copies text to the system clipboard.
#[command]
pub async fn copy_to_clipboard(text: String) -> Result<(), String> {
    command_in_non_privileged_context(|_| {
        // Try xclip first, then xsel
        let result = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            });

        if result.is_ok() {
            return Ok(());
        }

        // Fallback to xsel
        Command::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
            .with_whatever_context(|e| format!("Failed to copy to clipboard: {}", e))?;

        Ok(())
    })
}
