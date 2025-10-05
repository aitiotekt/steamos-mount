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
    /// Whether the device is rotational (HDD) or not (SSD).
    pub rota: bool,
    /// Whether the device is removable.
    pub removable: bool,
    /// Transport type (e.g., "usb", "nvme", "sata", "mmc").
    pub transport: Option<String>,
}

impl BlockDevice {
    // ... fstab_spec and other methods remain same
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
                    uuid: format!("UUID {} does not exist at {}", uuid, uuid_path.display()),
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
    rota: Option<bool>,
    #[serde(default)]
    rm: Option<bool>,
    #[serde(default)]
    tran: Option<String>,
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
            "NAME,LABEL,UUID,PARTUUID,FSTYPE,MOUNTPOINT,SIZE,TYPE,ROTA,RM,TRAN",
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
    collect_devices(&lsblk_output.blockdevices, &mut devices, None);

    Ok(devices)
}

/// Recursively collect devices from lsblk output, including children (partitions).
///
/// We propagate parent properties (ROTA, RM, TRAN) to children if they are missing
/// in the child (lsblk usually sets them for children too, but good to be safe/consistent).
fn collect_devices(
    lsblk_devices: &[LsblkDevice],
    devices: &mut Vec<BlockDevice>,
    parent: Option<&LsblkDevice>,
) {
    for dev in lsblk_devices {
        // Inherit properties from parent if not present (though lsblk usually provides them)
        // or prioritize device's own properties if present.
        let rota = dev
            .rota
            .or_else(|| parent.and_then(|p| p.rota))
            .unwrap_or(false);
        let removable = dev
            .rm
            .or_else(|| parent.and_then(|p| p.rm))
            .unwrap_or(false);
        let transport = dev
            .tran
            .clone()
            .or_else(|| parent.and_then(|p| p.tran.clone()));

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
                rota,
                removable,
                transport: transport.clone(),
            });
        }

        // Recurse into children (partitions of a disk)
        if let Some(children) = &dev.children {
            collect_devices(children, devices, Some(dev));
        }
    }
}

/// Filters block devices to only include NTFS and exFAT partitions.
pub fn filter_mountable_devices(devices: &[BlockDevice]) -> Vec<&BlockDevice> {
    devices.iter().filter(|d| d.is_mountable()).collect()
}

/// Represents an offline managed device from fstab that is not currently online.
///
/// This struct contains information extracted from an fstab entry for a device
/// that is configured but not currently connected/visible to the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineDevice {
    /// The device identifier from fstab (e.g., "UUID=xxx", "PARTUUID=xxx", "LABEL=xxx").
    pub fs_spec: String,
    /// Mount point path.
    pub mount_point: std::path::PathBuf,
    /// Filesystem type (e.g., "ntfs3", "exfat").
    pub vfs_type: String,
    /// Mount options.
    pub mount_options: Vec<String>,
    /// Extracted UUID if fs_spec starts with "UUID=".
    pub uuid: Option<String>,
    /// Extracted PARTUUID if fs_spec starts with "PARTUUID=".
    pub partuuid: Option<String>,
    /// Extracted LABEL if fs_spec starts with "LABEL=".
    pub label: Option<String>,
}

impl OfflineDevice {
    /// Creates an OfflineDevice from an FstabEntry.
    pub fn from_fstab_entry(entry: &crate::fstab::FstabEntry) -> Self {
        let (uuid, partuuid, label) = parse_fs_spec(&entry.fs_spec);

        Self {
            fs_spec: entry.fs_spec.clone(),
            mount_point: entry.mount_point.clone(),
            vfs_type: entry.vfs_type.clone(),
            mount_options: entry.mount_options.clone(),
            uuid,
            partuuid,
            label,
        }
    }

    /// Returns the raw fstab line for this device.
    pub fn to_fstab_line(&self) -> String {
        format!(
            "{}  {}  {}  {}  0  0",
            self.fs_spec,
            self.mount_point.display(),
            self.vfs_type,
            self.mount_options.join(",")
        )
    }
}

fn parse_fs_spec(fs_spec: &str) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(uuid) = fs_spec.strip_prefix("UUID=") {
        (Some(uuid.to_string()), None, None)
    } else if let Some(partuuid) = fs_spec.strip_prefix("PARTUUID=") {
        (None, Some(partuuid.to_string()), None)
    } else if let Some(label) = fs_spec.strip_prefix("LABEL=") {
        (None, None, Some(label.to_string()))
    } else {
        (None, None, None)
    }
}

