//! Command execution abstraction with privilege escalation support.
//!
//! This module provides a flexible way to execute system commands with
//! optional privilege escalation via `pkexec` (GUI) or `sudo` (TTY).
//! It supports session mode where a daemon process handles multiple commands
//! within a single command execution context.
//!
//! ## Security Model
//!
//! Session mode uses HMAC-SHA256 signing to prevent unauthorized command injection:
//! 1. Daemon generates a random secret on startup and sends it to parent via stdout
//! 2. Each request includes an HMAC signature computed from the secret
//! 3. Request IDs must be monotonically increasing to prevent replay attacks
//!
//! **Important**: Each command execution requires its own authorization. Sessions are
//! not shared between different command invocations to ensure explicit user consent
//! for each privileged operation.
//!
//! ## Architecture
//!
//! The module provides traits for abstracting daemon process management:
//! - [`DaemonChild`]: Trait for communicating with a daemon process (stdin/stdout)
//! - [`DaemonSpawner`]: Trait for lazily spawning daemon processes
//!
//! This allows different environments (Tauri, CLI, tests) to provide their own
//! implementations while sharing the core session logic.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::protocol::{
    DaemonCommand, DaemonHandshake, DaemonRequest, DaemonResponse, compute_hmac,
};

// ============================================================================
// Traits for daemon process abstraction
// ============================================================================

/// Trait for communicating with a daemon child process.
///
/// This trait abstracts the stdin/stdout/stderr operations needed to communicate
/// with a `steamos-mount-cli daemon` process. It allows different implementations
/// for different environments (e.g., `std::process::Child` for CLI, Tauri's
/// `CommandChild` for GUI).
///
/// # Thread Safety
///
/// Implementations must be `Send` to allow session management across threads.
pub trait DaemonChild: Send {
    /// Returns a mutable reference to the process stdin for writing requests.
    fn stdin(&mut self) -> Option<&mut dyn Write>;

    /// Returns a mutable reference to the process stdout for reading responses.
    fn stdout(&mut self) -> Option<&mut dyn BufRead>;

    /// Returns a mutable reference to stderr for reading error messages.
    fn stderr(&mut self) -> Option<&mut dyn Read>;

    /// Checks if the child process has exited.
    ///
    /// Returns `Some(exit_code)` if exited, `None` if still running.
    fn try_wait(&mut self) -> Result<Option<i32>>;

    /// Waits for the child process to exit.
    fn wait(&mut self) -> Result<i32>;

    /// Kills the child process.
    fn kill(&mut self) -> Result<()>;
}

/// Trait for spawning daemon processes.
///
/// This trait allows lazy spawning of daemon processes - the actual spawn
/// only happens when a privileged command is first needed within a command's
/// execution context. This allows a single command to execute multiple privileged
/// operations with a single authentication prompt, while still requiring separate
/// authorization for each command invocation.
///
/// # Example
///
/// ```ignore
/// // In Tauri, implement a spawner that uses pkexec with the sidecar:
/// struct TauriDaemonSpawner {
///     app: AppHandle,
/// }
///
/// impl DaemonSpawner for TauriDaemonSpawner {
///     fn spawn(&self) -> Result<Box<dyn DaemonChild>> {
///         let cmd = self.app.shell().command("pkexec").args([...]);
///         let (rx, child) = cmd.spawn()?;
///         Ok(Box::new(TauriDaemonChild::new(rx, child)))
///     }
/// }
/// ```
pub trait DaemonSpawner: Send + Sync {
    /// Spawns a new daemon process and returns a boxed DaemonChild.
    ///
    /// This method should spawn the daemon with appropriate privilege escalation
    /// (e.g., via pkexec or sudo) and return a handle for communication.
    fn spawn(&self) -> Result<Box<dyn DaemonChild>>;
}

// ============================================================================
// Standard library Child implementation
// ============================================================================

/// Wrapper around `std::process::Child` that implements `DaemonChild`.
///
/// This provides the default implementation for CLI and testing environments.
pub struct StdDaemonChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    stderr: Option<ChildStderr>,
}

impl StdDaemonChild {
    /// Creates a new StdDaemonChild from a std::process::Child.
    ///
    /// The child process must have stdin, stdout, and stderr piped.
    pub fn new(mut child: Child) -> Self {
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);
        let stderr = child.stderr.take();
        Self {
            child,
            stdin,
            stdout,
            stderr,
        }
    }

    /// Consumes self and returns the underlying Child.
    pub fn into_inner(mut self) -> Child {
        // Put back the streams we took
        if let Some(stdin) = self.stdin.take() {
            self.child.stdin = Some(stdin);
        }
        if let Some(stdout) = self.stdout.take() {
            self.child.stdout = Some(stdout.into_inner());
        }
        if let Some(stderr) = self.stderr.take() {
            self.child.stderr = Some(stderr);
        }
        self.child
    }
}

