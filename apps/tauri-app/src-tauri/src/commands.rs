//! Tauri commands for the steamos-mount desktop application.
//!
//! These commands bridge the frontend UI with the core library functionality.

use std::io::Write;
use std::process::{Command, Stdio};

use tauri::command;

use steamos_mount_core::{disk, fstab, mount, preset, steam, ExecutionContext};

use crate::types::{
    DeviceInfo, ManagedEntryInfo, MountConfig, MountResult, PresetInfo, PresetType,
    SteamInjectionConfig, SteamInjectionMode,
};

/// Creates an execution context with pkexec session for GUI privilege escalation.
fn gui_context() -> ExecutionContext {
    ExecutionContext::with_pkexec_session()
}

/// Lists all mountable devices (NTFS/exFAT partitions).
#[command]
pub async fn list_devices() -> Result<Vec<DeviceInfo>, String> {
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;

    // Read fstab to check for configured entries
    // We ignore errors here as fstab might not exist or be readable, which is fine
    let fstab_entries = fstab::parse_fstab(std::path::Path::new(fstab::FSTAB_PATH))
        .map(|parsed| parsed.managed_entries)
        .unwrap_or_default();

    let mountable = disk::filter_mountable_devices(&devices);

    let result: Vec<DeviceInfo> = mountable
        .iter()
        .map(|d| {
            let mut info = DeviceInfo::from(*d);

            // Check if device is configured in fstab
            if let Some(entry) = fstab_entries.iter().find(|e| device_matches_entry(d, e)) {
                info.managed_entry = Some(ManagedEntryInfo {
                    mount_point: entry.mount_point.display().to_string(),
                    options: entry.mount_options.clone(),
                    raw_content: entry.to_fstab_line(),
                });
            }

            info
        })
        .collect();

    Ok(result)
}

fn device_matches_entry(device: &disk::BlockDevice, entry: &fstab::FstabEntry) -> bool {
    if entry.fs_spec.starts_with("UUID=") {
        if let Some(uuid) = &device.uuid {
            return entry.fs_spec == format!("UUID={}", uuid);
        }
    } else if entry.fs_spec.starts_with("PARTUUID=") {
        if let Some(partuuid) = &device.partuuid {
            return entry.fs_spec == format!("PARTUUID={}", partuuid);
        }
    } else if entry.fs_spec.starts_with("LABEL=") {
        if let Some(label) = &device.label {
            return entry.fs_spec == format!("LABEL={}", label);
        }
    } else {
        // Path match
        return entry.fs_spec == device.path.display().to_string();
    }
    false
}

/// Gets detailed information about a specific device by UUID.
#[command]
pub async fn get_device_info(uuid: String) -> Result<Option<DeviceInfo>, String> {
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;

    let device = devices.iter().find(|d| d.uuid.as_ref() == Some(&uuid));

    match device {
        Some(d) => Ok(Some(DeviceInfo::from(d))),
        None => Ok(None),
    }
}

/// Gets the default mount point for a device.
#[command]
pub async fn get_default_mount_point(uuid: String) -> Result<String, String> {
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;

    let device = devices
        .iter()
        .find(|d| d.uuid.as_ref() == Some(&uuid))
        .ok_or_else(|| format!("Device with UUID {} not found", uuid))?;

    let mount_name = device.suggested_mount_name();
    let mount_point = fstab::generate_mount_point(&mount_name).map_err(|e| e.to_string())?;

    Ok(mount_point.display().to_string())
}

