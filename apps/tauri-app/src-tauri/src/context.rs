//! Tauri-specific spawner and context management.
//!
//! This module provides:
//! - TauriPkexecSpawner: A DaemonSpawner implementation for Tauri apps
//! - Error conversion utilities for user-friendly messages
//! - Context creation and wrapper functions for privileged operations
//!
//! ## Authorization Model
//!
//! Each command that requires privilege escalation creates its own authorization session.
//! Commands do not share sessions between invocations, ensuring explicit user consent
//! for each privileged operation. This means each command will prompt for authorization
//! when it needs to perform privileged actions.
use std::process::{Command, Stdio};

use snafu::ResultExt;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

use steamos_mount_core::{
    DaemonChild, DaemonSpawner, ExecutionContext, PrivilegeEscalation, StdDaemonChild,
};

// ============================================================================
// Tauri DaemonSpawner implementation
// ============================================================================

/// Spawner that uses Tauri sidecar with pkexec for privilege escalation.
///
/// This spawner resolves the sidecar path using Tauri's resource system
/// and wraps it with pkexec for GUI-based privilege escalation.
pub struct TauriPkexecSpawner {
    sidecar_path: String,
}

impl TauriPkexecSpawner {
    /// Creates a new TauriPkexecSpawner from an AppHandle.
    pub fn new(app: &AppHandle) -> Result<Self, steamos_mount_core::Error> {
        use std::fs;
        // Resolve sidecar path using Tauri's resource system
        let target_triple = tauri::utils::platform::target_triple()
            .with_whatever_context(|e| format!("Failed to get target triple: {}", e))?;
        let exe_suffix = std::env::consts::EXE_SUFFIX;
        let sidecar_rel_name = format!("bin/steamos-mount-cli-{}{}", target_triple, exe_suffix);
        let sidecar_resource_path = app
            .path()
            .resolve(&sidecar_rel_name, BaseDirectory::Resource)
            .with_whatever_context(|e| {
                format!(
                    "Failed to resolve resource path of '{}': {}",
                    &sidecar_rel_name, e
                )
            })?;
        let sidecar_appdata_path = app
            .path()
            .resolve(&sidecar_rel_name, BaseDirectory::AppData)
            .with_whatever_context(|e| {
                format!(
                    "Failed to resolve appdata path of '{}': {}",
                    &sidecar_rel_name, e
                )
            })?;

        if !sidecar_appdata_path.exists() {
            if !sidecar_resource_path.exists() {
                return Err(steamos_mount_core::Error::SidecarNotFound {
                    path: sidecar_resource_path.to_string_lossy().to_string(),
                });
            }

            if let Some(sidecar_appdata_dir) = sidecar_appdata_path.parent()
                && !sidecar_appdata_dir.exists()
            {
                fs::create_dir_all(sidecar_appdata_dir).with_whatever_context(|e| {
                    format!(
                        "Failed to create directory '{}': {}",
                        sidecar_appdata_dir.display(),
                        e
                    )
                })?;
            }

            fs::copy(&sidecar_resource_path, &sidecar_appdata_path).with_whatever_context(|e| {
                format!("Failed to copy sidecar from resource to appdata: {}", e)
            })?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut perms = fs::metadata(&sidecar_appdata_path)
                .with_whatever_context(|e| {
                    format!(
                        "Failed to get metadata of '{}': {}",
                        sidecar_appdata_path.display(),
                        e
                    )
                })?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&sidecar_appdata_path, perms).with_whatever_context(|e| {
                format!(
                    "Failed to set permissions of '{}': {}",
                    sidecar_appdata_path.display(),
                    e
                )
            })?;
        }

        Ok(Self {
            sidecar_path: sidecar_appdata_path.to_string_lossy().to_string(),
        })
    }
}

impl DaemonSpawner for TauriPkexecSpawner {
    fn spawn(&self) -> steamos_mount_core::Result<Box<dyn DaemonChild>> {
        // First, check if sidecar binary exists
        if !std::path::Path::new(&self.sidecar_path).exists() {
            return Err(steamos_mount_core::Error::SidecarNotFound {
                path: self.sidecar_path.clone(),
            });
        }

        // Check if pkexec exists
        if Command::new("pkexec")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return Err(steamos_mount_core::Error::EscalationToolNotFound {
                tool: "pkexec".to_string(),
            });
        }

        // Use std::process::Command with pkexec to spawn the daemon
        // This gives us a std::process::Child that we can wrap in StdDaemonChild
        let child = Command::new("pkexec")
            .arg(&self.sidecar_path)
            .arg("daemon")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                // Check if it's a "command not found" error for pkexec
                if e.kind() == std::io::ErrorKind::NotFound {
                    steamos_mount_core::Error::EscalationToolNotFound {
                        tool: "pkexec".to_string(),
                    }
                } else {
                    steamos_mount_core::Error::SessionCreation {
                        message: format!(
                            "Failed to spawn pkexec with sidecar '{}': {}",
                            self.sidecar_path, e
                        ),
                    }
                }
            })?;

        Ok(Box::new(StdDaemonChild::new(child)))
    }
}

// ============================================================================
// Error conversion utilities
// ============================================================================