impl DaemonChild for StdDaemonChild {
    fn stdin(&mut self) -> Option<&mut dyn Write> {
        self.stdin.as_mut().map(|s| s as &mut dyn Write)
    }

    fn stdout(&mut self) -> Option<&mut dyn BufRead> {
        self.stdout.as_mut().map(|s| s as &mut dyn BufRead)
    }

    fn stderr(&mut self) -> Option<&mut dyn Read> {
        self.stderr.as_mut().map(|s| s as &mut dyn Read)
    }

    fn try_wait(&mut self) -> Result<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Ok(Some(status.code().unwrap_or(-1))),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::SessionCommunication {
                message: format!("Failed to check child status: {}", e),
            }),
        }
    }

    fn wait(&mut self) -> Result<i32> {
        match self.child.wait() {
            Ok(status) => Ok(status.code().unwrap_or(-1)),
            Err(e) => Err(Error::SessionCommunication {
                message: format!("Failed to wait for child: {}", e),
            }),
        }
    }

    fn kill(&mut self) -> Result<()> {
        self.child.kill().map_err(|e| Error::SessionCommunication {
            message: format!("Failed to kill child: {}", e),
        })
    }
}

// ============================================================================
// Standard library Spawner implementation
// ============================================================================

/// Spawner that uses `std::process::Command` to start the daemon.
///
/// This is the default spawner for CLI applications and testing environments.
/// It supports wrapping the daemon command with privilege escalation tools
/// like `pkexec` or `sudo`.
pub struct StdDaemonSpawner {
    /// Path to the steamos-mount-cli binary
    cli_path: String,
    /// Optional wrapper command (e.g., "pkexec" or "sudo")
    wrapper: Option<String>,
}

impl StdDaemonSpawner {
    /// Creates a new StdDaemonSpawner that will spawn the daemon directly.
    ///
    /// # Arguments
    /// * `cli_path` - Path to the steamos-mount-cli binary
    ///
    /// # Example
    /// ```
    /// use steamos_mount_core::executor::StdDaemonSpawner;
    ///
    /// let spawner = StdDaemonSpawner::new("steamos-mount-cli");
    /// ```
    pub fn new(cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
            wrapper: None,
        }
    }

    /// Creates a new StdDaemonSpawner that wraps the daemon with a privilege escalation tool.
    ///
    /// # Arguments
    /// * `wrapper` - The wrapper command (e.g., "pkexec" or "sudo")
    /// * `cli_path` - Path to the steamos-mount-cli binary
    ///
    /// # Example
    /// ```
    /// use steamos_mount_core::executor::StdDaemonSpawner;
    ///
    /// // For GUI applications
    /// let spawner = StdDaemonSpawner::with_wrapper("pkexec", "steamos-mount-cli");
    ///
    /// // For terminal applications
    /// let spawner = StdDaemonSpawner::with_wrapper("sudo", "steamos-mount-cli");
    /// ```
    pub fn with_wrapper(wrapper: impl Into<String>, cli_path: impl Into<String>) -> Self {
        Self {
            cli_path: cli_path.into(),
            wrapper: Some(wrapper.into()),
        }
    }
}