/// Mounts a device with the specified configuration.
#[command]
pub async fn mount_device(config: MountConfig) -> Result<MountResult, String> {
    let mut ctx = gui_context();

    // Find the device by UUID
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;
    let device = devices
        .iter()
        .find(|d| d.uuid.as_ref().map(|u| u == &config.uuid).unwrap_or(false))
        .ok_or_else(|| format!("Device with UUID {} not found", config.uuid))?;

    // Determine filesystem type
    let fstype = device
        .fstype
        .as_ref()
        .ok_or("Device has no filesystem type")?;
    let fs = preset::SupportedFilesystem::try_from(fstype.as_str()).map_err(|e| e.to_string())?;

    // Check for dirty volume first
    if device.is_ntfs() && !device.is_mounted() {
        if let Ok(is_dirty) = mount::detect_dirty_volume_with_ctx(device, &mut ctx) {
            if is_dirty {
                return Ok(MountResult {
                    success: false,
                    mount_point: "".to_string(),
                    error: Some(format!(
                        "Device {} has a dirty NTFS volume. Please repair it first.",
                        device.path.display()
                    )),
                });
            }
        }
    }

    // Build preset config
    let preset_config = match config.preset {
        PresetType::Ssd => preset::MountPreset::ssd_defaults(fs),
        PresetType::Portable => preset::MountPreset::portable_defaults(fs),
        PresetType::Custom => {
            preset::MountPreset::custom(fs, config.custom_options.as_deref().unwrap_or(""))
        }
    };

    // Generate mount options
    let uid = preset::current_uid();
    let gid = preset::current_gid();
    let options = preset_config.generate_options(uid, gid);

    // Generate mount point
    // Generate mount point
    let mount_point = if let Some(path) = &config.mount_point {
        std::path::PathBuf::from(path)
    } else {
        let mount_name = device.suggested_mount_name();
        fstab::generate_mount_point(&mount_name).map_err(|e| e.to_string())?
    };
    let mount_point_str = mount_point.display().to_string();

    // Create mount point with smart privilege handling
    // If not forced, try as user first
    if let Err(e) =
        mount::create_mount_point_smart(&mount_point, &mut ctx, !config.force_root_creation)
    {
        return Ok(MountResult {
            success: false,
            mount_point: mount_point_str,
            error: Some(e.to_string()),
        });
    }

    // Get fstab spec
    let fs_spec = device
        .fstab_spec()
        .ok_or("Could not determine device identifier for fstab")?;

    // Validate that the UUID/PARTUUID path exists
    device
        .validate_fstab_spec()
        .map_err(|e| format!("Device identifier validation failed: {}", e))?;

    // Create fstab entry
    let entry = fstab::FstabEntry::new(fs_spec, &mount_point, fs.driver_name(), options, 0, 0);

    // Backup fstab with privilege escalation
    let fstab_path = std::path::Path::new(fstab::FSTAB_PATH);
    fstab::backup_fstab_with_ctx(fstab_path, &mut ctx).map_err(|e| e.to_string())?;

    // Write fstab with privilege escalation
    fstab::write_managed_entries_with_ctx(fstab_path, &[entry], &mut ctx)
        .map_err(|e| e.to_string())?;

    // Reload systemd daemon
    mount::reload_systemd_daemon_with_ctx(&mut ctx).map_err(|e| e.to_string())?;

    // Mount the device
    match mount::mount_device_with_ctx(device, &mount_point, &mut ctx) {
        Ok(()) => Ok(MountResult {
            success: true,
            mount_point: mount_point_str,
            error: None,
        }),
        Err(steamos_mount_core::Error::DirtyVolume { device: dev }) => Ok(MountResult {
            success: false,
            mount_point: mount_point_str,
            error: Some(format!(
                "Device {} has a dirty NTFS volume. Please repair it first.",
                dev
            )),
        }),
        Err(steamos_mount_core::Error::AuthenticationCancelled) => Ok(MountResult {
            success: false,
            mount_point: mount_point_str,
            error: Some("Authentication cancelled by user".to_string()),
        }),
        Err(e) => Ok(MountResult {
            success: false,
            mount_point: mount_point_str,
            error: Some(e.to_string()),
        }),
    }
}

/// Unmounts a device from the specified mount point.
#[command]
pub async fn unmount_device(mount_point: String) -> Result<(), String> {
    let mut ctx = gui_context();
    let path = std::path::PathBuf::from(&mount_point);
    mount::unmount_device_with_ctx(&path, &mut ctx).map_err(|e| e.to_string())
}

/// Removes the fstab configuration for a device (deconfigure).
#[command]
#[allow(dead_code)] // Used by Tauri invoke handler
pub async fn deconfigure_device(uuid: String) -> Result<(), String> {
    let mut ctx = gui_context();

    // Find the device by UUID
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;
    let device = devices
        .iter()
        .find(|d| d.uuid.as_ref() == Some(&uuid))
        .ok_or_else(|| format!("Device with UUID {} not found", uuid))?;

    // Read current fstab entries
    let fstab_path = std::path::Path::new(fstab::FSTAB_PATH);
    let parsed = fstab::parse_fstab(fstab_path).map_err(|e| e.to_string())?;

    // Find the entry matching this device
    let entry_to_remove = parsed
        .managed_entries
        .iter()
        .find(|e| device_matches_entry(device, e));

    if entry_to_remove.is_none() {
        return Err("Device is not configured in fstab".to_string());
    }

    // Filter out the entry to remove
    let remaining_entries: Vec<fstab::FstabEntry> = parsed
        .managed_entries
        .into_iter()
        .filter(|e| !device_matches_entry(device, e))
        .collect();

    // Backup fstab with privilege escalation
    fstab::backup_fstab_with_ctx(fstab_path, &mut ctx).map_err(|e| e.to_string())?;

    // Write remaining entries with privilege escalation
    fstab::write_managed_entries_with_ctx(fstab_path, &remaining_entries, &mut ctx)
        .map_err(|e| e.to_string())?;

    // Reload systemd daemon
    mount::reload_systemd_daemon_with_ctx(&mut ctx).map_err(|e| e.to_string())?;

    Ok(())
}

