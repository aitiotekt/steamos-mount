//! Mount operations module.
//!
//! This module handles mounting and unmounting devices, detecting dirty NTFS
//! volumes, and running ntfsfix for repair.

use std::fs;
use std::path::Path;

use crate::disk::BlockDevice;
use crate::error::{Error, IoResultExt, Result};
use crate::executor::ExecutionContext;

/// Creates a mount point directory if it doesn't exist.
///
/// When using an `ExecutionContext` with privilege escalation,
/// use `create_mount_point_with_ctx` instead.
pub fn create_mount_point(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path).mount_point_context(path)?;
    }
    Ok(())
}

/// Creates a mount point directory with privilege escalation if needed.
pub fn create_mount_point_with_ctx(path: &Path, ctx: &mut ExecutionContext) -> Result<()> {
    // Default behavior is to try privileged creation if ctx allows,
    // but here we redirect to smart with try_unprivileged=false to keep existing behavior
    // where caller likely expects ctx to be used actively.
    // However, if ctx is None/Default, smart(false) just tries mkdir.
    // Actually, create_mount_point_smart(false) will skip the unprivileged check and go straight to ctx.mkdir_privileged.
    create_mount_point_smart(path, ctx, false)
}

/// Creates a mount point directory with smart privilege handling.
///
/// If `try_unprivileged` is true, it attempts to create the directory with current user privileges first.
/// If that fails with PermissionDenied, it returns `Error::MountPointPermissionDenied`.
/// Otherwise (or if `try_unprivileged` is false), it uses the execution context (potentially privileged).
pub fn create_mount_point_smart(
    path: &Path,
    ctx: &mut ExecutionContext,
    try_unprivileged: bool,
) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    if try_unprivileged {
        // Enforce using privilege for paths outside home directory
        if let Some(home) = dirs::home_dir()
            && !path.starts_with(home)
        {
            return Err(Error::MountPointPermissionDenied {
                path: path.to_path_buf(),
            });
        }

        match fs::create_dir_all(path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Err(Error::MountPointPermissionDenied {
                        path: path.to_path_buf(),
                    });
                }
                return Err(Error::MountPointCreation {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        }
    }

    ctx.mkdir_privileged(&path.display().to_string())
}

/// Mounts a device to the specified mount point.
///
/// Uses the `mount` command with the device path.
/// This version runs without privilege escalation.
pub fn mount_device(device: &BlockDevice, mount_point: &Path) -> Result<()> {
    mount_device_with_ctx(device, mount_point, &mut ExecutionContext::default())
}

/// Mounts a device with privilege escalation support.
pub fn mount_device_with_ctx(
    device: &BlockDevice,
    mount_point: &Path,
    ctx: &mut ExecutionContext,
) -> Result<()> {
    // Ensure mount point exists
    create_mount_point_with_ctx(mount_point, ctx)?;

    let device_path = device.path.display().to_string();
    let mount_point_str = mount_point.display().to_string();

    let output = ctx.run_privileged("mount", &[&device_path, &mount_point_str])?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Check if this is a dirty volume issue
        if is_dirty_volume_error(&stderr) {
            return Err(Error::DirtyVolume {
                device: device.path.display().to_string(),
            });
        }

        // Check for authentication cancellation
        if output.status.code() == Some(126) {
            return Err(Error::AuthenticationCancelled);
        }

        return Err(Error::Mount { message: stderr });
    }

    Ok(())
}

/// Unmounts a device from the specified mount point.
pub fn unmount_device(mount_point: &Path) -> Result<()> {
    unmount_device_with_ctx(mount_point, &mut ExecutionContext::default())
}

/// Unmounts a device with privilege escalation support.
pub fn unmount_device_with_ctx(mount_point: &Path, ctx: &mut ExecutionContext) -> Result<()> {
    let mount_point_str = mount_point.display().to_string();
    let output = ctx.run_privileged("umount", &[&mount_point_str])?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.code() == Some(126) {
            return Err(Error::AuthenticationCancelled);
        }

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
///
/// Note: On some systems, `dmesg` requires root privileges
/// (kernel.dmesg_restrict=1). Use `detect_dirty_volume_with_ctx` if needed.
pub fn detect_dirty_volume(device: &BlockDevice) -> Result<bool> {
    detect_dirty_volume_with_ctx(device, &mut ExecutionContext::default())
}

/// Detects a dirty NTFS volume with privilege escalation support.
///
/// Uses dmesg to check for dirty volume messages. On systems with
/// `kernel.dmesg_restrict=1`, this requires elevated privileges.
pub fn detect_dirty_volume_with_ctx(
    device: &BlockDevice,
    ctx: &mut ExecutionContext,
) -> Result<bool> {
    // Only NTFS can have dirty volumes
    if !device.is_ntfs() {
        return Ok(false);
    }

    let output = ctx.run_privileged("dmesg", &[])?;

    if !output.status.success() {
        // If dmesg fails (e.g., permission denied), we can't detect dirty state
        // Return false rather than erroring - the mount will fail later if dirty
        return Ok(false);
    }

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
    repair_dirty_volume_with_ctx(device, &mut ExecutionContext::default())
}

/// Repairs a dirty NTFS volume with privilege escalation support.
pub fn repair_dirty_volume_with_ctx(
    device: &BlockDevice,
    ctx: &mut ExecutionContext,
) -> Result<()> {
    if !device.is_ntfs() {
        return Err(Error::Ntfsfix {
            device: device.path.display().to_string(),
            message: "ntfsfix only works on NTFS volumes".to_string(),
        });
    }

    let device_path = device.path.display().to_string();
    let output = ctx.run_privileged("ntfsfix", &["-d", &device_path])?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.code() == Some(126) {
            return Err(Error::AuthenticationCancelled);
        }

        return Err(Error::Ntfsfix {
            device: device.path.display().to_string(),
            message: stderr,
        });
    }

    Ok(())
}

/// Reloads systemd daemon to pick up fstab changes.
pub fn reload_systemd_daemon() -> Result<()> {
    crate::syscall::daemon_reload()
}

/// Reloads systemd daemon with privilege escalation support.
pub fn reload_systemd_daemon_with_ctx(ctx: &mut ExecutionContext) -> Result<()> {
    crate::syscall::daemon_reload_with_ctx(ctx)
}

/// Starts a systemd mount unit for a mount point.
///
/// The unit name is derived from the mount point path.
pub fn start_mount_unit(mount_point: &Path) -> Result<()> {
    let unit_name = crate::syscall::mount_point_to_unit_name(mount_point);
    crate::syscall::start_unit(&unit_name)
}

/// Stops a systemd mount unit for a mount point.
pub fn stop_mount_unit(mount_point: &Path) -> Result<()> {
    let unit_name = crate::syscall::mount_point_to_unit_name(mount_point);
    crate::syscall::stop_unit(&unit_name)
}

#[cfg(test)]
mod tests {
    use super::*;

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