impl DaemonSpawner for StdDaemonSpawner {
    fn spawn(&self) -> Result<Box<dyn DaemonChild>> {
        // Always check if CLI binary exists
        if !std::path::Path::new(&self.cli_path).exists() {
            return Err(Error::SidecarNotFound {
                path: self.cli_path.clone(),
            });
        }

        // Check wrapper tool existence only for known standard tools (pkexec, sudo)
        // Other third-party wrappers may have special logic or may not be in PATH
        if let Some(ref wrapper) = self.wrapper {
            match wrapper.as_str() {
                "pkexec" | "sudo" => {
                    // Check if the wrapper tool exists by trying to get its version
                    if Command::new(wrapper)
                        .arg("--version")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .is_err()
                    {
                        return Err(Error::EscalationToolNotFound {
                            tool: wrapper.clone(),
                        });
                    }
                }
                // For other wrappers, don't check - they may have special logic
                // or may not be standard executables in PATH
                _ => {}
            }
        }

        let mut cmd = if let Some(ref wrapper) = self.wrapper {
            let mut c = Command::new(wrapper);
            c.arg(&self.cli_path);
            c.arg("daemon");
            c
        } else {
            let mut c = Command::new(&self.cli_path);
            c.arg("daemon");
            c
        };

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn().map_err(|e| {
            // Check if it's a "command not found" error
            if e.kind() == std::io::ErrorKind::NotFound {
                if let Some(ref wrapper) = self.wrapper {
                    // If wrapper is pkexec or sudo, we already checked, so this shouldn't happen
                    // But handle it gracefully anyway
                    if wrapper == "pkexec" || wrapper == "sudo" {
                        Error::EscalationToolNotFound {
                            tool: wrapper.clone(),
                        }
                    } else {
                        // For other wrappers, return a generic error
                        Error::SessionCreation {
                            message: format!("Wrapper tool '{}' not found: {}", wrapper, e),
                        }
                    }
                } else {
                    // CLI binary not found (shouldn't happen since we checked, but handle it)
                    Error::SidecarNotFound {
                        path: self.cli_path.clone(),
                    }
                }
            } else {
                Error::SessionCreation {
                    message: format!(
                        "Failed to spawn daemon{}: {}",
                        if let Some(w) = self.wrapper.as_ref() {
                            format!(" with {}", w)
                        } else {
                            String::new()
                        },
                        e
                    ),
                }
            }
        })?;

        Ok(Box::new(StdDaemonChild::new(child)))
    }
}

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

