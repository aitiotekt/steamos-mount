//! Unified device abstraction module.
//!
//! This module provides a unified `Device` type that combines information from:
//! - `BlockDevice` (lsblk): Physical block device information
//! - `FstabEntry` (fstab): Managed mount configuration
//! - `LibraryFolder` (Steam VDF): Steam library associations
//!
//! The `Device` type is the primary interface for UI/UX layers and serves as:
//! - A complete view of device state for user display
//! - A filter key for operations with side effects

use std::path::{Path, PathBuf};

use crate::disk::{self, BlockDevice, OfflineDevice};
use crate::error::Result;
use crate::fstab::{self, FstabEntry};
use crate::steam::{self, LibraryFolder};

/// Represents the connection state of a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceConnectionState {
    /// Device is currently connected and visible to the system.
    Online,
    /// Device is configured in fstab but not currently connected.
    Offline,
}

/// Unified device information combining block device, fstab, and Steam data.
///
/// This is the primary type for UI/UX interaction. It aggregates:
/// - Physical device properties (from lsblk)
/// - Mount configuration (from fstab)  
/// - Steam library associations (from Steam VDF)
#[derive(Debug, Clone)]
pub struct Device {
    // === Identification ===
    /// Display name for the device (label, mount point basename, or fs_spec).
    pub name: String,
    /// The fstab fs_spec identifier (e.g., "UUID=xxx", "PARTUUID=xxx", "LABEL=xxx").
    /// None for unmanaged online devices.
    pub fs_spec: Option<String>,
    /// Full device path (e.g., "/dev/sda1"). None for offline devices.
    pub path: Option<PathBuf>,

    // === Device Properties ===
    /// Volume label (if available).
    pub label: Option<String>,
    /// Filesystem UUID.
    pub uuid: Option<String>,
    /// Partition UUID.
    pub partuuid: Option<String>,
    /// Filesystem type (e.g., "ntfs", "exfat").
    pub fstype: String,
    /// Size in bytes. 0 for offline devices.
    pub size: u64,
    /// Whether the device is rotational (HDD). None if unknown (offline).
    pub rota: Option<bool>,
    /// Whether the device is removable. None if unknown (offline).
    pub removable: Option<bool>,
    /// Transport type (e.g., "usb", "nvme"). None if unknown/none.
    pub transport: Option<String>,

    // === State ===
    /// Current mount point (if mounted).
    mountpoint: Option<PathBuf>,
    /// Whether the device is currently mounted.
    pub is_mounted: bool,
    /// Whether the device has a dirty NTFS volume (needs repair).
    pub is_dirty: bool,
    /// Connection state (online/offline).
    pub connection_state: DeviceConnectionState,

    // === Associated Data (Full Information) ===
    /// The complete fstab entry if this device is managed.
    pub fstab_entry: Option<FstabEntry>,
    /// Steam libraries whose paths are under this device's mount point.
    /// Multiple libraries can exist under a single mount point.
    pub steam_libraries: Vec<LibraryFolder>,
}

impl Device {
    /// Returns true if this device is offline.
    pub fn is_offline(&self) -> bool {
        self.connection_state == DeviceConnectionState::Offline
    }

    /// Returns true if this device is managed (has fstab configuration).
    pub fn is_managed(&self) -> bool {
        self.fstab_entry.is_some()
    }

    /// Returns the effective mount point (actual or configured target).
    pub fn effective_mount_point(&self) -> Option<&Path> {
        self.mountpoint
            .as_deref()
            .or_else(|| self.fstab_entry.as_ref().map(|e| e.mount_point.as_path()))
    }

    /// Creates a Device from an online BlockDevice.
    fn from_block_device(device: &BlockDevice) -> Self {
        Self {
            name: device.label.clone().unwrap_or_else(|| device.name.clone()),
            fs_spec: None, // Will be populated if matched with fstab
            path: Some(device.path.clone()),
            label: device.label.clone(),
            uuid: device.uuid.clone(),
            partuuid: device.partuuid.clone(),
            fstype: device.fstype.clone().unwrap_or_default(),
            size: device.size,
            rota: Some(device.rota),
            removable: Some(device.removable),
            transport: device.transport.clone(),
            mountpoint: device.mountpoint.as_ref().map(PathBuf::from),
            is_mounted: device.is_mounted(),
            is_dirty: false, // Will be checked separately
            connection_state: DeviceConnectionState::Online,
            fstab_entry: None,
            steam_libraries: Vec::new(),
        }
    }