/// Checks if a device has a dirty NTFS volume.
#[command]
pub async fn check_dirty_volume(uuid: String) -> Result<bool, String> {
    let mut ctx = gui_context();
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;
    let device = devices
        .iter()
        .find(|d| d.uuid.as_ref() == Some(&uuid))
        .ok_or_else(|| format!("Device with UUID {} not found", uuid))?;

    mount::detect_dirty_volume_with_ctx(device, &mut ctx).map_err(|e| e.to_string())
}

/// Attempts to repair a dirty NTFS volume.
#[command]
pub async fn repair_dirty_volume(uuid: String) -> Result<(), String> {
    let mut ctx = gui_context();
    let devices = disk::list_block_devices().map_err(|e| e.to_string())?;
    let device = devices
        .iter()
        .find(|d| d.uuid.as_ref() == Some(&uuid))
        .ok_or_else(|| format!("Device with UUID {} not found", uuid))?;

    mount::repair_dirty_volume_with_ctx(device, &mut ctx).map_err(|e| e.to_string())
}

/// Detects the default Steam libraryfolders.vdf path.
#[command]
pub async fn detect_steam_library_vdf() -> Result<String, String> {
    steam::steam_library_vdf_path()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// Injects a Steam library folder.
#[command]
pub async fn inject_steam_library(config: SteamInjectionConfig) -> Result<(), String> {
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
                steam::shutdown_steam().map_err(|e| e.to_string())?;
            }

            // Get VDF path (custom or detected) and inject
            let vdf_path = if let Some(path) = &config.steam_vdf_path {
                std::path::PathBuf::from(path)
            } else {
                steam::steam_library_vdf_path().map_err(|e| e.to_string())?
            };

            steam::inject_library_folder(&vdf_path, &library_path, "")
                .map_err(|e| e.to_string())?;

            // Restart Steam if it was running
            if was_running {
                Command::new("steam")
                    .spawn()
                    .map_err(|e| format!("Failed to restart Steam: {}", e))?;
            }

            Ok(())
        }
        SteamInjectionMode::Semi => {
            // Open Steam storage settings
            Command::new("steam")
                .arg("steam://open/settings/storage")
                .spawn()
                .map_err(|e| format!("Failed to open Steam settings: {}", e))?;

            Ok(())
        }
        SteamInjectionMode::Manual => {
            // Nothing to do, UI will show instructions
            Ok(())
        }
    }
}

/// Checks the state of Steam library configuration.
#[command]
pub async fn get_steam_state(
    steam_vdf_path: Option<String>,
) -> Result<crate::types::SteamState, String> {
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
}

/// Gets available presets for a filesystem type.
#[command]
pub async fn get_presets(filesystem: String) -> Result<Vec<PresetInfo>, String> {
    let fs =
        preset::SupportedFilesystem::try_from(filesystem.as_str()).map_err(|e| e.to_string())?;

    let uid = preset::current_uid();
    let gid = preset::current_gid();

    let ssd_preset = preset::MountPreset::ssd_defaults(fs);
    let portable_preset = preset::MountPreset::portable_defaults(fs);

    Ok(vec![
        PresetInfo {
            id: "ssd".to_string(),
            name: "Internal SSD".to_string(),
            description: "High performance settings for internal or fixed drives".to_string(),
            options_preview: ssd_preset.generate_options(uid, gid),
        },
        PresetInfo {
            id: "portable".to_string(),
            name: "Portable Drive".to_string(),
            description: "Safe settings for removable drives with auto-mount".to_string(),
            options_preview: portable_preset.generate_options(uid, gid),
        },
        PresetInfo {
            id: "custom".to_string(),
            name: "Custom".to_string(),
            description: "Manually configure mount options".to_string(),
            options_preview: "".to_string(),
        },
    ])
}

/// Copies text to the system clipboard.
#[command]
pub async fn copy_to_clipboard(text: String) -> Result<(), String> {
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
        .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;

    Ok(())
}