/// Normalizes filesystem type for comparison purposes.
///
/// lsblk reports filesystem types differently from fstab vfs_type:
/// - lsblk: "ntfs" vs fstab: "ntfs3"  
/// - lsblk: "exfat" vs fstab: "exfat" (same)
///
/// This function normalizes both to a common form for comparison.
pub fn normalize_fstype(fstype: &str) -> &str {
    match fstype {
        // lsblk -> normalized
        "ntfs" => "ntfs",
        // fstab vfs_type -> normalized
        "ntfs3" => "ntfs",
        // Others remain unchanged
        _ => fstype,
    }
}

/// Converts a normalized filesystem type to the fstab vfs_type.
///
/// This is the reverse of normalization - converting lsblk fstype to fstab vfs_type.
pub fn fstype_to_vfs_type(fstype: &str) -> &str {
    match fstype {
        // ntfs from lsblk should use ntfs3 driver in fstab
        "ntfs" => "ntfs3",
        // Others remain unchanged
        _ => fstype,
    }
}

/// Converts an fstab vfs_type to the display fstype (as lsblk would report).
pub fn vfs_type_to_fstype(vfs_type: &str) -> &str {
    match vfs_type {
        // ntfs3 driver displays as ntfs
        "ntfs3" => "ntfs",
        // Others remain unchanged
        _ => vfs_type,
    }
}

/// Represents a device that may be online (connected) or offline (in fstab but not connected).
#[derive(Debug, Clone)]
pub enum ManagedDevice {
    /// Device is online and visible to the system.
    Online(BlockDevice),
    /// Device is configured in fstab but currently offline.
    Offline(OfflineDevice),
}

impl ManagedDevice {
    /// Returns true if this is an online device.
    pub fn is_online(&self) -> bool {
        matches!(self, ManagedDevice::Online(_))
    }

    /// Returns true if this is an offline device.
    pub fn is_offline(&self) -> bool {
        matches!(self, ManagedDevice::Offline(_))
    }

    /// Returns the UUID if available.
    pub fn uuid(&self) -> Option<&str> {
        match self {
            ManagedDevice::Online(d) => d.uuid.as_deref(),
            ManagedDevice::Offline(d) => d.uuid.as_deref(),
        }
    }

    /// Returns the PARTUUID if available.
    pub fn partuuid(&self) -> Option<&str> {
        match self {
            ManagedDevice::Online(d) => d.partuuid.as_deref(),
            ManagedDevice::Offline(d) => d.partuuid.as_deref(),
        }
    }

    /// Returns the label if available.
    pub fn label(&self) -> Option<&str> {
        match self {
            ManagedDevice::Online(d) => d.label.as_deref(),
            ManagedDevice::Offline(d) => d.label.as_deref(),
        }
    }
}

