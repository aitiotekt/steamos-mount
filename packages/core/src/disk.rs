//! Disk scanning module using lsblk.
//!
//! This module provides functionality to scan and list block devices
//! on the system, filtering for NTFS and exFAT partitions that can be
//! mounted by this tool.

use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Represents a block device (partition) on the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDevice {
    /// Device name (e.g., "nvme0n1p2", "sda1").
    pub name: String,
    /// Volume label, if set.
    pub label: Option<String>,
    /// Filesystem UUID (case-sensitive, as returned by blkid).
    pub uuid: Option<String>,
    /// Partition UUID (case-sensitive, as returned by blkid).
    pub partuuid: Option<String>,
    /// Filesystem type (e.g., "ntfs", "exfat").
    pub fstype: Option<String>,
    /// Current mount point, if mounted.
    pub mountpoint: Option<String>,
    /// Size in bytes.
    pub size: u64,
    /// Full device path (e.g., "/dev/nvme0n1p2").
    pub path: PathBuf,
}

impl BlockDevice {
    /// Returns the device identifier path for fstab.
    ///
    /// Uses UUID by default if available, otherwise PARTUUID.
    /// UUID and PARTUUID are case-sensitive and must match the values in
    /// `/dev/disk/by-uuid/` and `/dev/disk/by-partuuid/` respectively.
    pub fn fstab_spec(&self) -> Option<String> {
        self.uuid
            .as_ref()
            .map(|uuid| format!("UUID={}", uuid))
            .or_else(|| {
                self.partuuid
                    .as_ref()
                    .map(|partuuid| format!("PARTUUID={}", partuuid))
            })
    }

    /// Validates that the device identifier path exists in the filesystem.
    ///
    /// Checks if `/dev/disk/by-uuid/<UUID>` or `/dev/disk/by-partuuid/<PARTUUID>`
    /// exists, as required for fstab mounting.
    pub fn validate_fstab_spec(&self) -> Result<()> {
        use std::path::Path;

        if let Some(uuid) = &self.uuid {
            let uuid_path = Path::new("/dev/disk/by-uuid").join(uuid);
            if !uuid_path.exists() {
                return Err(Error::InvalidUuid {
                    uuid: format!(
                        "UUID {} does not exist at {}",
                        uuid,
                        uuid_path.display()
                    ),
                });
            }
        } else if let Some(partuuid) = &self.partuuid {
            let partuuid_path = Path::new("/dev/disk/by-partuuid").join(partuuid);
            if !partuuid_path.exists() {
                return Err(Error::InvalidUuid {
                    uuid: format!(
                        "PARTUUID {} does not exist at {}",
                        partuuid,
                        partuuid_path.display()
                    ),
                });
            }
        } else {
            return Err(Error::InvalidUuid {
                uuid: "Device has no UUID or PARTUUID".to_string(),
            });
        }

        Ok(())
    }

    /// Returns a suggested mount point name based on label or UUID.
    ///
    /// If label is empty or None, uses the first 8 characters of UUID.
    pub fn suggested_mount_name(&self) -> String {
        if let Some(label) = self.label.as_ref().filter(|l| !l.is_empty()) {
            return sanitize_mount_name(label);
        }
        if let Some(uuid) = &self.uuid {
            // Use first 8 characters of UUID for uniqueness
            return uuid.chars().take(8).collect();
        }
        // Fallback to device name
        self.name.clone()
    }

    /// Returns true if this device is an NTFS partition.
    pub fn is_ntfs(&self) -> bool {
        self.fstype.as_deref() == Some("ntfs")
    }

    /// Returns true if this device is an exFAT partition.
    pub fn is_exfat(&self) -> bool {
        self.fstype.as_deref() == Some("exfat")
    }

    /// Returns true if this device can be mounted by this tool.
    pub fn is_mountable(&self) -> bool {
        self.is_ntfs() || self.is_exfat()
    }

    /// Returns true if this device is currently mounted.
    pub fn is_mounted(&self) -> bool {
        self.mountpoint.is_some()
    }
}

/// Sanitize a string for use as a mount point directory name.
///
/// Replaces problematic characters with underscores.
fn sanitize_mount_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Raw JSON structure from lsblk output.
#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    name: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    partuuid: Option<String>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(rename = "type")]
    device_type: Option<String>,
    #[serde(default)]
    children: Option<Vec<LsblkDevice>>,
}

/// Lists all block devices on the system.
///
/// Calls `lsblk --json --bytes` and parses the output.
pub fn list_block_devices() -> Result<Vec<BlockDevice>> {
    use crate::error::IoResultExt;

    let output = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--output",
            "NAME,LABEL,UUID,PARTUUID,FSTYPE,MOUNTPOINT,SIZE,TYPE",
        ])
        .output()
        .command_context("lsblk")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::CommandExit {
            command: "lsblk".to_string(),
            code: output.status.code().unwrap_or(-1),
            stderr,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lsblk_output: LsblkOutput =
        serde_json::from_str(&stdout).map_err(|e| Error::LsblkParse {
            message: e.to_string(),
        })?;

    let mut devices = Vec::new();
    collect_devices(&lsblk_output.blockdevices, &mut devices);

    Ok(devices)
}

/// Recursively collect devices from lsblk output, including children (partitions).
fn collect_devices(lsblk_devices: &[LsblkDevice], devices: &mut Vec<BlockDevice>) {
    for dev in lsblk_devices {
        // Only include partitions (type = "part")
        if dev.device_type.as_deref() == Some("part") {
            devices.push(BlockDevice {
                name: dev.name.clone(),
                label: dev.label.clone(),
                uuid: dev.uuid.clone(),
                partuuid: dev.partuuid.clone(),
                fstype: dev.fstype.clone(),
                mountpoint: dev.mountpoint.clone(),
                size: dev.size.unwrap_or(0),
                path: PathBuf::from(format!("/dev/{}", dev.name)),
            });
        }

        // Recurse into children (partitions of a disk)
        if let Some(children) = &dev.children {
            collect_devices(children, devices);
        }
    }
}