/// A privileged session that allows executing multiple commands within a single command execution.
///
/// The session communicates with a `steamos-mount-cli daemon` process via signed JSON protocol
/// over stdin/stdout. The daemon process should be started externally (e.g., via Tauri sidecar)
/// and passed to this session.
///
/// **Note**: Each command execution creates its own session. Sessions are not shared between
/// different command invocations to ensure each command requires explicit user authorization.
///
/// ## Security
///
/// - The daemon generates a random secret on startup
/// - All requests are signed with HMAC-SHA256
/// - Request IDs must be monotonically increasing (anti-replay)
pub struct PrivilegedSession {
    child: Box<dyn DaemonChild>,
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
    /// Creates a new privileged session from a DaemonChild implementation.
    ///
    /// The child process should be a running `steamos-mount-cli daemon` process
    /// with stdin, stdout, and stderr properly piped.
    ///
    /// This method will read the handshake message from the daemon to establish
    /// the shared secret for HMAC signing.
    ///
    /// # Arguments
    /// * `child` - The spawned daemon process (must implement DaemonChild)
    ///
    /// # Example
    /// ```ignore
    /// use std::process::{Command, Stdio};
    /// use steamos_mount_core::executor::{PrivilegedSession, StdDaemonChild};
    ///
    /// let child = Command::new("pkexec")
    ///     .args(["steamos-mount-cli", "daemon"])
    ///     .stdin(Stdio::piped())
    ///     .stdout(Stdio::piped())
    ///     .stderr(Stdio::piped())
    ///     .spawn()?;
    ///
    /// let session = PrivilegedSession::new(Box::new(StdDaemonChild::new(child)))?;
    /// ```
    pub fn new(mut child: Box<dyn DaemonChild>) -> Result<Self> {
        // Check if child process has already exited (e.g., authentication cancelled)
        if let Some(exit_code) = child.try_wait()? {
            let mut stderr_output = String::new();
            if let Some(stderr) = child.stderr() {
                let _ = stderr.read_to_string(&mut stderr_output);
            }
            let stderr_str = stderr_output.trim();

            // Error code 126: pkexec authentication cancelled by user
            if exit_code == 126 {
                return Err(Error::AuthenticationCancelled);
            }

            // Other exit codes: other authorization errors or daemon failures
            return Err(Error::SessionCreation {
                message: format!(
                    "Daemon process exited early with code {}: {}",
                    exit_code,
                    if stderr_str.is_empty() {
                        "No error message available".to_string()
                    } else {
                        stderr_str.to_string()
                    }
                ),
            });
        }

        // Read the handshake message containing the secret
        let stdout = child.stdout().ok_or_else(|| Error::SessionCreation {
            message: "Daemon stdout not available for handshake".to_string(),
        })?;

        let mut handshake_line = String::new();
        stdout
            .read_line(&mut handshake_line)
            .map_err(|e| Error::SessionCreation {
                message: format!("Failed to read handshake from daemon: {}", e),
            })?;

        // Check if handshake line is empty
        if handshake_line.trim().is_empty() {
            // Check if process exited
            if let Some(exit_code) = child.try_wait()?
                && exit_code == 126
            {
                return Err(Error::AuthenticationCancelled);
            }
            return Err(Error::SessionCreation {
                message: "Daemon sent empty handshake (process may have failed to start or authentication was cancelled)".to_string(),
            });
        }

        let handshake: DaemonHandshake =
            serde_json::from_str(handshake_line.trim()).map_err(|e| {
                // If JSON parse fails, check stderr for error messages
                let mut stderr_output = String::new();
                if let Some(stderr) = child.stderr() {
                    let _ = stderr.read_to_string(&mut stderr_output);
                }

                Error::SessionCreation {
                    message: format!(
                        "Failed to parse daemon handshake (received: {:?}): {}.{}",
                        handshake_line.trim(),
                        e,
                        if stderr_output.is_empty() {
                            "".to_string()
                        } else {
                            format!(" Stderr: {}", stderr_output.trim())
                        }
                    ),
                }
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

    /// Creates a new privileged session from a std::process::Child.
    ///
    /// This is a convenience method for the common case of using std::process::Child.
    /// For other child process types (e.g., Tauri's CommandChild), use `new()` directly.
    pub fn from_child(child: Child) -> Result<Self> {
        Self::new(Box::new(StdDaemonChild::new(child)))
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
        let json = serde_json::to_string(request).map_err(|e| Error::SessionCommunication {
            message: format!("Failed to serialize request: {}", e),
        })?;

        // Write to stdin
        {
            let stdin = self
                .child
                .stdin()
                .ok_or_else(|| Error::SessionCommunication {
                    message: "Daemon stdin not available".to_string(),
                })?;

            writeln!(stdin, "{}", json).map_err(|e| Error::SessionCommunication {
                message: format!("Failed to write to daemon: {}", e),
            })?;

            stdin.flush().map_err(|e| Error::SessionCommunication {
                message: format!("Failed to flush daemon stdin: {}", e),
            })?;
        }

        // Read from stdout
        let stdout = self
            .child
            .stdout()
            .ok_or_else(|| Error::SessionCommunication {
                message: "Daemon stdout not available".to_string(),
            })?;

        let mut line = String::new();
        stdout
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
            .stdin()
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
/// # Lazy Session Spawning
///
/// When using session-based escalation modes (`PkexecSession` or `SudoSession`),
/// the session is spawned lazily on first privileged command execution. This
/// avoids unnecessary authentication prompts for command sequences that don't
/// require privilege escalation.
///
/// To enable lazy spawning, provide a `DaemonSpawner` implementation using
/// `with_spawner()`. When a privileged command is executed and no session
/// exists, the spawner will be called to create one.
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
/// // For session-based execution with lazy spawning
/// // let session_ctx = ExecutionContext::with_spawner(my_spawner);
/// ```
pub struct ExecutionContext {
    escalation: PrivilegeEscalation,
    session: Option<Mutex<PrivilegedSession>>,
    /// Optional spawner for lazy session creation.
    spawner: Option<Box<dyn DaemonSpawner>>,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("escalation", &self.escalation)
            .field("has_session", &self.session.is_some())
            .field("has_spawner", &self.spawner.is_some())
            .finish()
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            escalation: PrivilegeEscalation::None,
            session: None,
            spawner: None,
        }
    }
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
            spawner: None,
        }
    }

    /// Creates an execution context that uses `sudo` for privileged commands.
    ///
    /// This is suitable for terminal applications.
    pub fn with_sudo() -> Self {
        Self {
            escalation: PrivilegeEscalation::Sudo,
            session: None,
            spawner: None,
        }
    }

    /// Creates an execution context that uses `pkexec` session mode.
    ///
    /// **Note**: This creates a context without a spawner. You should either:
    /// - Call `set_session()` to provide a pre-created session, or
    /// - Use `with_spawner()` for lazy session creation
    ///
    /// Otherwise, session-based commands will fail with an error.
    pub fn with_pkexec_session() -> Self {
        Self {
            escalation: PrivilegeEscalation::PkexecSession,
            session: None,
            spawner: None,
        }
    }

    /// Creates an execution context that uses `sudo` session mode.
    ///
    /// **Note**: This creates a context without a spawner. You should either:
    /// - Call `set_session()` to provide a pre-created session, or
    /// - Use `with_spawner()` for lazy session creation
    ///
    /// Otherwise, session-based commands will fail with an error.
    pub fn with_sudo_session() -> Self {
        Self {
            escalation: PrivilegeEscalation::SudoSession,
            session: None,
            spawner: None,
        }
    }

    /// Creates an execution context with a spawner for lazy session creation.
    ///
    /// The session will be created lazily when the first privileged command
    /// is executed within this command's execution context. This allows a single
    /// command to execute multiple privileged operations with a single authentication
    /// prompt, while still requiring separate authorization for each command invocation.
    ///
    /// # Arguments
    /// * `escalation` - The privilege escalation method (should be PkexecSession or SudoSession)
    /// * `spawner` - The spawner to use for creating the daemon process
    ///
    /// # Example
    /// ```ignore
    /// let spawner = MyDaemonSpawner::new(app_handle);
    /// let ctx = ExecutionContext::with_spawner(
    ///     PrivilegeEscalation::PkexecSession,
    ///     Box::new(spawner),
    /// );
    /// ```
    pub fn with_spawner(escalation: PrivilegeEscalation, spawner: Box<dyn DaemonSpawner>) -> Self {
        Self {
            escalation,
            session: None,
            spawner: Some(spawner),
        }
    }

    /// Creates an execution context with a specific escalation method.
    pub fn with_escalation(escalation: PrivilegeEscalation) -> Self {
        Self {
            escalation,
            session: None,
            spawner: None,
        }
    }

    /// Returns the current privilege escalation method.
    pub fn escalation(&self) -> PrivilegeEscalation {
        self.escalation
    }

    /// Sets the spawner for lazy session creation.
    ///
    /// This allows providing a spawner after context creation.
    pub fn set_spawner(&mut self, spawner: Box<dyn DaemonSpawner>) {
        self.spawner = Some(spawner);
    }

    /// Sets the privileged session for this execution context.
    ///
    /// This should be called before using session-based escalation modes.
    /// The session should be created from an externally spawned daemon process
    /// (e.g., via Tauri sidecar).
    ///
    /// # Arguments
    /// * `session` - The privileged session to use
    pub fn set_session(&mut self, session: PrivilegedSession) {
        self.session = Some(Mutex::new(session));
    }

    /// Ensures a session is available for session-based escalation modes.
    ///
    /// If a session doesn't exist but a spawner is available, the session
    /// will be created lazily when the first privileged command is executed.
    /// This allows a single command to execute multiple privileged operations
    /// with a single authentication prompt.
    ///
    /// **Note**: Each command execution creates its own session. Sessions are not
    /// shared between different command invocations.
    ///
    /// Returns an error if a session is required but neither a session nor
    /// a spawner is available.
    fn ensure_session(&mut self) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }

