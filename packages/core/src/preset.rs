//! Mount preset definitions for different device types.
//!
//! This module provides predefined mount option sets for common scenarios:
//! - SSD: Internal/fixed drives optimized for performance
//! - Portable: Hot-swappable devices like SD cards and USB drives

use serde::{Deserialize, Serialize};

/// Default user ID for the deck user on SteamOS.
pub const DEFAULT_UID: u32 = 1000;

/// Default group ID for the deck user on SteamOS.
pub const DEFAULT_GID: u32 = 1000;

/// Mount preset for different device scenarios.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MountPreset {
    /// Internal/Fixed SSD - High performance mode.
    ///
    /// Optimized for NVMe SSDs or permanently connected external SSDs.
    /// Features:
    /// - `ntfs3` kernel driver for high performance
    /// - `rw,noatime` to reduce metadata writes
    /// - `discard` for TRIM support (SSD longevity)
    /// - `prealloc` (NTFS only) for reduced fragmentation
    /// - `x-systemd.device-timeout=3s` for quick skip on missing devices
    #[default]
    Ssd,

    /// Hot-swappable SD Card/USB Drive - Portable mode.
    ///
    /// Optimized for devices that are frequently plugged/unplugged.
    /// Features:
    /// - `noauto` to prevent boot-time mounting
    /// - `x-systemd.automount` for on-demand mounting
    /// - `x-systemd.idle-timeout=60s` for automatic unmount after inactivity
    Portable,

    /// Custom mount options provided by the user.
    Custom(String),
}

impl MountPreset {
    /// Generates the mount options string for fstab.
    ///
    /// # Arguments
    /// * `fstype` - Filesystem type ("ntfs" or "exfat")
    /// * `uid` - User ID for file ownership
    /// * `gid` - Group ID for file ownership
    ///
    /// # Returns
    /// A comma-separated string of mount options.
    pub fn to_options(&self, fstype: &str, uid: u32, gid: u32) -> String {
        let base_options = format!("uid={},gid={},umask=000,nofail", uid, gid);

        match self {
            MountPreset::Ssd => {
                let mut options = vec![
                    base_options,
                    "rw".to_string(),
                    "noatime".to_string(),
                    "discard".to_string(),
                    "x-systemd.device-timeout=3s".to_string(),
                ];

                // NTFS-specific options
                if fstype == "ntfs" {
                    options.push("prealloc".to_string());
                }

                options.join(",")
            }
            MountPreset::Portable => [
                base_options.as_str(),
                "noauto",
                "x-systemd.automount",
                "x-systemd.idle-timeout=60s",
            ]
            .join(","),
            MountPreset::Custom(opts) => {
                // Prepend base options to custom options
                format!("{},{}", base_options, opts)
            }
        }
    }

    /// Returns a human-readable description of the preset.
    pub fn description(&self) -> &'static str {
        match self {
            MountPreset::Ssd => "Internal/Fixed SSD - High performance with TRIM and preallocation",
            MountPreset::Portable => {
                "Hot-swappable SD Card/USB - On-demand mount with auto-unmount"
            }
            MountPreset::Custom(_) => "Custom mount options",
        }
    }
}

/// Configuration for mount operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    /// User ID for file ownership.
    pub uid: u32,
    /// Group ID for file ownership.
    pub gid: u32,
    /// Mount preset to use.
    pub preset: MountPreset,
}

impl Default for PresetConfig {
    fn default() -> Self {
        Self {
            uid: DEFAULT_UID,
            gid: DEFAULT_GID,
            preset: MountPreset::default(),
        }
    }
}

impl PresetConfig {
    /// Creates a new preset configuration with default UID/GID.
    pub fn new(preset: MountPreset) -> Self {
        Self {
            uid: DEFAULT_UID,
            gid: DEFAULT_GID,
            preset,
        }
    }

    /// Creates a preset configuration with custom UID/GID.
    pub fn with_ids(preset: MountPreset, uid: u32, gid: u32) -> Self {
        Self { uid, gid, preset }
    }

    /// Generates mount options for the given filesystem type.
    pub fn generate_options(&self, fstype: &str) -> String {
        self.preset.to_options(fstype, self.uid, self.gid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssd_preset_ntfs() {
        let preset = MountPreset::Ssd;
        let options = preset.to_options("ntfs", 1000, 1000);

        assert!(options.contains("uid=1000"));
        assert!(options.contains("gid=1000"));
        assert!(options.contains("umask=000"));
        assert!(options.contains("nofail"));
        assert!(options.contains("rw"));
        assert!(options.contains("noatime"));
        assert!(options.contains("discard"));
        assert!(options.contains("prealloc"));
        assert!(options.contains("x-systemd.device-timeout=3s"));
    }

    #[test]
    fn test_ssd_preset_exfat() {
        let preset = MountPreset::Ssd;
        let options = preset.to_options("exfat", 1000, 1000);

        // exFAT should not have prealloc
        assert!(!options.contains("prealloc"));
        assert!(options.contains("discard"));
    }

    #[test]
    fn test_portable_preset() {
        let preset = MountPreset::Portable;
        let options = preset.to_options("ntfs", 1000, 1000);

        assert!(options.contains("noauto"));
        assert!(options.contains("x-systemd.automount"));
        assert!(options.contains("x-systemd.idle-timeout=60s"));
        // Portable should NOT have discard or prealloc
        assert!(!options.contains("discard"));
        assert!(!options.contains("prealloc"));
    }

    #[test]
    fn test_custom_preset() {
        let preset = MountPreset::Custom("rw,sync".to_string());
        let options = preset.to_options("ntfs", 1000, 1000);

        assert!(options.contains("uid=1000"));
        assert!(options.contains("rw,sync"));
    }

    #[test]
    fn test_preset_config_generate_options() {
        let config = PresetConfig::new(MountPreset::Ssd);
        let options = config.generate_options("ntfs");

        assert!(options.contains("uid=1000"));
        assert!(options.contains("gid=1000"));
    }

    #[test]
    fn test_preset_config_custom_ids() {
        let config = PresetConfig::with_ids(MountPreset::Ssd, 1001, 1002);
        let options = config.generate_options("ntfs");

        assert!(options.contains("uid=1001"));
        assert!(options.contains("gid=1002"));
    }
}
