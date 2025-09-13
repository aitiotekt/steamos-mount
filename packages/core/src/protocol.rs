//! Protocol types for privileged daemon session communication.
//!
//! This module defines the JSON-RPC style protocol used between the
//! main process and the privileged daemon subprocess (`steamos-mount-cli daemon`).
//!
//! ## Security Model
//!
//! The protocol uses HMAC-SHA256 signing to prevent unauthorized command injection:
//! 1. Daemon generates a random 32-byte secret on startup and sends it to parent
//! 2. Each request includes an HMAC signature: `HMAC-SHA256(secret, id || cmd_json)`
//! 3. Daemon verifies signature and rejects requests with invalid signatures
//! 4. Request IDs must be monotonically increasing to prevent replay attacks

use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Secret length in bytes.
pub const SECRET_LENGTH: usize = 32;

/// Generates a random secret for HMAC signing.
pub fn generate_secret() -> [u8; SECRET_LENGTH] {
    let mut rng = rand::rng();
    let mut secret = [0u8; SECRET_LENGTH];
    rng.fill(&mut secret);
    secret
}

/// Computes HMAC-SHA256 signature for a request.
pub fn compute_hmac(secret: &[u8], id: u64, payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(&id.to_le_bytes());
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Verifies HMAC-SHA256 signature for a request.
pub fn verify_hmac(secret: &[u8], id: u64, payload: &str, signature: &str) -> bool {
    let expected = compute_hmac(secret, id, payload);
    // Constant-time comparison to prevent timing attacks
    constant_time_eq(&expected, signature)
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Initial handshake message sent by daemon to parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHandshake {
    /// Hex-encoded secret for HMAC signing.
    pub secret: String,
}

/// Request sent to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRequest {
    /// Request ID (must be monotonically increasing).
    pub id: u64,
    /// HMAC-SHA256 signature of (id || cmd_json).
    pub hmac: String,
    /// The actual command.
    #[serde(flatten)]
    pub cmd: DaemonCommand,
}

/// Command types for the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DaemonCommand {
    /// Execute a command.
    Exec {
        /// Program to execute.
        program: String,
        /// Arguments to pass.
        args: Vec<String>,
    },
    /// Write content to a file.
    WriteFile {
        /// File path.
        path: String,
        /// Content to write.
        content: String,
    },
    /// Copy a file.
    CopyFile {
        /// Source path.
        src: String,
        /// Destination path.
        dst: String,
    },
    /// Create a directory with parents.
    MkdirP {
        /// Directory path.
        path: String,
    },
    /// Shutdown the daemon.
    Shutdown,
}

/// Response from the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonResponse {
    /// Request ID this response corresponds to.
    pub id: u64,
    /// Whether the operation succeeded.
    pub success: bool,
    /// Exit code for exec commands, 0 for file operations.
    #[serde(default)]
    pub exit_code: i32,
    /// Standard output (for exec commands).
    #[serde(default)]
    pub stdout: String,
    /// Standard error (for exec commands) or error message.
    #[serde(default)]
    pub stderr: String,
    /// Error message if success is false.
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sign_and_verify() {
        let secret = generate_secret();
        let id = 1u64;
        let payload = r#"{"cmd":"exec","program":"ls","args":["-la"]}"#;

        let signature = compute_hmac(&secret, id, payload);
        assert!(verify_hmac(&secret, id, payload, &signature));

        // Wrong secret should fail
        let wrong_secret = generate_secret();
        assert!(!verify_hmac(&wrong_secret, id, payload, &signature));

        // Wrong id should fail
        assert!(!verify_hmac(&secret, 2, payload, &signature));

        // Wrong payload should fail
        assert!(!verify_hmac(&secret, id, "wrong", &signature));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
    }
}
