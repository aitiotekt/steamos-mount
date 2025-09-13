//! Command execution abstraction with privilege escalation support.
//!
//! This module provides a flexible way to execute system commands with
//! optional privilege escalation via `pkexec` (GUI) or `sudo` (TTY).
//! It also supports session mode where multiple commands can be executed
//! with a single authentication.
//!
//! ## Security Model
//!
//! Session mode uses HMAC-SHA256 signing to prevent unauthorized command injection:
//! 1. Daemon generates a random secret on startup and sends it to parent via stdout
//! 2. Each request includes an HMAC signature computed from the secret
//! 3. Request IDs must be monotonically increasing to prevent replay attacks

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::protocol::{
    DaemonCommand, DaemonHandshake, DaemonRequest, DaemonResponse, compute_hmac,
};

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
    /// Use `pkexec` to launch a daemon for session-based execution.
    PkexecSession,
    /// Use `sudo` to launch a daemon for session-based execution.
    SudoSession,
}

/// A privileged session that allows executing multiple commands with a single authentication.
///
/// The session spawns a `steamos-mount-cli daemon` process with elevated privileges
/// and communicates with it via signed JSON protocol over stdin/stdout.
///
/// ## Security
///
/// - The daemon generates a random secret on startup
/// - All requests are signed with HMAC-SHA256
/// - Request IDs must be monotonically increasing (anti-replay)
pub struct PrivilegedSession {
    child: Child,
    request_id: AtomicU64,
    /// The shared secret for HMAC signing (received from daemon).
    secret: Vec<u8>,
}

impl std::fmt::Debug for PrivilegedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivilegedSession")
            .field("request_id", &self.request_id.load(Ordering::SeqCst))
            .field("secret_len", &self.secret.len())
            .finish_non_exhaustive()
    }
}

impl PrivilegedSession {
    /// Spawns a new privileged session using the specified wrapper command.
    ///
    /// After spawning, reads the handshake message containing the shared secret.
    ///
    /// # Arguments
    /// * `wrapper` - Either "pkexec" or "sudo"
    /// * `cli_path` - Path to the steamos-mount-cli binary
    fn spawn(wrapper: &str, cli_path: &str) -> Result<Self> {
        let mut cmd = Command::new(wrapper);
        cmd.args([cli_path, "daemon"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| Error::SessionCreation {
            message: format!("Failed to spawn {} {}: {}", wrapper, cli_path, e),
        })?;

        // Read the handshake message containing the secret
        let stdout = child
            .stdout
            .as_mut()
            .ok_or_else(|| Error::SessionCreation {
                message: "Daemon stdout not available for handshake".to_string(),
            })?;

        let mut reader = BufReader::new(stdout);
        let mut handshake_line = String::new();
        reader
            .read_line(&mut handshake_line)
            .map_err(|e| Error::SessionCreation {
                message: format!("Failed to read handshake from daemon: {}", e),
            })?;

        let handshake: DaemonHandshake =
            serde_json::from_str(&handshake_line).map_err(|e| Error::SessionCreation {
                message: format!("Failed to parse daemon handshake: {}", e),
            })?;

        let secret = hex::decode(&handshake.secret).map_err(|e| Error::SessionCreation {
            message: format!("Failed to decode daemon secret: {}", e),
        })?;

        Ok(Self {
            child,
            request_id: AtomicU64::new(1),
            secret,
        })
    }

    /// Spawns a new privileged session using pkexec.
    pub fn spawn_pkexec() -> Result<Self> {
        Self::spawn("pkexec", "steamos-mount-cli")
    }

    /// Spawns a new privileged session using sudo.
    pub fn spawn_sudo() -> Result<Self> {
        Self::spawn("sudo", "steamos-mount-cli")
    }

    /// Gets the next request ID.
    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Creates a signed request for a command.
    fn create_signed_request(&self, id: u64, cmd: DaemonCommand) -> DaemonRequest {
        // Serialize the command for HMAC computation
        let cmd_json = serde_json::to_string(&cmd).expect("Failed to serialize command");
        let hmac = compute_hmac(&self.secret, id, &cmd_json);

        DaemonRequest { id, hmac, cmd }
    }

    /// Sends a request to the daemon and waits for a response.
    fn send_request(&mut self, request: &DaemonRequest) -> Result<DaemonResponse> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::SessionCommunication {
                message: "Daemon stdin not available".to_string(),
            })?;

        let json = serde_json::to_string(request).map_err(|e| Error::SessionCommunication {
            message: format!("Failed to serialize request: {}", e),
        })?;

        writeln!(stdin, "{}", json).map_err(|e| Error::SessionCommunication {
            message: format!("Failed to write to daemon: {}", e),
        })?;

