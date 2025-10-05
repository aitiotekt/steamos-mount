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
    /// The fstab fs_spec (e.g. "UUID=...", "LABEL=...", or device path)
    pub fs_spec: Option<String>,
    /// Steam libraries under this device's mount point
    pub steam_libraries: Vec<SteamLibraryInfo>,
    /// Whether the device is rotational (HDD)
    pub rota: Option<bool>,
    /// Whether the device is removable
    pub removable: Option<bool>,
    /// Transport type (e.g., "usb", "nvme")
    pub transport: Option<String>,
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
            fs_spec: None,       // Will be populated if matched with fstab entry
            steam_libraries: Vec::new(),
            rota: Some(device.rota),
            removable: Some(device.removable),
            transport: device.transport.clone(),
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
            fs_spec: Some(device.fs_spec.clone()),
            steam_libraries: Vec::new(),
            rota: None,
            removable: None,
            transport: None,
        }
    }
}

/// Implement conversion from core Device to DeviceInfo.
impl From<&steamos_mount_core::Device> for DeviceInfo {
    fn from(device: &steamos_mount_core::Device) -> Self {
        Self {
            name: device.name.clone(),
            path: device
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            label: device.label.clone(),
            uuid: device.uuid.clone(),
            partuuid: device.partuuid.clone(),
            fstype: device.fstype.clone(),
            size: device.size,
            mountpoint: device
                .effective_mount_point()
                .map(|p| p.display().to_string()),
            is_mounted: device.is_mounted,
            is_dirty: device.is_dirty,
            is_offline: device.is_offline(),
            managed_entry: device.fstab_entry.as_ref().map(|e| ManagedEntryInfo {
                mount_point: e.mount_point.display().to_string(),
                options: e.mount_options.clone(),
                raw_content: e.to_fstab_line(),
            }),
            fs_spec: device.fs_spec.clone(),
            steam_libraries: device
                .steam_libraries
                .iter()
                .map(SteamLibraryInfo::from)
                .collect(),
            rota: device.rota,
            removable: device.removable,
            transport: device.transport.clone(),
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

/// Steam library information for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamLibraryInfo {
    /// Path to the library folder.
    pub path: String,
    /// Optional label for the library.
    pub label: String,
}

impl From<&steamos_mount_core::LibraryFolder> for SteamLibraryInfo {
    fn from(lib: &steamos_mount_core::LibraryFolder) -> Self {
        Self {
            path: lib.path.display().to_string(),
            label: lib.label.clone(),
        }
    }
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
    /// Media type (Flash/Rotational)
    pub media_type: MediaType,
    /// Device type (Fixed/Removable)
    pub device_type: DeviceType,
    /// Device timeout in seconds (for Fixed devices)
    pub device_timeout_secs: Option<u32>,
    /// Idle timeout in seconds (for Removable devices)
    pub idle_timeout_secs: Option<u32>,
    /// Custom mount options (appended to generated options)
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

impl MountConfig {
    /// Converts to core PresetConfig.
    pub fn to_preset_config(
        &self,
        filesystem: steamos_mount_core::preset::SupportedFilesystem,
    ) -> steamos_mount_core::preset::PresetConfig {
        steamos_mount_core::preset::PresetConfig {
            filesystem,
            media_type: self.media_type.into(),
            device_type: self.device_type.into(),
            timeout: steamos_mount_core::preset::TimeoutConfig {
                device_timeout_secs: self.device_timeout_secs,
                idle_timeout_secs: self.idle_timeout_secs,
            },
            custom_options: self.custom_options.clone(),
        }
    }
}

/// Fstab line preview response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FstabPreview {
    /// Generated mount options string
    pub options: String,
    /// Complete fstab line
    pub fstab_line: String,
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

/// Metadata for a UI option.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionMetadata {
    pub value: String,
    pub label: String,
    pub description: String,
    pub recommended: bool,
}

impl From<steamos_mount_core::preset::OptionMetadata> for OptionMetadata {
    fn from(meta: steamos_mount_core::preset::OptionMetadata) -> Self {
        Self {
            value: meta.value,
            label: meta.label,
            description: meta.description,
            recommended: meta.recommended,
        }
    }
}

/// Preset configuration DTO for suggestions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetConfigDto {
    pub media_type: MediaType,
    pub device_type: DeviceType,
    pub device_timeout_secs: Option<u32>,
    pub idle_timeout_secs: Option<u32>,
}

/// Suggestion for mount configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountConfigSuggestion {
    pub default_config: PresetConfigDto,
    pub connection_type_options: Vec<OptionMetadata>,
    pub media_type_options: Vec<OptionMetadata>,
    pub device_timeout_desc: String,
    pub idle_timeout_desc: String,
}

impl From<steamos_mount_core::preset::MountConfigSuggestion> for MountConfigSuggestion {
    fn from(suggestion: steamos_mount_core::preset::MountConfigSuggestion) -> Self {
        Self {
            default_config: PresetConfigDto {
                media_type: match suggestion.default_config.media_type {
                    steamos_mount_core::preset::MediaType::Flash => MediaType::Flash,
                    steamos_mount_core::preset::MediaType::Rotational => MediaType::Rotational,
                },
                device_type: match suggestion.default_config.device_type {
                    steamos_mount_core::preset::DeviceType::Fixed => DeviceType::Fixed,
                    steamos_mount_core::preset::DeviceType::Removable => DeviceType::Removable,
                },
                device_timeout_secs: suggestion.default_config.timeout.device_timeout_secs,
                idle_timeout_secs: suggestion.default_config.timeout.idle_timeout_secs,
            },
            connection_type_options: suggestion
                .connection_type_options
                .into_iter()
                .map(OptionMetadata::from)
                .collect(),
            media_type_options: suggestion
                .media_type_options
                .into_iter()
                .map(OptionMetadata::from)
                .collect(),
            device_timeout_desc: suggestion.device_timeout_desc,
            idle_timeout_desc: suggestion.idle_timeout_desc,
        }
    }
}