        // Check if we need a session for this escalation mode
        match self.escalation {
            PrivilegeEscalation::PkexecSession | PrivilegeEscalation::SudoSession => {
                // Try to spawn a session using the spawner
                if let Some(ref spawner) = self.spawner {
                    let child = spawner.spawn()?;
                    let session = PrivilegedSession::new(child)?;
                    self.session = Some(Mutex::new(session));
                    Ok(())
                } else {
                    Err(Error::SessionCreation {
                        message: "Privileged session not available. Either set a session using set_session() or provide a spawner using set_spawner().".to_string(),
                    })
                }
            }
            _ => Ok(()), // Not a session mode
        }
    }

    /// Returns whether a session is currently active.
    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    /// Returns whether a spawner is configured for lazy session creation.
    pub fn has_spawner(&self) -> bool {
        self.spawner.is_some()
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
    // Check if wrapper tool exists (only for known standard tools)
    match wrapper {
        "pkexec" | "sudo" => {
            // Check if the wrapper tool exists by trying to get its version
            if Command::new(wrapper)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_err()
            {
                return Err(Error::EscalationToolNotFound {
                    tool: wrapper.to_string(),
                });
            }
        }
        // For other wrappers, don't check - they may have special logic
        // or may not be standard executables in PATH
        _ => {}
    }

    let mut wrapper_args = vec![cmd];
    wrapper_args.extend(args);

    Command::new(wrapper)
        .args(&wrapper_args)
        .output()
        .map_err(|e| {
            // Check if it's a "command not found" error for the wrapper
            if e.kind() == std::io::ErrorKind::NotFound {
                if wrapper == "pkexec" || wrapper == "sudo" {
                    Error::EscalationToolNotFound {
                        tool: wrapper.to_string(),
                    }
                } else {
                    Error::CommandExecution {
                        command: format!("{} {}", wrapper, cmd),
                        source: e,
                    }
                }
            } else {
                Error::CommandExecution {
                    command: format!("{} {}", wrapper, cmd),
                    source: e,
                }
            }
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