/// Converts core library errors to user-friendly error messages.
///
/// This function provides detailed, actionable error messages for different
/// error scenarios, making it easier for users to understand and resolve issues.
fn error_to_user_message(error: &steamos_mount_core::Error) -> String {
    match error {
        steamos_mount_core::Error::SidecarNotFound { path } => {
            format!(
                "Sidecar binary not found at '{}'. This may indicate:\n\
                - The application was not properly installed\n\
                - The binary file is missing or corrupted\n\
                - Please reinstall the application",
                path
            )
        }
        steamos_mount_core::Error::EscalationToolNotFound { tool } => {
            format!(
                "Privilege escalation tool '{}' not found. Please install it:\n\
                - On Debian/Ubuntu: sudo apt install policykit-1\n\
                - On Arch Linux: sudo pacman -S polkit\n\
                - On Fedora: sudo dnf install polkit",
                tool
            )
        }
        steamos_mount_core::Error::AuthenticationCancelled => {
            "Authentication cancelled by user".to_string()
        }
        steamos_mount_core::Error::SessionCommunication { message } => {
            format!("Session communication error: {}", message)
        }
        // For other errors, use the default Display implementation
        _ => error.to_string(),
    }
}

// ============================================================================
// Context creation
// ============================================================================

/// Creates a new execution context with pkexec session for GUI privilege escalation.
///
/// This function creates a new ExecutionContext with a TauriPkexecSpawner.
/// **Each call creates a new context**, ensuring each command requires its own authorization.
/// The daemon is spawned lazily - only when a privileged command is first executed within
/// that command's execution context.
///
/// **Important**: Sessions are not shared between different command invocations. Each
/// command will prompt for authorization when it needs to perform privileged actions.
///
/// Returns a new execution context.
/// Errors are returned as core errors for unified error handling.
pub fn create_privileged_context(app: &AppHandle) -> steamos_mount_core::Result<ExecutionContext> {
    // Create spawner for lazy session creation
    let spawner = TauriPkexecSpawner::new(app)
        .with_whatever_context(|e| format!("Failed to create spawner: {}", e))?;

    // Create execution context with spawner
    // The session will be created lazily when first needed
    let ctx = ExecutionContext::with_spawner(PrivilegeEscalation::PkexecSession, Box::new(spawner));

    Ok(ctx)
}

/// Creates a new non-privileged execution context.
///
/// This function creates a new default ExecutionContext without privilege escalation.
/// Each call creates a new context instance.
/// Errors are returned as core errors for unified error handling.
pub fn create_non_privileged_context() -> steamos_mount_core::Result<ExecutionContext> {
    let ctx = ExecutionContext::default();
    Ok(ctx)
}

// ============================================================================
// Command execution wrappers
// ============================================================================

/// Executes a command that requires privilege escalation context,
/// with automatic error conversion for core library errors.
///
/// This function provides a unified wrapper for commands that need privileged execution.
/// **Each call creates a new privileged context**, requiring its own authorization.
/// Sessions are not shared between different command invocations.
///
/// It handles:
/// - Creating a new privileged context for this command (with error conversion for sidecar/pkexec errors)
/// - Creating a non-privileged context for operations that don't require escalation
/// - Converting privilege-related errors to user-friendly messages
/// - Locking the contexts for thread-safe access
///
/// # Arguments
/// * `app` - The Tauri AppHandle
/// * `command_impl` - A closure that receives both privileged and non-privileged contexts
///   and performs the actual command logic
///
/// # Returns
/// * `Ok(T)` - The result from the command implementation
/// * `Err(String)` - User-friendly error message
///
/// # Example
/// ```ignore
/// command_in_privileged_context(&app, |privileged_ctx, non_privileged_ctx| {
///     // Use privileged_ctx for operations requiring root
///     mount::mount_device_with_ctx(device, &mount_point, privileged_ctx)
/// })
/// ```
pub fn command_in_privileged_context<F, T>(app: &AppHandle, command_impl: F) -> Result<T, String>
where
    F: FnOnce(&mut ExecutionContext, &mut ExecutionContext) -> steamos_mount_core::Result<T>,
{
    // Create a new privileged context for this command (each command requires its own authorization)
    let mut privileged_ctx =
        create_privileged_context(app).map_err(|e| error_to_user_message(&e))?;
    let mut non_privileged_ctx =
        create_non_privileged_context().map_err(|e| error_to_user_message(&e))?;

    // Execute the command implementation and convert errors
    command_impl(&mut privileged_ctx, &mut non_privileged_ctx)
        .map_err(|e| error_to_user_message(&e))
}

/// Executes a command that does not require privilege escalation context,
/// with automatic error conversion for core library errors.
///
/// This function provides a unified wrapper for commands that don't need privileged execution.
/// It handles:
/// - Creating a new non-privileged context for this command
/// - Converting errors to user-friendly messages
/// - Locking the context for thread-safe access
///
/// # Arguments
/// * `command_impl` - A closure that receives a locked context and performs the actual command logic
///
/// # Returns
/// * `Ok(T)` - The result from the command implementation
/// * `Err(String)` - User-friendly error message
///
/// # Example
/// ```ignore
/// command_in_non_privileged_context(|_ctx| {
///     disk::list_block_devices()
/// })
/// ```
pub fn command_in_non_privileged_context<F, T>(command_impl: F) -> Result<T, String>
where
    F: FnOnce(&mut ExecutionContext) -> steamos_mount_core::Result<T>,
{
    // Get or create non-privileged context
    let mut ctx = create_non_privileged_context().map_err(|e| error_to_user_message(&e))?;

    // Execute the command implementation and convert errors
    command_impl(&mut ctx).map_err(|e| error_to_user_message(&e))
}