        stdin.flush().map_err(|e| Error::SessionCommunication {
            message: format!("Failed to flush daemon stdin: {}", e),
        })?;

        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| Error::SessionCommunication {
                message: "Daemon stdout not available".to_string(),
            })?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| Error::SessionCommunication {
                message: format!("Failed to read from daemon: {}", e),
            })?;

        let response: DaemonResponse =
            serde_json::from_str(&line).map_err(|e| Error::SessionCommunication {
                message: format!("Failed to parse daemon response: {}", e),
            })?;

        // Check for authentication errors
        if !response.success
            && let Some(ref err) = response.error
            && (err.contains("authentication") || err.contains("HMAC"))
        {
            return Err(Error::SessionCommunication {
                message: format!("Security verification failed: {}", err),
            });
        }
        Ok(response)
    }

    /// Executes a command in the privileged session.
    pub fn run_command(&mut self, program: &str, args: &[&str]) -> Result<Output> {
        let id = self.next_id();
        let cmd = DaemonCommand::Exec {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        };
        let request = self.create_signed_request(id, cmd);

        let response = self.send_request(&request)?;

        // Convert response to Output-like structure
        Ok(Output {
            status: std::process::ExitStatus::from_raw(response.exit_code),
            stdout: response.stdout.into_bytes(),
            stderr: response.stderr.into_bytes(),
        })
    }

    /// Writes content to a file in the privileged session.
    pub fn write_file(&mut self, path: &str, content: &str) -> Result<()> {
        let id = self.next_id();
        let cmd = DaemonCommand::WriteFile {
            path: path.to_string(),
            content: content.to_string(),
        };
        let request = self.create_signed_request(id, cmd);

        let response = self.send_request(&request)?;

        if !response.success {
            return Err(Error::SessionCommunication {
                message: response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            });
        }

        Ok(())
    }

    /// Copies a file in the privileged session.
    pub fn copy_file(&mut self, src: &str, dst: &str) -> Result<()> {
        let id = self.next_id();
        let cmd = DaemonCommand::CopyFile {
            src: src.to_string(),
            dst: dst.to_string(),
        };
        let request = self.create_signed_request(id, cmd);

        let response = self.send_request(&request)?;

        if !response.success {
            return Err(Error::SessionCommunication {
                message: response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            });
        }

        Ok(())
    }

    /// Creates a directory with parents in the privileged session.
    pub fn mkdir_p(&mut self, path: &str) -> Result<()> {
        let id = self.next_id();
        let cmd = DaemonCommand::MkdirP {
            path: path.to_string(),
        };
        let request = self.create_signed_request(id, cmd);

        let response = self.send_request(&request)?;

        if !response.success {
            return Err(Error::SessionCommunication {
                message: response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            });
        }

        Ok(())
    }

    /// Shuts down the privileged session.
    pub fn shutdown(&mut self) -> Result<()> {
        // Create signed request first (before borrowing stdin)
        let id = self.next_id();
        let cmd = DaemonCommand::Shutdown;
        let request = self.create_signed_request(id, cmd);

        let json = serde_json::to_string(&request).map_err(|e| Error::SessionCommunication {
            message: format!("Failed to serialize shutdown request: {}", e),
        })?;

        // Now borrow stdin
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::SessionCommunication {
                message: "Daemon stdin not available".to_string(),
            })?;

        writeln!(stdin, "{}", json).map_err(|e| Error::SessionCommunication {
            message: format!("Failed to send shutdown to daemon: {}", e),
        })?;

        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for PrivilegedSession {
    fn drop(&mut self) {
        // Best effort shutdown
        let _ = self.shutdown();
    }
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
///
/// // For session-based execution (single auth for multiple commands)
/// let session_ctx = ExecutionContext::with_pkexec_session();
/// ```
#[derive(Debug, Default)]
pub struct ExecutionContext {
    escalation: PrivilegeEscalation,
    session: Option<Mutex<PrivilegedSession>>,
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
            session: None,
        }
    }

    /// Creates an execution context that uses `sudo` for privileged commands.
    ///
    /// This is suitable for terminal applications.
    pub fn with_sudo() -> Self {
        Self {
            escalation: PrivilegeEscalation::Sudo,
            session: None,
        }
    }

    /// Creates an execution context that uses `pkexec` session mode.
    ///
    /// In session mode, a single authentication is used for multiple commands.
    /// The session is started lazily on first privileged command execution.
    /// All commands are signed with HMAC-SHA256 for security.
    pub fn with_pkexec_session() -> Self {
        Self {
            escalation: PrivilegeEscalation::PkexecSession,
            session: None,
        }
    }

    /// Creates an execution context that uses `sudo` session mode.
    ///
    /// In session mode, a single authentication is used for multiple commands.
    /// The session is started lazily on first privileged command execution.
    /// All commands are signed with HMAC-SHA256 for security.
    pub fn with_sudo_session() -> Self {
        Self {
            escalation: PrivilegeEscalation::SudoSession,
            session: None,
        }
    }

    /// Creates an execution context with a specific escalation method.
    pub fn with_escalation(escalation: PrivilegeEscalation) -> Self {
        Self {
            escalation,
            session: None,
        }
    }

    /// Returns the current privilege escalation method.
    pub fn escalation(&self) -> PrivilegeEscalation {
        self.escalation
    }

    /// Ensures a session is started for session-based escalation modes.
    fn ensure_session(&mut self) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }

        let session = match self.escalation {
            PrivilegeEscalation::PkexecSession => PrivilegedSession::spawn_pkexec()?,
            PrivilegeEscalation::SudoSession => PrivilegedSession::spawn_sudo()?,
            _ => return Ok(()), // Not a session mode
        };

        self.session = Some(Mutex::new(session));
        Ok(())
    }

    /// Executes a command that requires root privileges.
    ///
    /// The command will be wrapped with the appropriate privilege escalation
    /// method based on the context configuration.
    pub fn run_privileged(&mut self, cmd: &str, args: &[&str]) -> Result<Output> {
        match self.escalation {
            PrivilegeEscalation::None => run_command(cmd, args),
            PrivilegeEscalation::Pkexec => run_with_wrapper("pkexec", cmd, args),
            PrivilegeEscalation::Sudo => run_with_wrapper("sudo", cmd, args),
            PrivilegeEscalation::PkexecSession | PrivilegeEscalation::SudoSession => {
                self.ensure_session()?;
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| Error::SessionCommunication {
                        message: "Session not available".to_string(),
                    })?;
                let mut guard = session.lock().map_err(|e| Error::SessionCommunication {
                    message: format!("Failed to lock session: {}", e),
                })?;
                guard.run_command(cmd, args)
            }
        }
    }

    /// Executes a command that requires root privileges, checking for success.
    ///
    /// Returns an error if the command fails or if authentication is cancelled.
    pub fn run_privileged_checked(&mut self, cmd: &str, args: &[&str]) -> Result<()> {
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
    pub fn write_file_privileged(&mut self, path: &str, content: &str) -> Result<()> {
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
                    _ => unreachable!(),
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
            PrivilegeEscalation::PkexecSession | PrivilegeEscalation::SudoSession => {
                self.ensure_session()?;
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| Error::SessionCommunication {
                        message: "Session not available".to_string(),
                    })?;
                let mut guard = session.lock().map_err(|e| Error::SessionCommunication {
                    message: format!("Failed to lock session: {}", e),
                })?;
                guard.write_file(path, content)
            }
        }
    }

    /// Copies a file with root privileges.
    pub fn copy_file_privileged(&mut self, src: &str, dst: &str) -> Result<()> {
        match self.escalation {
            PrivilegeEscalation::PkexecSession | PrivilegeEscalation::SudoSession => {
                self.ensure_session()?;
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| Error::SessionCommunication {
                        message: "Session not available".to_string(),
                    })?;
                let mut guard = session.lock().map_err(|e| Error::SessionCommunication {
                    message: format!("Failed to lock session: {}", e),
                })?;
                guard.copy_file(src, dst)
            }
            _ => self.run_privileged_checked("cp", &[src, dst]),
        }
    }

    /// Creates a directory with root privileges.
    pub fn mkdir_privileged(&mut self, path: &str) -> Result<()> {
        match self.escalation {
            PrivilegeEscalation::PkexecSession | PrivilegeEscalation::SudoSession => {
                self.ensure_session()?;
                let session = self
                    .session
                    .as_ref()
                    .ok_or_else(|| Error::SessionCommunication {
                        message: "Session not available".to_string(),
                    })?;
                let mut guard = session.lock().map_err(|e| Error::SessionCommunication {
                    message: format!("Failed to lock session: {}", e),
                })?;
                guard.mkdir_p(path)
            }
            _ => self.run_privileged_checked("mkdir", &["-p", path]),
        }
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

    #[test]
    fn test_pkexec_session_context() {
        let ctx = ExecutionContext::with_pkexec_session();
        assert_eq!(ctx.escalation(), PrivilegeEscalation::PkexecSession);
    }

    #[test]
    fn test_sudo_session_context() {
        let ctx = ExecutionContext::with_sudo_session();
        assert_eq!(ctx.escalation(), PrivilegeEscalation::SudoSession);
    }
}