    /// Creates a Device from an offline fstab entry.
    fn from_offline_entry(entry: &FstabEntry) -> Self {
        let offline = OfflineDevice::from_fstab_entry(entry);

        // Derive name from mount point basename or fs_spec
        let name = entry
            .mount_point
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| entry.fs_spec.clone());

        Self {
            name,
            fs_spec: Some(entry.fs_spec.clone()),
            path: None,
            label: offline.label,
            uuid: offline.uuid,
            partuuid: offline.partuuid,
            fstype: disk::vfs_type_to_fstype(&entry.vfs_type).to_string(),
            size: 0,
            rota: None,
            removable: None,
            transport: None,
            mountpoint: Some(entry.mount_point.clone()),
            is_mounted: false,
            is_dirty: false,
            connection_state: DeviceConnectionState::Offline,
            fstab_entry: Some(entry.clone()),
            steam_libraries: Vec::new(),
        }
    }

    /// Attaches fstab entry information to this device.
    fn attach_fstab_entry(&mut self, entry: &FstabEntry) {
        self.fstab_entry = Some(entry.clone());
        self.fs_spec = Some(entry.fs_spec.clone());
        self.mountpoint = Some(entry.mount_point.clone());
    }

    /// Attaches matching Steam libraries to this device.
    fn attach_steam_libraries(&mut self, libraries: &[(String, LibraryFolder)]) {
        if let Some(mount_point) = self.effective_mount_point() {
            // Find all libraries whose path starts with this device's mount point
            self.steam_libraries = libraries
                .iter()
                .filter(|(_, lib)| lib.path.starts_with(mount_point))
                .map(|(_, lib)| lib.clone())
                .collect();
        }
    }
}

/// Checks if a block device matches an fstab entry.
pub fn device_matches_fstab_entry(device: &BlockDevice, entry: &FstabEntry) -> bool {
    if let Some(uuid) = entry.fs_spec.strip_prefix("UUID=") {
        if let Some(dev_uuid) = &device.uuid {
            return uuid == dev_uuid;
        }
    } else if let Some(partuuid) = entry.fs_spec.strip_prefix("PARTUUID=") {
        if let Some(dev_partuuid) = &device.partuuid {
            return partuuid == dev_partuuid;
        }
    } else if let Some(label) = entry.fs_spec.strip_prefix("LABEL=") {
        if let Some(dev_label) = &device.label {
            return label == dev_label;
        }
    } else {
        // Path match
        return entry.fs_spec == device.path.display().to_string();
    }
    false
}

/// Configuration for listing devices.
#[derive(Debug, Clone, Default)]
pub struct ListDevicesConfig {
    /// Path to fstab file. Defaults to /etc/fstab.
    pub fstab_path: Option<PathBuf>,
    /// Path to Steam's libraryfolders.vdf. If None, will attempt auto-detection.
    pub steam_vdf_path: Option<PathBuf>,
    /// Whether to include Steam library information.
    pub include_steam: bool,
}

impl ListDevicesConfig {
    /// Creates a new config with default settings.
    pub fn new() -> Self {
        Self {
            fstab_path: None,
            steam_vdf_path: None,
            include_steam: true,
        }
    }

    /// Sets the fstab path.
    pub fn with_fstab_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.fstab_path = Some(path.into());
        self
    }

    /// Disables Steam library detection.
    pub fn without_steam(mut self) -> Self {
        self.include_steam = false;
        self
    }
}

