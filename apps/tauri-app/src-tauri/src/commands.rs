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
use std::process::{Command, Stdio};

use snafu::{OptionExt, ResultExt};
use tauri::AppHandle;
use tauri::{Manager, command};

use steamos_mount_core::{
    DaemonChild, DaemonSpawner, ExecutionContext, PrivilegeEscalation, StdDaemonChild, disk, fstab,
    mount, preset, steam,
};
use tauri::path::BaseDirectory;

use crate::types::{
    DeviceInfo, ManagedEntryInfo, MountConfig, PresetInfo, PresetType, SteamInjectionConfig,
    SteamInjectionMode,
};

// ============================================================================
// Tauri DaemonSpawner implementation
// ============================================================================

/// Spawner that uses Tauri sidecar with pkexec for privilege escalation.
///
/// This spawner resolves the sidecar path using Tauri's resource system
/// and wraps it with pkexec for GUI-based privilege escalation.
struct TauriPkexecSpawner {
    sidecar_path: String,
}

impl TauriPkexecSpawner {
    /// Creates a new TauriPkexecSpawner from an AppHandle.
    fn new(app: &AppHandle) -> Result<Self, steamos_mount_core::Error> {
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

        if !sidecar_appdata_path.exists() {
            if !sidecar_resource_path.exists() {
                return Err(steamos_mount_core::Error::SidecarNotFound {
                    path: sidecar_resource_path.to_string_lossy().to_string(),
                });
            }

            if let Some(sidecar_appdata_dir) = sidecar_appdata_path.parent()
                && !sidecar_appdata_dir.exists()
            {
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
        }

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

        Ok(Self {
            sidecar_path: sidecar_appdata_path.to_string_lossy().to_string(),
        })
    }
}

impl DaemonSpawner for TauriPkexecSpawner {
    fn spawn(&self) -> steamos_mount_core::Result<Box<dyn DaemonChild>> {
        // First, check if sidecar binary exists
        if !std::path::Path::new(&self.sidecar_path).exists() {
            return Err(steamos_mount_core::Error::SidecarNotFound {
                path: self.sidecar_path.clone(),
            });
        }

        // Check if pkexec exists
        if Command::new("pkexec")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return Err(steamos_mount_core::Error::EscalationToolNotFound {
                tool: "pkexec".to_string(),
            });
        }

        // Use std::process::Command with pkexec to spawn the daemon
        // This gives us a std::process::Child that we can wrap in StdDaemonChild
        let child = Command::new("pkexec")
            .arg(&self.sidecar_path)
            .arg("daemon")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                // Check if it's a "command not found" error for pkexec
                if e.kind() == std::io::ErrorKind::NotFound {
                    steamos_mount_core::Error::EscalationToolNotFound {
                        tool: "pkexec".to_string(),
                    }
                } else {
                    steamos_mount_core::Error::SessionCreation {
                        message: format!(
                            "Failed to spawn pkexec with sidecar '{}': {}",
                            self.sidecar_path, e
                        ),
                    }
                }
            })?;

        Ok(Box::new(StdDaemonChild::new(child)))
    }
}

// ============================================================================
// Error conversion utilities
// ============================================================================

/// Converts core library errors to user-friendly error messages.
///
/// This function provides detailed, actionable error messages for different
/// error scenarios, making it easier for users to understand and resolve issues.
fn error_to_user_message(error: &steamos_mount_core::Error) -> String {
    match error {
        steamos_mount_core::Error::SidecarNotFound { path } => {
            format!(
                "Sidecar binary not found at '{}'. This may indicate:\n\
                - The application was not properly installed\n\
                - The binary file is missing or corrupted\n\
                - Please reinstall the application",
                path
            )
        }
        steamos_mount_core::Error::EscalationToolNotFound { tool } => {
            format!(
                "Privilege escalation tool '{}' not found. Please install it:\n\
                - On Debian/Ubuntu: sudo apt install policykit-1\n\
                - On Arch Linux: sudo pacman -S polkit\n\
                - On Fedora: sudo dnf install polkit",
                tool
            )
        }
        steamos_mount_core::Error::AuthenticationCancelled => {
            "Authentication cancelled by user".to_string()
        }
        steamos_mount_core::Error::SessionCommunication { message } => {
            format!("Session communication error: {}", message)
        }
        // For other errors, use the default Display implementation
        _ => error.to_string(),
    }
}

// ============================================================================
// Context creation
// ============================================================================

