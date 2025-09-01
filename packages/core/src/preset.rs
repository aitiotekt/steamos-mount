//! Mount preset definitions for different device types.
//!
//! This module provides flexible mount option generation based on:
//! - Filesystem (NTFS, exFAT, etc.)
//! - Storage Media (Flash/SSD vs HDD)
//! - Device Scenario (Fixed vs Removable)

use serde::{Deserialize, Serialize};

/// Default user ID for the deck user on SteamOS.
pub const DEFAULT_UID: u32 = 1000;

/// Default group ID for the deck user on SteamOS.
pub const DEFAULT_GID: u32 = 1000;

/// Default options applied to all mounts.
pub const BASE_OPTIONS: &str = "umask=000,nofail,rw,noatime";

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

/// Configuration for mount option generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetConfig {
    pub filesystem: SupportedFilesystem,
    pub media_type: MediaType,
    pub device_type: DeviceType,
    pub custom_options: Option<String>,
}

impl PresetConfig {
    /// Creates a new configuration with defaults.
    pub fn new(filesystem: SupportedFilesystem) -> Self {
        Self {
            filesystem,
            media_type: MediaType::default(),
            device_type: DeviceType::default(),
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

        // 4. Device Type Specifics
        match self.device_type {
            DeviceType::Fixed => {
                opts.push("x-systemd.device-timeout=3s".to_string());
            }
            DeviceType::Removable => {
                opts.push("noauto".to_string());
                opts.push("x-systemd.automount".to_string());
                opts.push("x-systemd.idle-timeout=60s".to_string());
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
            custom_options: None,
        }
    }

    /// Preset for Portable Devices (Hot-swappable).
    pub fn portable_defaults(fs: SupportedFilesystem) -> Self {
        Self {
            filesystem: fs,
            media_type: MediaType::Flash, // Assume portable are mostly flash
            device_type: DeviceType::Removable,
            custom_options: None,
        }
    }

    /// Creates a custom preset.
    pub fn custom(fs: SupportedFilesystem, options: &str) -> Self {
        Self {
            filesystem: fs,
            media_type: MediaType::default(),
            device_type: DeviceType::default(),
            custom_options: Some(options.to_string()),
        }
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
}