/// Lists all devices (online + offline managed), with fstab and Steam associations.
///
/// This is the primary API for obtaining device information. It:
/// 1. Scans for online block devices via lsblk
/// 2. Parses fstab for managed entries
/// 3. Merges online devices with offline entries (avoiding duplicates)
/// 4. Attaches Steam library information based on mount point matching
///
/// # Arguments
/// * `config` - Configuration for device listing
///
/// # Returns
/// A vector of `Device` structs representing all known devices.
pub fn list_devices(config: &ListDevicesConfig) -> Result<Vec<Device>> {
    let fstab_path = config
        .fstab_path
        .as_deref()
        .unwrap_or_else(|| Path::new(fstab::FSTAB_PATH));

    // Step 1: Get online block devices
    let online_devices = disk::list_block_devices()?;
    let mountable = disk::filter_mountable_devices(&online_devices);

    // Step 2: Parse fstab for managed entries
    let fstab_entries = fstab::parse_fstab(fstab_path)
        .map(|parsed| parsed.managed_entries)
        .unwrap_or_default();

    // Step 3: Get Steam libraries if enabled
    let steam_libraries: Vec<(String, LibraryFolder)> = if config.include_steam {
        config
            .steam_vdf_path
            .as_ref()
            .and_then(|p| steam::parse_library_folders(p).ok())
            .or_else(|| {
                steam::steam_library_vdf_path()
                    .ok()
                    .and_then(|p| steam::parse_library_folders(&p).ok())
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Step 4: Build device list
    let mut devices: Vec<Device> = Vec::new();
    let mut matched_entries: Vec<&FstabEntry> = Vec::new();

    // Process online devices
    for block_device in mountable {
        let mut device = Device::from_block_device(block_device);

        // Check for matching fstab entry
        if let Some(entry) = fstab_entries
            .iter()
            .find(|e| device_matches_fstab_entry(block_device, e))
        {
            device.attach_fstab_entry(entry);
            matched_entries.push(entry);
        }

        // Attach Steam libraries
        device.attach_steam_libraries(&steam_libraries);

        devices.push(device);
    }

    // Add offline devices (fstab entries without matching online devices)
    for entry in &fstab_entries {
        if !matched_entries.iter().any(|e| e.fs_spec == entry.fs_spec) {
            let mut device = Device::from_offline_entry(entry);
            device.attach_steam_libraries(&steam_libraries);
            devices.push(device);
        }
    }

    Ok(devices)
}

/// Finds a device by UUID.
pub fn find_device_by_uuid<'a>(devices: &'a [Device], uuid: &str) -> Option<&'a Device> {
    devices
        .iter()
        .find(|d| d.uuid.as_deref().is_some_and(|u| u == uuid))
}

/// Finds a device by fs_spec.
pub fn find_device_by_fs_spec<'a>(devices: &'a [Device], fs_spec: &str) -> Option<&'a Device> {
    devices
        .iter()
        .find(|d| d.fs_spec.as_deref().is_some_and(|f| f == fs_spec))
}

/// Finds a device by mount point.
pub fn find_device_by_mount_point<'a>(
    devices: &'a [Device],
    mount_point: &Path,
) -> Option<&'a Device> {
    devices
        .iter()
        .find(|d| d.effective_mount_point().is_some_and(|p| p == mount_point))
}

/// Finds a BlockDevice by UUID from online devices.
///
/// This is useful when operations require the underlying BlockDevice
/// (e.g., mount, repair) rather than the unified Device abstraction.
///
/// # Returns
/// The matching BlockDevice, or None if not found.
pub fn find_online_block_device_by_uuid(uuid: &str) -> Result<Option<BlockDevice>> {
    let devices = disk::list_block_devices()?;
    Ok(devices
        .into_iter()
        .find(|d| d.uuid.as_deref().is_some_and(|u| u == uuid)))
}

/// Finds a BlockDevice by path from online devices.
///
/// # Returns
/// The matching BlockDevice, or None if not found.
pub fn find_online_block_device_by_path(path: &Path) -> Result<Option<BlockDevice>> {
    let devices = disk::list_block_devices()?;
    Ok(devices.into_iter().find(|d| d.path == path))
}

use crate::executor::ExecutionContext;
use crate::mount;

