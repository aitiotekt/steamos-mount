//! Daemon mode implementation for privileged command execution.
//!
//! This module provides a daemon that runs with elevated privileges and
//! accepts signed commands via stdin, executing them and returning results via stdout.
//!
//! ## Security Model
//!
//! 1. On startup, generates a random secret and sends it to parent via handshake
//! 2. All requests must include a valid HMAC-SHA256 signature
//! 3. Request IDs must be monotonically increasing (anti-replay)
//! 4. Uses PR_SET_PDEATHSIG to terminate when parent dies

use std::fs;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use steamos_mount_core::protocol::{
    DaemonCommand, DaemonHandshake, DaemonRequest, DaemonResponse, generate_secret, verify_hmac,
};

/// Runs the daemon, reading requests from stdin and writing responses to stdout.
pub fn run_daemon() -> io::Result<()> {
    // Set up parent death signal to prevent orphan processes.
    // When parent dies, this process receives SIGTERM.
    #[cfg(target_os = "linux")]
    {
        use nix::libc;
        unsafe {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
        }
    }

    // Generate secret and send handshake
    let secret = generate_secret();
    let handshake = DaemonHandshake {
        secret: hex::encode(secret),
    };

    let mut stdout = io::stdout();
    let handshake_json = serde_json::to_string(&handshake).expect("Failed to serialize handshake");
    writeln!(stdout, "{}", handshake_json)?;
    stdout.flush()?;

    let stdin = io::stdin();
    let reader = stdin.lock();

    // Track last request ID for anti-replay
    let mut last_id: u64 = 0;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: DaemonRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                // Can't respond without an ID, log to stderr
                eprintln!("Failed to parse request: {}", e);
                continue;
            }
        };

        // Verify request ID is monotonically increasing (anti-replay)
        if request.id <= last_id {
            let response = error_response(
                request.id,
                format!(
                    "Replay attack detected: ID {} <= last ID {}",
                    request.id, last_id
                ),
            );
            write_response(&mut stdout, &response)?;
            continue;
        }

        // Verify HMAC signature
        let cmd_json =
            serde_json::to_string(&request.cmd).expect("Failed to serialize command for HMAC");
        if !verify_hmac(&secret, request.id, &cmd_json, &request.hmac) {
            let response = error_response(request.id, "HMAC authentication failed");
            write_response(&mut stdout, &response)?;
            continue;
        }

        // Update last ID after successful verification
        last_id = request.id;

        match request.cmd {
            DaemonCommand::Shutdown => {
                break;
            }
            DaemonCommand::Exec { program, args } => {
                let response = handle_exec(request.id, &program, &args);
                write_response(&mut stdout, &response)?;
            }
            DaemonCommand::WriteFile { path, content } => {
                let response = handle_write_file(request.id, &path, &content);
                write_response(&mut stdout, &response)?;
            }
            DaemonCommand::CopyFile { src, dst } => {
                let response = handle_copy_file(request.id, &src, &dst);
                write_response(&mut stdout, &response)?;
            }
            DaemonCommand::MkdirP { path } => {
                let response = handle_mkdir_p(request.id, &path);
                write_response(&mut stdout, &response)?;
            }
        }
    }

    Ok(())
}

fn write_response(stdout: &mut io::Stdout, response: &DaemonResponse) -> io::Result<()> {
    let json = serde_json::to_string(response).expect("Failed to serialize response");
    writeln!(stdout, "{}", json)?;
    stdout.flush()?;
    Ok(())
}

fn handle_exec(id: u64, program: &str, args: &[String]) -> DaemonResponse {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            DaemonResponse {
                id,
                success: exit_code == 0,
                exit_code,
                stdout,
                stderr,
                error: None,
            }
        }
        Err(e) => error_response(id, format!("Failed to execute command: {}", e)),
    }
}

fn handle_write_file(id: u64, path: &str, content: &str) -> DaemonResponse {
    match fs::write(path, content) {
        Ok(()) => success_response(id),
        Err(e) => error_response(id, format!("Failed to write file: {}", e)),
    }
}

fn handle_copy_file(id: u64, src: &str, dst: &str) -> DaemonResponse {
    match fs::copy(src, dst) {
        Ok(_) => success_response(id),
        Err(e) => error_response(id, format!("Failed to copy file: {}", e)),
    }
}

fn handle_mkdir_p(id: u64, path: &str) -> DaemonResponse {
    match fs::create_dir_all(path) {
        Ok(()) => success_response(id),
        Err(e) => error_response(id, format!("Failed to create directory: {}", e)),
    }
}

fn success_response(id: u64) -> DaemonResponse {
    DaemonResponse {
        id,
        success: true,
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        error: None,
    }
}

fn error_response(id: u64, message: impl Into<String>) -> DaemonResponse {
    DaemonResponse {
        id,
        success: false,
        exit_code: -1,
        stdout: String::new(),
        stderr: String::new(),
        error: Some(message.into()),
    }
}
