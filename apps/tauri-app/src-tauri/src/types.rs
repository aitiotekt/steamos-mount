//! Data transfer types for the Tauri frontend.
//!
//! These types are designed for serialization between Rust and TypeScript.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Device information for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Device name (e.g., "nvme0n1p3")
    pub name: String,
    /// Full device path (e.g., "/dev/nvme0n1p3")
    pub path: String,
    /// Volume label (if available)
    pub label: Option<String>,
    /// Filesystem UUID
    pub uuid: Option<String>,
    /// Partition UUID
    pub partuuid: Option<String>,
    /// Filesystem type (e.g., "ntfs", "exfat")
    pub fstype: String,
    /// Size in bytes
    pub size: u64,
    /// Current mount point (if mounted)
    pub mountpoint: Option<String>,
    /// Whether the device is currently mounted
    pub is_mounted: bool,
    /// Whether the device has a dirty NTFS volume
    pub is_dirty: bool,
    /// Whether the device is offline (in fstab but not connected)
    pub is_offline: bool,
    /// Managed fstab configuration if available
    pub managed_entry: Option<ManagedEntryInfo>,
}

impl From<&steamos_mount_core::BlockDevice> for DeviceInfo {
    fn from(device: &steamos_mount_core::BlockDevice) -> Self {
        Self {
            name: device.name.clone(),
            path: device.path.display().to_string(),
            label: device.label.clone(),
            uuid: device.uuid.clone(),
            partuuid: device.partuuid.clone(),
            fstype: device.fstype.clone().unwrap_or_default(),
            size: device.size,
            mountpoint: device.mountpoint.clone(),
            is_mounted: device.is_mounted(),
            is_dirty: false,     // Will be checked separately
            is_offline: false,   // Online device
            managed_entry: None, // Will be populated separately
        }
    }
}

impl From<&steamos_mount_core::OfflineDevice> for DeviceInfo {
    fn from(device: &steamos_mount_core::OfflineDevice) -> Self {
        Self {
            name: device
                .mount_point
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| device.fs_spec.clone()),
            path: String::new(), // No path for offline devices
            label: device.label.clone(),
            uuid: device.uuid.clone(),
            partuuid: device.partuuid.clone(),
            fstype: steamos_mount_core::vfs_type_to_fstype(&device.vfs_type).to_string(),
            size: 0,          // Unknown size for offline devices
            mountpoint: None, // Not mounted
            is_mounted: false,
            is_dirty: false,
            is_offline: true, // Offline device
            managed_entry: Some(ManagedEntryInfo {
                mount_point: device.mount_point.display().to_string(),
                options: device.mount_options.clone(),
                raw_content: device.to_fstab_line(),
            }),
        }
    }
}

/// Managed fstab entry information for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEntryInfo {
    /// Configured mount point
    pub mount_point: String,
    /// Mount options
    pub options: Vec<String>,
    /// Raw fstab entry content (line)
    pub raw_content: String,
}

/// Mount preset type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetType {
    Ssd,
    Portable,
    Custom,
}

/// Storage media type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Flash,
    Rotational,
}

impl From<MediaType> for steamos_mount_core::preset::MediaType {
    fn from(media: MediaType) -> Self {
        match media {
            MediaType::Flash => Self::Flash,
            MediaType::Rotational => Self::Rotational,
        }
    }
}

/// Device connection type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Fixed,
    Removable,
}

impl From<DeviceType> for steamos_mount_core::preset::DeviceType {
    fn from(device: DeviceType) -> Self {
        match device {
            DeviceType::Fixed => Self::Fixed,
            DeviceType::Removable => Self::Removable,
        }
    }
}

/// Mount configuration from UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountConfig {
    /// Device UUID to mount
    pub uuid: String,
    /// Preset type
    pub preset: PresetType,
    /// Media type (SSD/HDD)
    pub media_type: MediaType,
    /// Device type (Fixed/Removable)
    pub device_type: DeviceType,
    /// Custom mount options (only for Custom preset)
    pub custom_options: Option<String>,
    /// Custom mount point path
    pub mount_point: String,
    /// Whether to force root privileges for mount point creation
    pub force_root_creation: bool,
    /// Whether to inject Steam library
    pub inject_steam: bool,
    /// Steam library path (relative to mount point)
    pub steam_library_path: Option<String>,
}

/// Steam injection mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SteamInjectionMode {
    /// Automatic: shutdown Steam, modify VDF, restore state
    Auto,
    /// Semi-automatic: open Steam settings for manual add
    Semi,
    /// Manual: just show instructions
    Manual,
}

/// Steam injection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamInjectionConfig {
    /// Mount point path
    pub mount_point: PathBuf,
    /// Library path (defaults to {mount_point}/SteamLibrary)
    pub library_path: Option<String>,
    /// Path to libraryfolders.vdf (optional, overrides default detection)
    pub steam_vdf_path: Option<String>,
    /// Injection mode
    pub mode: SteamInjectionMode,
}

/// Status of the Steam library configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamState {
    /// Whether the VDF file is valid and readable
    pub is_valid: bool,
    /// The path to the VDF file used
    pub vdf_path: String,
    /// List of library folder paths found
    pub libraries: Vec<String>,
    /// Error message if invalid
    pub error: Option<String>,
}

/// Preset information for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    /// Preset identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Generated mount options preview
    pub options_preview: String,
}
