//! Command execution abstraction with privilege escalation support.
//!
//! This module provides a flexible way to execute system commands with
//! optional privilege escalation via `pkexec` (GUI) or `sudo` (TTY).

use std::io::Write;
use std::process::{Command, Output, Stdio};

use crate::error::{Error, Result};

/// Privilege escalation method for executing commands that require root.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrivilegeEscalation {
    /// Execute directly without privilege escalation.
    #[default]
    None,
    /// Use `pkexec` for GUI-based privilege escalation (polkit).
    Pkexec,
    /// Use `sudo` for TTY-based privilege escalation.
    Sudo,
}

/// Execution context for running system commands.
///
/// This struct holds the configuration for how commands should be executed,
/// particularly whether they need privilege escalation.
///
/// # Example
///
/// ```
/// use steamos_mount_core::executor::ExecutionContext;
///
/// // Default: no privilege escalation
/// let ctx = ExecutionContext::default();
///
/// // For GUI applications (Tauri)
/// let gui_ctx = ExecutionContext::with_pkexec();
///
/// // For terminal applications
/// let tty_ctx = ExecutionContext::with_sudo();
/// ```
#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    escalation: PrivilegeEscalation,
}

impl ExecutionContext {
    /// Creates a new execution context with no privilege escalation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an execution context that uses `pkexec` for privileged commands.
    ///
    /// This is suitable for GUI applications where the user should see
    /// a graphical authentication dialog.
    pub fn with_pkexec() -> Self {
        Self {
            escalation: PrivilegeEscalation::Pkexec,
        }
    }

    /// Creates an execution context that uses `sudo` for privileged commands.
    ///
    /// This is suitable for terminal applications.
    pub fn with_sudo() -> Self {
        Self {
            escalation: PrivilegeEscalation::Sudo,
        }
    }

    /// Creates an execution context with a specific escalation method.
    pub fn with_escalation(escalation: PrivilegeEscalation) -> Self {
        Self { escalation }
    }

    /// Returns the current privilege escalation method.
    pub fn escalation(&self) -> PrivilegeEscalation {
        self.escalation
    }

    /// Executes a command that requires root privileges.
    ///
    /// The command will be wrapped with the appropriate privilege escalation
    /// method based on the context configuration.
    pub fn run_privileged(&self, cmd: &str, args: &[&str]) -> Result<Output> {
        match self.escalation {
            PrivilegeEscalation::None => run_command(cmd, args),
            PrivilegeEscalation::Pkexec => run_with_wrapper("pkexec", cmd, args),
            PrivilegeEscalation::Sudo => run_with_wrapper("sudo", cmd, args),
        }
    }

    /// Executes a command that requires root privileges, checking for success.
    ///
    /// Returns an error if the command fails or if authentication is cancelled.
    pub fn run_privileged_checked(&self, cmd: &str, args: &[&str]) -> Result<()> {
        let output = self.run_privileged(cmd, args)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            // Check for authentication cancellation (pkexec returns 126)
            if output.status.code() == Some(126) {
                return Err(Error::AuthenticationCancelled);
            }

            return Err(Error::CommandExit {
                command: cmd.to_string(),
                code: output.status.code().unwrap_or(-1),
                stderr,
            });
        }

        Ok(())
    }

    /// Writes content to a file with root privileges.
    ///
    /// Uses `tee` to write the content, wrapped with the appropriate
    /// privilege escalation method.
    pub fn write_file_privileged(&self, path: &str, content: &str) -> Result<()> {
        match self.escalation {
            PrivilegeEscalation::None => {
                std::fs::write(path, content).map_err(|e| Error::FstabWrite {
                    path: path.into(),
                    source: e,
                })
            }
            PrivilegeEscalation::Pkexec | PrivilegeEscalation::Sudo => {
                let wrapper = match self.escalation {
                    PrivilegeEscalation::Pkexec => "pkexec",
                    PrivilegeEscalation::Sudo => "sudo",
                    PrivilegeEscalation::None => unreachable!(),
                };

                let mut child = Command::new(wrapper)
                    .args(["tee", path])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .map_err(|e| Error::CommandExecution {
                        command: format!("{} tee", wrapper),
                        source: e,
                    })?;

                if let Some(mut stdin) = child.stdin.take() {
                    stdin
                        .write_all(content.as_bytes())
                        .map_err(|e| Error::FstabWrite {
                            path: path.into(),
                            source: e,
                        })?;
                }

                let status = child.wait().map_err(|e| Error::CommandExecution {
                    command: format!("{} tee", wrapper),
                    source: e,
                })?;

                if !status.success() {
                    if status.code() == Some(126) {
                        return Err(Error::AuthenticationCancelled);
                    }
                    return Err(Error::FstabWrite {
                        path: path.into(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Failed to write file with elevated privileges",
                        ),
                    });
                }

                Ok(())
            }
        }
    }

    /// Copies a file with root privileges.
    pub fn copy_file_privileged(&self, src: &str, dst: &str) -> Result<()> {
        self.run_privileged_checked("cp", &[src, dst])
    }

    /// Creates a directory with root privileges.
    pub fn mkdir_privileged(&self, path: &str) -> Result<()> {
        self.run_privileged_checked("mkdir", &["-p", path])
    }
}

/// Runs a command directly without any wrapper.
fn run_command(cmd: &str, args: &[&str]) -> Result<Output> {
    Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| Error::CommandExecution {
            command: cmd.to_string(),
            source: e,
        })
}

/// Runs a command with a privilege escalation wrapper (pkexec or sudo).
fn run_with_wrapper(wrapper: &str, cmd: &str, args: &[&str]) -> Result<Output> {
    let mut wrapper_args = vec![cmd];
    wrapper_args.extend(args);

    Command::new(wrapper)
        .args(&wrapper_args)
        .output()
        .map_err(|e| Error::CommandExecution {
            command: format!("{} {}", wrapper, cmd),
            source: e,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_context() {
        let ctx = ExecutionContext::default();
        assert_eq!(ctx.escalation(), PrivilegeEscalation::None);
    }

    #[test]
    fn test_pkexec_context() {
        let ctx = ExecutionContext::with_pkexec();
        assert_eq!(ctx.escalation(), PrivilegeEscalation::Pkexec);
    }

    #[test]
    fn test_sudo_context() {
        let ctx = ExecutionContext::with_sudo();
        assert_eq!(ctx.escalation(), PrivilegeEscalation::Sudo);
    }
}