/// Filters block devices to only include NTFS and exFAT partitions.
pub fn filter_mountable_devices(devices: &[BlockDevice]) -> Vec<&BlockDevice> {
    devices.iter().filter(|d| d.is_mountable()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LSBLK_JSON: &str = r#"{
        "blockdevices": [
            {
                "name": "nvme0n1",
                "label": null,
                "uuid": null,
                "partuuid": null,
                "fstype": null,
                "mountpoint": null,
                "size": 500107862016,
                "type": "disk",
                "children": [
                    {
                        "name": "nvme0n1p1",
                        "label": "EFI",
                        "uuid": "1234-5678",
                        "partuuid": "abcd-efgh",
                        "fstype": "vfat",
                        "mountpoint": "/boot/efi",
                        "size": 536870912,
                        "type": "part"
                    },
                    {
                        "name": "nvme0n1p2",
                        "label": "Games",
                        "uuid": "AABBCCDD11223344",
                        "partuuid": "1122-3344",
                        "fstype": "ntfs",
                        "mountpoint": null,
                        "size": 499570991104,
                        "type": "part"
                    }
                ]
            },
            {
                "name": "sda",
                "label": null,
                "uuid": null,
                "partuuid": null,
                "fstype": null,
                "mountpoint": null,
                "size": 128849018880,
                "type": "disk",
                "children": [
                    {
                        "name": "sda1",
                        "label": "PORTABLE",
                        "uuid": "DEAD-BEEF",
                        "partuuid": "5566-7788",
                        "fstype": "exfat",
                        "mountpoint": null,
                        "size": 128849018880,
                        "type": "part"
                    }
                ]
            }
        ]
    }"#;

    #[test]
    fn test_parse_lsblk_json() {
        let lsblk_output: LsblkOutput = serde_json::from_str(SAMPLE_LSBLK_JSON).unwrap();
        let mut devices = Vec::new();
        collect_devices(&lsblk_output.blockdevices, &mut devices);

        assert_eq!(devices.len(), 3);

        // Check NTFS partition
        let ntfs_device = devices.iter().find(|d| d.name == "nvme0n1p2").unwrap();
        assert_eq!(ntfs_device.label, Some("Games".to_string()));
        assert_eq!(ntfs_device.uuid, Some("AABBCCDD11223344".to_string())); // case-sensitive, original case
        assert_eq!(ntfs_device.fstype, Some("ntfs".to_string()));
        assert!(ntfs_device.is_ntfs());
        assert!(ntfs_device.is_mountable());

        // Check exFAT partition
        let exfat_device = devices.iter().find(|d| d.name == "sda1").unwrap();
        assert_eq!(exfat_device.label, Some("PORTABLE".to_string()));
        assert!(exfat_device.is_exfat());
        assert!(exfat_device.is_mountable());

        // Check EFI partition (not mountable)
        let efi_device = devices.iter().find(|d| d.name == "nvme0n1p1").unwrap();
        assert!(!efi_device.is_mountable());
    }

    #[test]
    fn test_filter_mountable_devices() {
        let lsblk_output: LsblkOutput = serde_json::from_str(SAMPLE_LSBLK_JSON).unwrap();
        let mut devices = Vec::new();
        collect_devices(&lsblk_output.blockdevices, &mut devices);

        let mountable = filter_mountable_devices(&devices);
        assert_eq!(mountable.len(), 2);
        assert!(mountable.iter().all(|d| d.is_mountable()));
    }

    #[test]
    fn test_fstab_spec() {
        let device = BlockDevice {
            name: "sda1".to_string(),
            label: Some("Test".to_string()),
            uuid: Some("AABB-CCDD".to_string()),
            partuuid: Some("1122-3344".to_string()),
            fstype: Some("ntfs".to_string()),
            mountpoint: None,
            size: 1024,
            path: PathBuf::from("/dev/sda1"),
        };

        // UUID takes precedence, case-sensitive
        assert_eq!(device.fstab_spec(), Some("UUID=AABB-CCDD".to_string()));
    }

    #[test]
    fn test_suggested_mount_name() {
        // With label
        let device_with_label = BlockDevice {
            name: "sda1".to_string(),
            label: Some("My Games".to_string()),
            uuid: Some("1234-5678".to_string()),
            partuuid: None,
            fstype: Some("ntfs".to_string()),
            mountpoint: None,
            size: 1024,
            path: PathBuf::from("/dev/sda1"),
        };
        assert_eq!(device_with_label.suggested_mount_name(), "My_Games");

        // Without label, uses UUID
        let device_no_label = BlockDevice {
            name: "sda1".to_string(),
            label: None,
            uuid: Some("12345678-abcd-efgh".to_string()),
            partuuid: None,
            fstype: Some("ntfs".to_string()),
            mountpoint: None,
            size: 1024,
            path: PathBuf::from("/dev/sda1"),
        };
        assert_eq!(device_no_label.suggested_mount_name(), "12345678");
    }

    #[test]
    fn test_sanitize_mount_name() {
        assert_eq!(sanitize_mount_name("My Games"), "My_Games");
        assert_eq!(sanitize_mount_name("Test-Drive_123"), "Test-Drive_123");
        assert_eq!(sanitize_mount_name("Game/Data"), "Game_Data");
    }
}
