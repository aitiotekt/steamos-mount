//! Mount preset definitions for different device types.
//!
//! This module provides flexible mount option generation based on:
//! - Filesystem (NTFS, exFAT, etc.)
//! - Storage Media (Flash/SSD vs HDD)
//! - Device Scenario (Fixed vs Removable)

use serde::{Deserialize, Serialize};

/// Default user ID (first regular user on most Linux systems).
pub const DEFAULT_UID: u32 = 1000;

/// Default group ID (first regular user's group on most Linux systems).
pub const DEFAULT_GID: u32 = 1000;

/// Returns the current user's UID.
///
/// This supports SteamOS-like systems (ChimeraOS, Bazzite, HoloISO, etc.)
/// where the primary user may not have UID 1000.
pub fn current_uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

/// Returns the current user's primary GID.
pub fn current_gid() -> u32 {
    nix::unistd::getgid().as_raw()
}

/// Default options applied to all mounts.
pub const BASE_OPTIONS: &str = "umask=000,nofail,rw,noatime";

/// Default device timeout for internal devices (seconds).
pub const DEFAULT_DEVICE_TIMEOUT_SECS: u32 = 3;

/// Default idle timeout for removable devices (seconds).
pub const DEFAULT_IDLE_TIMEOUT_SECS: u32 = 60;

/// Supported filesystem types for preset generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportedFilesystem {
    Ntfs,
    Exfat,
}

impl TryFrom<&str> for SupportedFilesystem {
    type Error = crate::error::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "ntfs" | "ntfs3" => Ok(SupportedFilesystem::Ntfs),
            "exfat" => Ok(SupportedFilesystem::Exfat),
            _ => Err(crate::error::Error::InvalidFilesystem { fs: s.to_string() }),
        }
    }
}

impl SupportedFilesystem {
    /// Returns the preferred kernel driver name.
    pub fn driver_name(&self) -> &'static str {
        match self {
            Self::Ntfs => "ntfs3",
            Self::Exfat => "exfat",
        }
    }
}

/// Storage media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MediaType {
    /// Flash storage (SSD, SD Card, USB Stick).
    #[default]
    Flash,
    /// Rotational hard drive (HDD).
    Rotational,
}

/// Device connection scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DeviceType {
    /// Internal or permanently connected devices.
    #[default]
    Fixed,
    /// Hot-swappable devices (SD cards, portable drives).
    Removable,
}

/// Timeout configuration for systemd mount options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// x-systemd.device-timeout in seconds (for Fixed devices).
    /// Set to None to omit this option.
    pub device_timeout_secs: Option<u32>,
    /// x-systemd.idle-timeout in seconds (for Removable devices).
    /// Set to None to omit this option.
    pub idle_timeout_secs: Option<u32>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            device_timeout_secs: Some(DEFAULT_DEVICE_TIMEOUT_SECS),
            idle_timeout_secs: Some(DEFAULT_IDLE_TIMEOUT_SECS),
        }
    }
}

/// Configuration for mount option generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetConfig {
    pub filesystem: SupportedFilesystem,
    pub media_type: MediaType,
    pub device_type: DeviceType,
    pub timeout: TimeoutConfig,
    pub custom_options: Option<String>,
}

impl PresetConfig {
    /// Creates a new configuration with defaults.
    pub fn new(filesystem: SupportedFilesystem) -> Self {
        Self {
            filesystem,
            media_type: MediaType::default(),
            device_type: DeviceType::default(),
            timeout: TimeoutConfig::default(),
            custom_options: None,
        }
    }

    /// Generates the mount options string.
    pub fn generate_options(&self, uid: u32, gid: u32) -> String {
        let mut opts = Vec::new();

        // 1. General Configuration
        opts.push(format!("uid={},gid={}", uid, gid));
        opts.push(BASE_OPTIONS.to_string());

        // 2. Filesystem Specifics
        if self.filesystem == SupportedFilesystem::Ntfs {
            opts.push("prealloc".to_string());
        }

        // 3. Media Specifics
        if self.media_type == MediaType::Flash {
            opts.push("discard".to_string());
        }

        // 4. Device Type Specifics with configurable timeouts
        match self.device_type {
            DeviceType::Fixed => {
                if let Some(timeout) = self.timeout.device_timeout_secs {
                    opts.push(format!("x-systemd.device-timeout={}s", timeout));
                }
            }
            DeviceType::Removable => {
                opts.push("noauto".to_string());
                opts.push("x-systemd.automount".to_string());
                if let Some(timeout) = self.timeout.idle_timeout_secs {
                    opts.push(format!("x-systemd.idle-timeout={}s", timeout));
                }
            }
        }

        // 5. Custom Options
        match &self.custom_options {
            Some(custom) if !custom.is_empty() => {
                opts.push(custom.clone());
            }
            _ => {}
        }

        opts.join(",")
    }