/// Deconfigures a device by removing its managed fstab entry.
///
/// This function removes the fstab entry associated with the device and reloads
/// systemd daemon. Works for both online and offline devices.
///
/// # Matching Strategy
/// Uses precise matching with both `fs_spec` AND `mount_point` from the device's
/// fstab_entry, supporting scenarios where a single block device has multiple
/// mount points configured.
///
/// # Arguments
/// * `device` - The device to deconfigure (must have fstab_entry)
/// * `ctx` - Execution context for privileged operations
///
/// # Errors
/// Returns error if:
/// - Device has no fstab_entry (not managed)
/// - Fstab backup fails
/// - Entry removal fails
/// - Systemd daemon reload fails
pub fn deconfigure_device_with_ctx(device: &Device, ctx: &mut ExecutionContext) -> Result<()> {
    use snafu::OptionExt;

    // Device must have an fstab entry to deconfigure
    let entry = device
        .fstab_entry
        .as_ref()
        .whatever_context("Device is not configured in fstab (no managed entry)")?;

    let fstab_path = Path::new(fstab::FSTAB_PATH);

    // Backup fstab with privilege escalation
    fstab::backup_fstab_with_ctx(fstab_path, ctx)?;

    // Remove matching entries using fs_spec + mount_point for precise matching
    let target_fs_spec = &entry.fs_spec;
    let target_mount_point = &entry.mount_point;

    fstab::remove_managed_entries_with_ctx(fstab_path, ctx, |e| {
        e.fs_spec == *target_fs_spec && e.mount_point == *target_mount_point
    })?;

    // Reload systemd daemon
    mount::reload_systemd_daemon_with_ctx(ctx)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_connection_state() {
        let device = Device {
            name: "test".to_string(),
            fs_spec: None,
            path: Some(PathBuf::from("/dev/sda1")),
            label: None,
            uuid: Some("1234-5678".to_string()),
            partuuid: None,
            fstype: "ntfs".to_string(),
            size: 1024,
            rota: None,
            removable: None,
            transport: None,
            mountpoint: None,
            is_mounted: false,
            is_dirty: false,
            connection_state: DeviceConnectionState::Online,
            fstab_entry: None,
            steam_libraries: Vec::new(),
        };

        assert!(!device.is_offline());
        assert!(!device.is_managed());
    }

    #[test]
    fn test_steam_library_prefix_matching() {
        let mut device = Device {
            name: "Games".to_string(),
            fs_spec: Some("UUID=1234".to_string()),
            path: Some(PathBuf::from("/dev/sda1")),
            label: Some("Games".to_string()),
            uuid: Some("1234".to_string()),
            partuuid: None,
            fstype: "ntfs".to_string(),
            size: 1024,
            rota: None,
            removable: None,
            transport: None,
            mountpoint: Some(PathBuf::from("/mnt/games")),
            is_mounted: true,
            is_dirty: false,
            connection_state: DeviceConnectionState::Online,
            fstab_entry: None,
            steam_libraries: Vec::new(),
        };

        let libraries = vec![
            (
                "0".to_string(),
                LibraryFolder {
                    path: PathBuf::from("/home/user/.steam"),
                    label: "Default".to_string(),
                    contentid: "0".to_string(),
                    totalsize: "0".to_string(),
                    apps: Default::default(),
                },
            ),
            (
                "1".to_string(),
                LibraryFolder {
                    path: PathBuf::from("/mnt/games/SteamLibrary"),
                    label: "Games".to_string(),
                    contentid: "0".to_string(),
                    totalsize: "0".to_string(),
                    apps: Default::default(),
                },
            ),
            (
                "2".to_string(),
                LibraryFolder {
                    path: PathBuf::from("/mnt/games/SteamLibrary2"),
                    label: "Games2".to_string(),
                    contentid: "0".to_string(),
                    totalsize: "0".to_string(),
                    apps: Default::default(),
                },
            ),
        ];

        device.attach_steam_libraries(&libraries);

        // Should match 2 libraries under /mnt/games
        assert_eq!(device.steam_libraries.len(), 2);
        assert!(device.steam_libraries.iter().any(|l| l.label == "Games"));
        assert!(device.steam_libraries.iter().any(|l| l.label == "Games2"));
    }
}
