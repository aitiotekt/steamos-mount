//! Systemd control module.
//!
//! This module provides functions to interact with systemd for reloading
//! the daemon, managing mount units, and restarting the display manager.

use std::process::Command;

use crate::error::{Error, IoResultExt, Result};

/// Reloads the systemd daemon to pick up configuration changes.
///
/// This is equivalent to running `systemctl daemon-reload`.
pub fn daemon_reload() -> Result<()> {
    run_systemctl(&["daemon-reload"])
}

/// Starts a systemd mount unit.
///
/// # Arguments
/// * `unit_name` - The name of the mount unit (e.g., "home-deck-Drives-GamesSSD.mount")
pub fn start_unit(unit_name: &str) -> Result<()> {
    run_systemctl(&["start", unit_name])
}

/// Stops a systemd mount unit.
///
/// # Arguments
/// * `unit_name` - The name of the mount unit
pub fn stop_unit(unit_name: &str) -> Result<()> {
    run_systemctl(&["stop", unit_name])
}

/// Restarts a systemd unit.
///
/// # Arguments
/// * `unit_name` - The name of the unit to restart
pub fn restart_unit(unit_name: &str) -> Result<()> {
    run_systemctl(&["restart", unit_name])
}

/// Checks if a unit is active.
///
/// Returns true if the unit is in "active" state.
pub fn is_unit_active(unit_name: &str) -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["is-active", unit_name])
        .output()
        .command_context(format!("systemctl is-active {}", unit_name))?;

    Ok(output.status.success())
}

/// Restarts the SDDM display manager.
///
/// This is used to restart the Steam UI after VDF injection.
pub fn restart_sddm() -> Result<()> {
    run_systemctl(&["restart", "sddm"])
}

/// Runs `steamos-session-select` to switch session.
///
/// # Arguments
/// * `session` - Session to select: "plasma" for Desktop or "gamescope" for Game Mode
pub fn session_select(session: &str) -> Result<()> {
    let output = Command::new("steamos-session-select")
        .arg(session)
        .output()
        .command_context(format!("steamos-session-select {}", session))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Systemd {
            message: format!("Failed to select session '{}': {}", session, stderr),
        });
    }

    Ok(())
}

/// Switches to Desktop Mode.
pub fn switch_to_desktop() -> Result<()> {
    session_select("plasma")
}

/// Switches to Game Mode.
pub fn switch_to_game_mode() -> Result<()> {
    session_select("gamescope")
}

/// Helper function to run systemctl commands.
fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .command_context(format!("systemctl {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(Error::Systemd { message: stderr });
    }

    Ok(())
}

/// Converts a mount point path to a systemd unit name.
///
/// Implements systemd path escaping logic:
/// 1. Removes leading slashes
/// 2. Replaces slashes with dashes
/// 3. Escapes other special characters (like spaces and dashes) as \xNN
///
/// Example: "/home/deck/Drives/GamesSSD" -> "home-deck-Drives-GamesSSD.mount"
/// Example: "/home/deck/Drives/My Drive" -> "home-deck-Drives-My\x20Drive.mount"
pub fn mount_point_to_unit_name(mount_point: &std::path::Path) -> String {
    let path_str = mount_point.to_string_lossy();
    let trimmed = path_str.trim_start_matches('/');

    if trimmed.is_empty() {
        return "-.mount".to_string();
    }

    let mut escaped = String::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        if c == '/' {
            escaped.push('-');
        } else if c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '.' {
            escaped.push(c);
        } else {
            escaped.push_str(&format!("\\x{:02x}", c as u32));
        }
    }

    format!("{}.mount", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
    fn test_mount_point_to_unit_name_escaped() {
        // "My Drive" -> "My\x20Drive"
        assert_eq!(
            mount_point_to_unit_name(Path::new("/home/deck/Drives/My Drive")),
            "home-deck-Drives-My\\x20Drive.mount"
        );
    }
}