    /// Generates a complete fstab line preview.
    ///
    /// # Arguments
    /// * `fs_spec` - The device identifier (e.g., "UUID=xxx", "PARTUUID=xxx")
    /// * `mount_point` - The mount point path
    /// * `uid` - User ID for ownership
    /// * `gid` - Group ID for ownership
    ///
    /// # Returns
    /// A complete fstab line suitable for display or writing.
    pub fn preview_fstab_line(
        &self,
        fs_spec: &str,
        mount_point: &std::path::Path,
        uid: u32,
        gid: u32,
    ) -> String {
        let options = self.generate_options(uid, gid);
        let vfs_type = self.filesystem.driver_name();
        format!(
            "{}  {}  {}  {}  0  0",
            fs_spec,
            mount_point.display(),
            vfs_type,
            options
        )
    }
}

// For backward compatibility / ease of use during transition
pub type MountPreset = PresetConfig;

impl MountPreset {
    /// Preset for Internal/Fixed SSDs (High Performance).
    pub fn ssd_defaults(fs: SupportedFilesystem) -> Self {
        Self {
            filesystem: fs,
            media_type: MediaType::Flash,
            device_type: DeviceType::Fixed,
            timeout: TimeoutConfig::default(),
            custom_options: None,
        }
    }

    /// Preset for Portable Devices (Hot-swappable).
    pub fn portable_defaults(fs: SupportedFilesystem) -> Self {
        Self {
            filesystem: fs,
            media_type: MediaType::Flash, // Assume portable are mostly flash
            device_type: DeviceType::Removable,
            timeout: TimeoutConfig::default(),
            custom_options: None,
        }
    }

    /// Creates a custom preset.
    pub fn custom(fs: SupportedFilesystem, options: &str) -> Self {
        Self {
            filesystem: fs,
            media_type: MediaType::default(),
            device_type: DeviceType::default(),
            timeout: TimeoutConfig::default(),
            custom_options: Some(options.to_string()),
        }
    }
}

/// Metadata for a UI option.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptionMetadata {
    /// Option value (e.g., "fixed", "removable").
    pub value: String,
    /// Display label.
    pub label: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this is the recommended option.
    pub recommended: bool,
}

/// Suggestion for mount configuration and UI metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfigSuggestion {
    /// Recommended configuration.
    pub default_config: PresetConfig,

    /// Options for connection type (Fixed vs Removable).
    pub connection_type_options: Vec<OptionMetadata>,

    /// Options for media type (Flash vs Rotational).
    pub media_type_options: Vec<OptionMetadata>,

    /// Description for device timeout (Fixed).
    pub device_timeout_desc: String,

    /// Description for idle timeout (Removable).
    pub idle_timeout_desc: String,
}