/// Creates a new execution context with pkexec session for GUI privilege escalation.
///
/// This function creates a new ExecutionContext with a TauriPkexecSpawner.
/// **Each call creates a new context**, ensuring each command requires its own authorization.
/// The daemon is spawned lazily - only when a privileged command is first executed within
/// that command's execution context.
///
/// **Important**: Sessions are not shared between different command invocations. Each
/// command will prompt for authorization when it needs to perform privileged actions.
///
/// Returns a new execution context.
/// Errors are returned as core errors for unified error handling.
fn create_privileged_context(app: &AppHandle) -> steamos_mount_core::Result<ExecutionContext> {
    // Create spawner for lazy session creation
    let spawner = TauriPkexecSpawner::new(app)
        .with_whatever_context(|e| format!("Failed to create spawner: {}", e))?;

    // Create execution context with the spawner
    // The session will be created lazily when first needed
    let ctx = ExecutionContext::with_spawner(PrivilegeEscalation::PkexecSession, Box::new(spawner));

    Ok(ctx)
}

/// Creates a new non-privileged execution context.
///
/// This function creates a new default ExecutionContext without privilege escalation.
/// Each call creates a new context instance.
/// Errors are returned as core errors for unified error handling.
fn get_non_privileged_context() -> steamos_mount_core::Result<ExecutionContext> {
    let ctx = ExecutionContext::default();
    Ok(ctx)
}

/// Executes a command that requires privilege escalation context,
/// with automatic error conversion for core library errors.
///
/// This function provides a unified wrapper for commands that need privileged execution.
/// **Each call creates a new privileged context**, requiring its own authorization.
/// Sessions are not shared between different command invocations.
///
/// It handles:
/// - Creating a new privileged context for this command (with error conversion for sidecar/pkexec errors)
/// - Creating a non-privileged context for operations that don't require escalation
/// - Converting privilege-related errors to user-friendly messages
/// - Locking the contexts for thread-safe access
///
/// # Arguments
/// * `app` - The Tauri AppHandle
/// * `command_impl` - A closure that receives both privileged and non-privileged contexts
///   and performs the actual command logic
///
/// # Returns
/// * `Ok(T)` - The result from the command implementation
/// * `Err(String)` - User-friendly error message
///
/// # Example
/// ```ignore
/// command_in_privileged_context(&app, |privileged_ctx, non_privileged_ctx| {
///     // Use privileged_ctx for operations requiring root
///     mount::mount_device_with_ctx(device, &mount_point, privileged_ctx)
/// })
/// ```
fn command_in_privileged_context<F, T>(app: &AppHandle, command_impl: F) -> Result<T, String>
where
    F: FnOnce(&mut ExecutionContext, &mut ExecutionContext) -> steamos_mount_core::Result<T>,
{
    // Create a new privileged context for this command (each command requires its own authorization)
    let mut privileged_ctx =
        create_privileged_context(app).map_err(|e| error_to_user_message(&e))?;
    let mut non_privileged_ctx =
        get_non_privileged_context().map_err(|e| error_to_user_message(&e))?;

    // Execute the command implementation and convert errors
    command_impl(&mut privileged_ctx, &mut non_privileged_ctx)
        .map_err(|e| error_to_user_message(&e))
}

/// Executes a command that does not require privilege escalation context,
/// with automatic error conversion for core library errors.
///
/// This function provides a unified wrapper for commands that don't need privileged execution.
/// It handles:
/// - Creating a new non-privileged context for this command
/// - Converting errors to user-friendly messages
/// - Locking the context for thread-safe access
///
/// # Arguments
/// * `command_impl` - A closure that receives the locked context and performs the actual command logic
///
/// # Returns
/// * `Ok(T)` - The result from the command implementation
/// * `Err(String)` - User-friendly error message
///
/// # Example
/// ```ignore
/// command_in_non_privileged_context(|_ctx| {
///     disk::list_block_devices()
/// })
/// ```
fn command_in_non_privileged_context<F, T>(command_impl: F) -> Result<T, String>
where
    F: FnOnce(&mut ExecutionContext) -> steamos_mount_core::Result<T>,
{
    // Get or create non-privileged context
    let mut ctx = get_non_privileged_context().map_err(|e| error_to_user_message(&e))?;

    // Execute the command implementation and convert errors
    command_impl(&mut ctx).map_err(|e| error_to_user_message(&e))
}