/// Checks if a block device matches an fstab entry.
fn device_matches_fstab_entry(device: &BlockDevice, entry: &crate::fstab::FstabEntry) -> bool {
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

/// Result of listing managed devices, including both devices and fstab entries.
///
/// This struct is returned by `list_managed_devices` to provide access to both
/// the merged device list and the original fstab entries, avoiding the need for
/// callers to re-parse fstab.
#[derive(Debug, Clone)]
pub struct ManagedDevicesResult {
    /// List of managed devices (online + offline).
    pub devices: Vec<ManagedDevice>,
    /// Managed fstab entries (for matching against online devices).
    pub fstab_entries: Vec<crate::fstab::FstabEntry>,
}

/// Lists all managed devices, including both online devices and offline fstab entries.
///
/// This function merges online devices from lsblk with offline managed entries from fstab.
/// Online devices take precedence - if a device is both online and in fstab, only the
/// online version is returned.
///
/// # Arguments
/// * `online_devices` - List of online block devices (from `list_block_devices()`)
/// * `fstab_path` - Path to the fstab file
///
/// # Returns
/// A `ManagedDevicesResult` containing:
/// - `devices`: Merged list with online devices first, followed by offline fstab entries
/// - `fstab_entries`: The parsed fstab entries for callers to use without re-parsing
pub fn list_managed_devices(
    online_devices: &[BlockDevice],
    fstab_path: &std::path::Path,
) -> Result<ManagedDevicesResult> {
    // Parse fstab to get managed entries
    let fstab_entries = crate::fstab::parse_fstab(fstab_path)
        .map(|parsed| parsed.managed_entries)
        .unwrap_or_default();

    let mut devices: Vec<ManagedDevice> = Vec::new();

    // First, add all mountable online devices
    for device in filter_mountable_devices(online_devices) {
        devices.push(ManagedDevice::Online(device.clone()));
    }

    // Then, add offline fstab entries that don't match any online device
    for entry in &fstab_entries {
        let is_online = online_devices
            .iter()
            .any(|d| device_matches_fstab_entry(d, entry));

        if !is_online {
            devices.push(ManagedDevice::Offline(OfflineDevice::from_fstab_entry(
                entry,
            )));
        }
    }

    Ok(ManagedDevicesResult {
        devices,
        fstab_entries,
    })
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
                "rota": false,
                "rm": false,
                "tran": "nvme",
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
                "rota": true,
                "rm": true,
                "tran": "usb",
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
        collect_devices(&lsblk_output.blockdevices, &mut devices, None);

        assert_eq!(devices.len(), 3);

        // Check NTFS partition (NVMe SSD)
        let ntfs_device = devices.iter().find(|d| d.name == "nvme0n1p2").unwrap();
        assert_eq!(ntfs_device.label, Some("Games".to_string()));
        assert_eq!(ntfs_device.uuid, Some("AABBCCDD11223344".to_string()));
        assert_eq!(ntfs_device.fstype, Some("ntfs".to_string()));
        assert!(ntfs_device.is_ntfs());
        assert!(!ntfs_device.rota); // from parent
        assert!(!ntfs_device.removable); // from parent
        assert_eq!(ntfs_device.transport.as_deref(), Some("nvme"));

        // Check exFAT partition (USB HDD)
        let exfat_device = devices.iter().find(|d| d.name == "sda1").unwrap();
        assert_eq!(exfat_device.label, Some("PORTABLE".to_string()));
        assert!(exfat_device.is_exfat());
        assert!(exfat_device.rota); // from parent
        assert!(exfat_device.removable); // from parent
        assert_eq!(exfat_device.transport.as_deref(), Some("usb"));
    }

    #[test]
    fn test_filter_mountable_devices() {
        let lsblk_output: LsblkOutput = serde_json::from_str(SAMPLE_LSBLK_JSON).unwrap();
        let mut devices = Vec::new();
        collect_devices(&lsblk_output.blockdevices, &mut devices, None);

        let mountable = filter_mountable_devices(&devices);
        assert_eq!(mountable.len(), 2);
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
            rota: false,
            removable: false,
            transport: None,
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
            rota: false,
            removable: false,
            transport: None,
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
            rota: false,
            removable: false,
            transport: None,
        };
        assert_eq!(device_no_label.suggested_mount_name(), "12345678");
    }

    #[test]
    fn test_sanitize_mount_name() {
        assert_eq!(sanitize_mount_name("My Games"), "My_Games");
        assert_eq!(sanitize_mount_name("Test-Drive_123"), "Test-Drive_123");
        assert_eq!(sanitize_mount_name("Game/Data"), "Game_Data");
    }

    #[test]
    fn test_parse_fs_spec() {
        // UUID
        let (uuid, partuuid, label) = parse_fs_spec("UUID=AABB-CCDD");
        assert_eq!(uuid, Some("AABB-CCDD".to_string()));
        assert_eq!(partuuid, None);
        assert_eq!(label, None);

        // PARTUUID
        let (uuid, partuuid, label) = parse_fs_spec("PARTUUID=1122-3344");
        assert_eq!(uuid, None);
        assert_eq!(partuuid, Some("1122-3344".to_string()));
        assert_eq!(label, None);

        // LABEL
        let (uuid, partuuid, label) = parse_fs_spec("LABEL=MyDrive");
        assert_eq!(uuid, None);
        assert_eq!(partuuid, None);
        assert_eq!(label, Some("MyDrive".to_string()));

        // Path (no prefix)
        let (uuid, partuuid, label) = parse_fs_spec("/dev/sda1");
        assert_eq!(uuid, None);
        assert_eq!(partuuid, None);
        assert_eq!(label, None);
    }

    #[test]
    fn test_offline_device_from_fstab_entry() {
        let entry = crate::fstab::FstabEntry::new(
            "UUID=1234-5678",
            "/home/deck/Drives/TestDrive",
            "ntfs3",
            "rw,uid=1000,gid=1000",
            0,
            0,
        );

        let offline = OfflineDevice::from_fstab_entry(&entry);
        assert_eq!(offline.fs_spec, "UUID=1234-5678");
        assert_eq!(
            offline.mount_point,
            PathBuf::from("/home/deck/Drives/TestDrive")
        );
        assert_eq!(offline.vfs_type, "ntfs3");
        assert_eq!(offline.uuid, Some("1234-5678".to_string()));
        assert_eq!(offline.partuuid, None);
        assert_eq!(offline.label, None);
    }

    #[test]
    fn test_managed_device_methods() {
        let online_device = BlockDevice {
            name: "sda1".to_string(),
            label: Some("Games".to_string()),
            uuid: Some("1234-5678".to_string()),
            partuuid: Some("abcd-efgh".to_string()),
            fstype: Some("ntfs".to_string()),
            mountpoint: None,
            size: 1024,
            path: PathBuf::from("/dev/sda1"),
            rota: false,
            removable: false,
            transport: None,
        };

        let managed_online = ManagedDevice::Online(online_device);
        assert!(managed_online.is_online());
        assert!(!managed_online.is_offline());
        assert_eq!(managed_online.uuid(), Some("1234-5678"));
        assert_eq!(managed_online.label(), Some("Games"));

        let offline_device = OfflineDevice {
            fs_spec: "UUID=dead-beef".to_string(),
            mount_point: PathBuf::from("/mnt/test"),
            vfs_type: "exfat".to_string(),
            mount_options: vec!["rw".to_string()],
            uuid: Some("dead-beef".to_string()),
            partuuid: None,
            label: None,
        };

        let managed_offline = ManagedDevice::Offline(offline_device);
        assert!(!managed_offline.is_online());
        assert!(managed_offline.is_offline());
        assert_eq!(managed_offline.uuid(), Some("dead-beef"));
    }

    #[test]
    fn test_device_matches_fstab_entry() {
        let device = BlockDevice {
            name: "sda1".to_string(),
            label: Some("Games".to_string()),
            uuid: Some("1234-5678".to_string()),
            partuuid: Some("abcd-efgh".to_string()),
            fstype: Some("ntfs".to_string()),
            mountpoint: None,
            size: 1024,
            path: PathBuf::from("/dev/sda1"),
            rota: false,
            removable: false,
            transport: None,
        };

        // Match by UUID
        let entry_uuid =
            crate::fstab::FstabEntry::new("UUID=1234-5678", "/mnt/test", "ntfs3", "defaults", 0, 0);
        assert!(device_matches_fstab_entry(&device, &entry_uuid));

        // Match by PARTUUID
        let entry_partuuid = crate::fstab::FstabEntry::new(
            "PARTUUID=abcd-efgh",
            "/mnt/test",
            "ntfs3",
            "defaults",
            0,
            0,
        );
        assert!(device_matches_fstab_entry(&device, &entry_partuuid));

        // Match by LABEL
        let entry_label =
            crate::fstab::FstabEntry::new("LABEL=Games", "/mnt/test", "ntfs3", "defaults", 0, 0);
        assert!(device_matches_fstab_entry(&device, &entry_label));

        // Non-matching
        let entry_no_match =
            crate::fstab::FstabEntry::new("UUID=different", "/mnt/test", "ntfs3", "defaults", 0, 0);
        assert!(!device_matches_fstab_entry(&device, &entry_no_match));
    }

    #[test]
    fn test_list_managed_devices_merge() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary fstab with some managed entries
        let fstab_content = r#"# /etc/fstab
