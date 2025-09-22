//! Unified error types for the steamos-mount-core library.
//!
//! Uses SNAFU for context-rich error handling, especially useful when the same
//! underlying error type (like `std::io::Error`) appears in different contexts.

use snafu::{ResultExt, Snafu};
use std::path::PathBuf;

/// Result type alias using the library's error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Unified error type for all core library operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// Failed to execute a system command.
    #[snafu(display("failed to execute command '{command}'"))]
    CommandExecution {
        command: String,
        source: std::io::Error,
    },

    /// Command executed but returned non-zero exit code.
    #[snafu(display("command '{command}' exited with code {code}: {stderr}"))]
    CommandExit {
        command: String,
        code: i32,
        stderr: String,
    },

    /// Failed to parse lsblk JSON output.
    #[snafu(display("failed to parse lsblk output: {message}"))]
    LsblkParse { message: String },

    /// Fstab file not found or cannot be read.
    #[snafu(display("failed to read fstab at {}", path.display()))]
    FstabRead {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to write fstab file.
    #[snafu(display("failed to write fstab at {}", path.display()))]
    FstabWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to parse fstab entry.
    #[snafu(display("failed to parse fstab entry: {message}"))]
    FstabParse { message: String },

    /// Failed to create backup.
    #[snafu(display("failed to create backup at {}", path.display()))]
    Backup {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Mount point creation failed.
    #[snafu(display("failed to create mount point at {}", path.display()))]
    MountPointCreation {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Mount point creation permission denied.
    #[snafu(display("permission denied creating mount point at {}", path.display()))]
    MountPointPermissionDenied { path: PathBuf },

    /// Mount operation failed.
    #[snafu(display("failed to mount device: {message}"))]
    Mount { message: String },

    /// Invalid filesystem type.
    #[snafu(display("Invalid filesystem type: {fs}"))]
    InvalidFilesystem { fs: String },

    /// Home directory not found.
    #[snafu(display("Could not determine home directory"))]
    HomeDirNotFound,

    /// Unmount operation failed.
    #[snafu(display("Failed to unmount {}: {message}", path.display()))]
    Unmount { path: PathBuf, message: String },

    /// Device has a dirty NTFS volume.
    #[snafu(display("device {device} has a dirty NTFS volume"))]
    DirtyVolume { device: String },

    /// ntfsfix repair failed.
    #[snafu(display("ntfsfix repair failed for {device}: {message}"))]
    Ntfsfix { device: String, message: String },

    /// Steam VDF file not found.
    #[snafu(display("Steam library folders VDF not found at {}", path.display()))]
    SteamVdfNotFound { path: PathBuf },

    /// Failed to parse Steam VDF file.
    #[snafu(display("failed to parse Steam VDF: {message}"))]
    VdfParse { message: String },

    /// Failed to write Steam VDF file.
    #[snafu(display("failed to write Steam VDF at {}", path.display()))]
    VdfWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Steam process control error.
    #[snafu(display("Steam process control error: {message}"))]
    SteamProcess { message: String },

    /// Systemd operation failed.
    #[snafu(display("systemd operation failed: {message}"))]
    Systemd { message: String },

    /// Invalid UUID format.
    #[snafu(display("invalid UUID format: {uuid}"))]
    InvalidUuid { uuid: String },

    /// User cancelled authentication dialog.
    #[snafu(display("authentication cancelled by user"))]
    AuthenticationCancelled,

    /// Sidecar binary not found.
    #[snafu(display(
        "sidecar binary not found at '{path}'. Please ensure the application is properly installed."
    ))]
    SidecarNotFound { path: String },

    /// Privilege escalation tool (pkexec/sudo) not found.
    #[snafu(display(
        "privilege escalation tool '{tool}' not found. Please install it to use this feature."
    ))]
    EscalationToolNotFound { tool: String },

    /// Failed to create privileged session.
    #[snafu(display("failed to create privileged session: {message}"))]
    SessionCreation { message: String },

    /// Failed to communicate with privileged session.
    #[snafu(display("session communication error: {message}"))]
    SessionCommunication { message: String },

    #[snafu(whatever, display("{message}"))]
    Generic {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
        source: Option<Box<dyn std::error::Error>>,
    },
}

/// Extension trait for adding context to io::Error results.
pub trait IoResultExt<T> {
    /// Add context for command execution errors.
    fn command_context(self, command: impl Into<String>) -> Result<T>;

    /// Add context for fstab read errors.
    fn fstab_read_context(self, path: impl Into<PathBuf>) -> Result<T>;

    /// Add context for fstab write errors.
    fn fstab_write_context(self, path: impl Into<PathBuf>) -> Result<T>;

    /// Add context for backup errors.
    fn backup_context(self, path: impl Into<PathBuf>) -> Result<T>;

    /// Add context for mount point creation errors.
    fn mount_point_context(self, path: impl Into<PathBuf>) -> Result<T>;

    /// Add context for VDF write errors.
    fn vdf_write_context(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoResultExt<T> for std::result::Result<T, std::io::Error> {
    fn command_context(self, command: impl Into<String>) -> Result<T> {
        self.context(CommandExecutionSnafu {
            command: command.into(),
        })
    }

    fn fstab_read_context(self, path: impl Into<PathBuf>) -> Result<T> {
        self.context(FstabReadSnafu { path: path.into() })
    }
    fn fstab_write_context(self, path: impl Into<PathBuf>) -> Result<T> {
        self.context(FstabWriteSnafu { path: path.into() })
    }

    fn backup_context(self, path: impl Into<PathBuf>) -> Result<T> {
        self.context(BackupSnafu { path: path.into() })
    }

    fn mount_point_context(self, path: impl Into<PathBuf>) -> Result<T> {
        self.context(MountPointCreationSnafu { path: path.into() })
    }

    fn vdf_write_context(self, path: impl Into<PathBuf>) -> Result<T> {
        self.context(VdfWriteSnafu { path: path.into() })
    }
}