/// Suggests a mount configuration based on device properties.
pub fn suggest_preset_config(
    filesystem: SupportedFilesystem,
    rota: Option<bool>,
    removable: Option<bool>,
    transport: Option<&str>,
) -> MountConfigSuggestion {
    // 1. Determine Recommended Values
    let is_removable = removable.unwrap_or(false) || transport == Some("usb");
    let recommended_device_type = if is_removable {
        DeviceType::Removable
    } else {
        DeviceType::Fixed
    };

    let is_rotational = rota.unwrap_or(false);
    let recommended_media_type = if is_rotational {
        MediaType::Rotational
    } else {
        MediaType::Flash
    };

    let default_config = PresetConfig {
        filesystem,
        media_type: recommended_media_type,
        device_type: recommended_device_type,
        timeout: TimeoutConfig::default(),
        custom_options: None,
    };

    // 2. Build Option Metadata with Descriptions
    let connection_type_options = vec![
        OptionMetadata {
            value: "fixed".to_string(),
            label: "Internal / Fixed".to_string(),
            description: "Always connected. Waits for device at boot (systemd device timeout)."
                .to_string(),
            recommended: recommended_device_type == DeviceType::Fixed,
        },
        OptionMetadata {
            value: "removable".to_string(),
            label: "Removable".to_string(),
            description: "Hot-swappable. Auto-mounts on access (systemd automount).".to_string(),
            recommended: recommended_device_type == DeviceType::Removable,
        },
    ];

    let media_type_options = vec![
        OptionMetadata {
            value: "flash".to_string(),
            label: "Flash (SSD / SD)".to_string(),
            description: " optimized for flash storage. Enables TRIM/Discard.".to_string(),
            recommended: recommended_media_type == MediaType::Flash,
        },
        OptionMetadata {
            value: "rotational".to_string(),
            label: "Rotational (HDD)".to_string(),
            description: "Optimized for spinning disks. Disables TRIM to avoid errors.".to_string(),
            recommended: recommended_media_type == MediaType::Rotational,
        },
    ];

    MountConfigSuggestion {
        default_config,
        connection_type_options,
        media_type_options,
        device_timeout_desc: "Time to wait for device at boot before failing.".to_string(),
        idle_timeout_desc: "Time before unmounting idle device.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssd_preset_ntfs() {
        let preset = PresetConfig::new(SupportedFilesystem::Ntfs); // Default is Flash/Fixed
        let options = preset.generate_options(1000, 1000);

        assert!(options.contains("uid=1000,gid=1000"));
        assert!(options.contains("rw,noatime"));
        assert!(options.contains("discard")); // Flash
        assert!(options.contains("prealloc")); // NTFS
        assert!(options.contains("x-systemd.device-timeout=3s")); // Fixed
        assert!(!options.contains("noauto"));
    }

    #[test]
    fn test_ssd_preset_exfat() {
        let preset = PresetConfig::new(SupportedFilesystem::Exfat);
        let options = preset.generate_options(1000, 1000);

        assert!(options.contains("discard"));
        assert!(!options.contains("prealloc")); // Not for exFAT
    }

    #[test]
    fn test_portable_preset() {
        let preset = PresetConfig {
            filesystem: SupportedFilesystem::Exfat,
            media_type: MediaType::Flash,
            device_type: DeviceType::Removable,
            timeout: TimeoutConfig::default(),
            custom_options: None,
        };
        let options = preset.generate_options(1000, 1000);

        assert!(options.contains("noauto"));
        assert!(options.contains("x-systemd.automount"));
        assert!(options.contains("x-systemd.idle-timeout=60s"));
        assert!(!options.contains("prealloc"));
    }

    #[test]
    fn test_custom_preset() {
        let preset = PresetConfig::custom(SupportedFilesystem::Ntfs, "rw,sync");
        let options = preset.generate_options(1000, 1000);

        assert!(options.contains("uid=1000"));
        assert!(options.contains("rw,sync"));
    }

    #[test]
    fn test_custom_ids() {
        let preset = PresetConfig::new(SupportedFilesystem::Ntfs);
        let options = preset.generate_options(1001, 1002);

        assert!(options.contains("uid=1001"));
        assert!(options.contains("gid=1002"));
    }

    #[test]
    fn test_rotational_defaults() {
        // Rotational Drive (HDD)
        let mut preset = PresetConfig::new(SupportedFilesystem::Exfat);
        preset.media_type = MediaType::Rotational;

        let options = preset.generate_options(1000, 1000);
        assert!(!options.contains("discard")); // HDD should not have discard
    }

    #[test]
    fn test_driver_selection() {
        assert_eq!(SupportedFilesystem::Ntfs.driver_name(), "ntfs3");
        assert_eq!(SupportedFilesystem::Exfat.driver_name(), "exfat");
    }

    #[test]
    fn test_suggestion_logic() {
        // USB -> Removable
        let sugg = suggest_preset_config(
            SupportedFilesystem::Exfat,
            Some(false),
            Some(false),
            Some("usb"),
        );
        assert_eq!(sugg.default_config.device_type, DeviceType::Removable);
        assert!(
            sugg.connection_type_options
                .iter()
                .find(|o| o.value == "removable")
                .unwrap()
                .recommended
        );

        // HDD -> Rotational
        let sugg = suggest_preset_config(SupportedFilesystem::Ntfs, Some(true), Some(false), None);
        assert_eq!(sugg.default_config.media_type, MediaType::Rotational);

        // NVMe -> Fixed, Flash
        let sugg = suggest_preset_config(
            SupportedFilesystem::Ntfs,
            Some(false),
            Some(false),
            Some("nvme"),
        );
        assert_eq!(sugg.default_config.device_type, DeviceType::Fixed);
        assert_eq!(sugg.default_config.media_type, MediaType::Flash);

        // Explicit Removable Flag -> Removable
        let sugg = suggest_preset_config(SupportedFilesystem::Exfat, Some(false), Some(true), None);
        assert_eq!(sugg.default_config.device_type, DeviceType::Removable);
    }
}
