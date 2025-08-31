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

#[cfg(test)]
mod tests {
    // Note: These tests would require systemd which may not be available
    // in all test environments. They are here for documentation purposes.

    #[test]
    fn test_unit_name_format() {
        // This test documents the expected unit name format
        let mount_point = "/home/deck/Drives/GamesSSD";
        let expected_unit = "home-deck-Drives-GamesSSD.mount";

        let unit_name = mount_point.trim_start_matches('/').replace('/', "-") + ".mount";

        assert_eq!(unit_name, expected_unit);
    }
}