UUID=system-root  /  ext4  defaults  0  1

# BEGIN STEAMOS-MOUNT-MANAGED
# Created by SteamOS Mount Tool. DO NOT EDIT THIS BLOCK MANUALLY.
UUID=AABBCCDD11223344  /home/deck/Drives/Games  ntfs3  rw,uid=1000  0  0
UUID=OFFLINE-DEVICE  /home/deck/Drives/Offline  exfat  rw  0  0
# END STEAMOS-MOUNT-MANAGED
"#;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(fstab_content.as_bytes()).unwrap();

        // Create online devices (one matches fstab, one doesn't)
        let online_devices = vec![
            BlockDevice {
                name: "nvme0n1p2".to_string(),
                label: Some("Games".to_string()),
                uuid: Some("AABBCCDD11223344".to_string()),
                partuuid: Some("1122-3344".to_string()),
                fstype: Some("ntfs".to_string()),
                mountpoint: Some("/home/deck/Drives/Games".to_string()),
                size: 499570991104,
                path: PathBuf::from("/dev/nvme0n1p2"),
                rota: false,
                removable: false,
                transport: Some("nvme".to_string()),
            },
            BlockDevice {
                name: "sda1".to_string(),
                label: Some("New".to_string()),
                uuid: Some("NEW-DEVICE".to_string()),
                partuuid: None,
                fstype: Some("exfat".to_string()),
                mountpoint: None,
                size: 128849018880,
                path: PathBuf::from("/dev/sda1"),
                rota: false,
                removable: true,
                transport: Some("usb".to_string()),
            },
        ];

        let result = list_managed_devices(&online_devices, temp_file.path()).unwrap();

        // Should have 3 devices: 2 online (both mountable) + 1 offline
        assert_eq!(result.devices.len(), 3);

        // First two should be online
        assert!(result.devices[0].is_online());
        assert!(result.devices[1].is_online());

        // Third should be offline (the OFFLINE-DEVICE from fstab)
        assert!(result.devices[2].is_offline());
        assert_eq!(result.devices[2].uuid(), Some("OFFLINE-DEVICE"));
    }
}