/// Lists all mountable devices (NTFS/exFAT partitions).
#[command]
pub async fn list_devices() -> Result<Vec<DeviceInfo>, String> {
    command_in_non_privileged_context(|_| {
        let devices = disk::list_block_devices()?;

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
    })
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

/// Removes the fstab configuration for a device (deconfigure).
///
/// This is an internal helper function that performs the actual deconfiguration:
/// - Filters out the device's fstab entry
/// - Backs up fstab
/// - Writes remaining entries
/// - Reloads systemd daemon
///
/// # Arguments
/// * `device` - The device to deconfigure
/// * `ctx` - The execution context for privileged operations
///
/// # Returns
/// * `Ok(())` - Success
/// * `Err(Error)` - If the device is not configured in fstab or if deconfiguration fails
fn deconfigure_device_internal(
    device: &disk::BlockDevice,
    ctx: &mut ExecutionContext,
) -> steamos_mount_core::Result<()> {
    // Read current fstab entries
    let fstab_path = std::path::Path::new(fstab::FSTAB_PATH);
    let parsed = fstab::parse_fstab(fstab_path)?;

    // Find the entry matching this device
    let _entry_to_remove = parsed
        .managed_entries
        .iter()
        .find(|e| device_matches_entry(device, e))
        .whatever_context("Device is not configured in fstab")?;

    // Filter out the entry to remove
    let remaining_entries: Vec<fstab::FstabEntry> = parsed
        .managed_entries
        .into_iter()
        .filter(|e| !device_matches_entry(device, e))
        .collect();

    // Backup fstab with privilege escalation
    fstab::backup_fstab_with_ctx(fstab_path, ctx)?;

    // Write remaining entries with privilege escalation
    fstab::write_managed_entries_with_ctx(fstab_path, &remaining_entries, ctx)?;

    // Reload systemd daemon
    mount::reload_systemd_daemon_with_ctx(ctx)?;

    Ok(())
}

/// Gets detailed information about a specific device by UUID.
#[command]
pub async fn get_device_info(uuid: String) -> Result<Option<DeviceInfo>, String> {
    command_in_non_privileged_context(|_| {
        let devices = disk::list_block_devices()?;

        let device = devices.iter().find(|d| d.uuid.as_ref() == Some(&uuid));

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
        let devices = disk::list_block_devices()?;

        let device = devices
            .iter()
            .find(|d| d.uuid.as_ref() == Some(&uuid))
            .with_whatever_context(|| format!("Device with UUID {} not found", uuid))?;

        let mount_name = device.suggested_mount_name();
        let mount_point = fstab::generate_mount_point(&mount_name)?;

        Ok(mount_point.display().to_string())
    })
}

/// Mounts a device with the specified configuration.
#[command]
pub async fn mount_device(app: AppHandle, config: MountConfig) -> Result<(), String> {
    // All privileged operations are wrapped in command_in_privileged_context
    command_in_privileged_context(&app, |ctx, _| {
        // Find the device by UUID (doesn't require privilege)
        let devices = disk::list_block_devices()?;
        let device = devices
            .iter()
            .find(|d| d.uuid.as_ref().map(|u| u == &config.uuid).unwrap_or(false))
            .with_whatever_context(|| format!("Device with UUID {} not found", config.uuid))?;

        // Determine filesystem type (doesn't require privilege)
        let fstype = device
            .fstype
            .as_ref()
            .with_whatever_context(|| "Device has no filesystem type")?;
        let fs = preset::SupportedFilesystem::try_from(fstype.as_str())
            .with_whatever_context(|e| format!("Invalid filesystem type: {}", e))?;

        // Build preset config (doesn't require privilege)
        let preset_config = match config.preset {
            PresetType::Ssd => preset::MountPreset::ssd_defaults(fs),
            PresetType::Portable => preset::MountPreset::portable_defaults(fs),
            PresetType::Custom => {
                preset::MountPreset::custom(fs, config.custom_options.as_deref().unwrap_or(""))
            }
        };

        // Generate mount options (doesn't require privilege)
        let uid = preset::current_uid();
        let gid = preset::current_gid();
        let options = preset_config.generate_options(uid, gid);

        // Generate mount point (doesn't require privilege)
        let mount_point = std::path::PathBuf::from(&config.mount_point);
        let device_uuid = &config.uuid;
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

        // Re-find the device (needed for reference)
        let devices = disk::list_block_devices()?;
        let device = devices
            .iter()
            .find(|d| d.uuid.as_ref() == Some(device_uuid))
            .with_whatever_context(|| format!("Device with UUID {} not found", device_uuid))?;

        // Check for dirty volume first
        if device.is_ntfs()
            && !device.is_mounted()
            && let Ok(is_dirty) = mount::detect_dirty_volume_with_ctx(device, ctx)
            && is_dirty
        {
            return Err(steamos_mount_core::Error::DirtyVolume {
                device: device.path.display().to_string(),
            });
        }

        // Create mount point with smart privilege handling
        mount::create_mount_point_smart(&mount_point, ctx, !force_root_creation)?;

        // Backup fstab with privilege escalation
        let fstab_path = std::path::Path::new(fstab::FSTAB_PATH);
        fstab::backup_fstab_with_ctx(fstab_path, ctx)?;

        // Write fstab with privilege escalation
        fstab::write_managed_entries_with_ctx(fstab_path, &[entry], ctx)?;

        // Reload systemd daemon
        mount::reload_systemd_daemon_with_ctx(ctx)?;

        // Mount the device
        mount::mount_device_with_ctx(device, &mount_point, ctx)?;

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
    command_in_privileged_context(&app, |priviledged_ctx, _| {
        // First, unmount the device
        mount::unmount_device_with_ctx(&path, priviledged_ctx)?;

        // Find the device by mount point (after unmount, mountpoint will be None,
        // so we need to find it by matching the mount point path)
        let devices = disk::list_block_devices()?;

        // Try to find device by checking fstab entries that match this mount point
        let fstab_path = std::path::Path::new(fstab::FSTAB_PATH);
        let parsed = fstab::parse_fstab(fstab_path)?;

        // Find fstab entry that matches this mount point
        if let Some(entry) = parsed
            .managed_entries
            .iter()
            .find(|e| e.mount_point == path)
        {
            // Find the device that matches this fstab entry
            if let Some(device) = devices.iter().find(|d| device_matches_entry(d, entry)) {
                // Device has a managed fstab entry, deconfigure it
                deconfigure_device_internal(device, priviledged_ctx)?;
            }
        }

        Ok(())
    })
}

/// Removes the fstab configuration for a device (deconfigure).
#[command]
#[allow(dead_code)] // Used by Tauri invoke handler
pub async fn deconfigure_device(app: AppHandle, uuid: String) -> Result<(), String> {
    // All privileged operations are wrapped in command_in_privileged_context
    command_in_privileged_context(&app, |privledged_ctx, _| {
        // Find the device by UUID (doesn't require privilege)
        let devices = disk::list_block_devices()?;
        let device = devices
            .iter()
            .find(|d| d.uuid.as_ref() == Some(&uuid))
            .with_whatever_context(|| format!("Device with UUID {} not found", uuid))?;

        // Deconfigure the device
        deconfigure_device_internal(device, privledged_ctx)?;

        Ok(())
    })
}

/// Checks if a device has a dirty NTFS volume.
#[command]
pub async fn check_dirty_volume(app: AppHandle, uuid: String) -> Result<bool, String> {
    // Prepare non-privileged data (device lookup)
    let device_uuid = uuid.clone();
    command_in_privileged_context(&app, |priviledged_ctx, _| {
        // Re-find the device (needed for reference)
        let devices = disk::list_block_devices()?;
        let device = devices
            .iter()
            .find(|d| d.uuid.as_ref() == Some(&device_uuid))
            .with_whatever_context(|| format!("Device with UUID {} not found", device_uuid))?;

        // Check dirty volume requires privilege
        mount::detect_dirty_volume_with_ctx(device, priviledged_ctx)
    })
}

/// Attempts to repair a dirty NTFS volume.
#[command]
pub async fn repair_dirty_volume(app: AppHandle, uuid: String) -> Result<(), String> {
    // Repair dirty volume requires privilege
    let device_uuid = uuid.clone();
    command_in_privileged_context(&app, |privileged_ctx, _| {
        // Find the device by UUID (doesn't require privilege, but done here for error handling)
        let devices = disk::list_block_devices()?;
        let device = devices
            .iter()
            .find(|d| d.uuid.as_ref() == Some(&device_uuid))
            .with_whatever_context(|| format!("Device with UUID {} not found", device_uuid))?;
        mount::repair_dirty_volume_with_ctx(device, privileged_ctx)
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

/// Gets available presets for a filesystem type.
#[command]
pub async fn get_presets(filesystem: String) -> Result<Vec<PresetInfo>, String> {
    command_in_non_privileged_context(|_| {
        let fs = preset::SupportedFilesystem::try_from(filesystem.as_str())?;

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
