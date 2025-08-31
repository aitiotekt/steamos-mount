//! Mount operations module.
//!
//! This module handles mounting and unmounting devices, detecting dirty NTFS
//! volumes, and running ntfsfix for repair.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::disk::BlockDevice;
use crate::error::{Error, IoResultExt, Result};

/// Creates a mount point directory if it doesn't exist.
pub fn create_mount_point(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).mount_point_context(path)?;
    }
    Ok(())
}

/// Mounts a device to the specified mount point.
///
/// Uses the `mount` command with the device path.
pub fn mount_device(device: &BlockDevice, mount_point: &Path) -> Result<()> {
    // Ensure mount point exists
    create_mount_point(mount_point)?;

    let output = Command::new("mount")
        .arg(&device.path)
        .arg(mount_point)
        .output()
        .command_context("mount")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Check if this is a dirty volume issue
        if is_dirty_volume_error(&stderr) {
            return Err(Error::DirtyVolume {
                device: device.path.display().to_string(),
            });
        }

        return Err(Error::Mount { message: stderr });
    }

    Ok(())
}

/// Unmounts a device from the specified mount point.
pub fn unmount_device(mount_point: &Path) -> Result<()> {
    let output = Command::new("umount")
        .arg(mount_point)
        .output()
        .command_context("umount")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Unmount {
            path: mount_point.to_path_buf(),
            message: stderr,
        });
    }

    Ok(())
}

/// Checks if an error message indicates a dirty NTFS volume.
fn is_dirty_volume_error(stderr: &str) -> bool {
    let dirty_indicators = [
        "volume is dirty",
        "Volume is dirty",
        "force flag is not set",
        "The disk contains an unclean file system",
    ];

    dirty_indicators
        .iter()
        .any(|indicator| stderr.contains(indicator))
}

/// Detects if a device has a dirty NTFS volume by checking dmesg.
pub fn detect_dirty_volume(device: &BlockDevice) -> Result<bool> {
    // Only NTFS can have dirty volumes
    if !device.is_ntfs() {
        return Ok(false);
    }

    let output = Command::new("dmesg").output().command_context("dmesg")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let device_name = &device.name;

    // Look for dirty volume messages related to this device
    let is_dirty = stdout
        .lines()
        .any(|line| line.contains(device_name) && is_dirty_volume_error(line));

    Ok(is_dirty)
}

/// Attempts to repair a dirty NTFS volume using ntfsfix.
///
/// Runs `ntfsfix -d <device>` to clear the dirty flag.
pub fn repair_dirty_volume(device: &BlockDevice) -> Result<()> {
    if !device.is_ntfs() {
        return Err(Error::Ntfsfix {
            device: device.path.display().to_string(),
            message: "ntfsfix only works on NTFS volumes".to_string(),
        });
    }

    let output = Command::new("ntfsfix")
        .arg("-d") // Clear the dirty flag
        .arg(&device.path)
        .output()
        .command_context("ntfsfix")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Ntfsfix {
            device: device.path.display().to_string(),
            message: stderr,
        });
    }

    Ok(())
}

/// Reloads systemd daemon to pick up fstab changes.
pub fn reload_systemd_daemon() -> Result<()> {
    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .command_context("systemctl daemon-reload")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Systemd { message: stderr });
    }

    Ok(())
}

/// Starts a systemd mount unit for a mount point.
///
/// The unit name is derived from the mount point path.
pub fn start_mount_unit(mount_point: &Path) -> Result<()> {
    let unit_name = mount_point_to_unit_name(mount_point);

    let output = Command::new("systemctl")
        .args(["start", &unit_name])
        .output()
        .command_context(format!("systemctl start {}", unit_name))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Systemd { message: stderr });
    }

    Ok(())
}

/// Stops a systemd mount unit for a mount point.
pub fn stop_mount_unit(mount_point: &Path) -> Result<()> {
    let unit_name = mount_point_to_unit_name(mount_point);

    let output = Command::new("systemctl")
        .args(["stop", &unit_name])
        .output()
        .command_context(format!("systemctl stop {}", unit_name))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Systemd { message: stderr });
    }

    Ok(())
}

/// Converts a mount point path to a systemd unit name.
///
/// Example: "/home/deck/Drives/GamesSSD" -> "home-deck-Drives-GamesSSD.mount"
fn mount_point_to_unit_name(mount_point: &Path) -> String {
    let path_str = mount_point.to_string_lossy();
    let escaped = path_str.trim_start_matches('/').replace('/', "-");

    format!("{}.mount", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_point_to_unit_name() {
        assert_eq!(
            mount_point_to_unit_name(Path::new("/home/deck/Drives/GamesSSD")),
            "home-deck-Drives-GamesSSD.mount"
        );
        assert_eq!(
            mount_point_to_unit_name(Path::new("/mnt/test")),
            "mnt-test.mount"
        );
    }

    #[test]
    fn test_is_dirty_volume_error() {
        assert!(is_dirty_volume_error("volume is dirty"));
        assert!(is_dirty_volume_error("Volume is dirty"));
        assert!(is_dirty_volume_error("ntfs3: force flag is not set"));
        assert!(is_dirty_volume_error(
            "The disk contains an unclean file system"
        ));
        assert!(!is_dirty_volume_error("mount successful"));
    }
}
