//! SteamOS Mount CLI - Command line interface for mount operations.
//!
//! This CLI provides both interactive commands and a daemon mode for
//! privileged session execution.

mod daemon;
mod protocol;

use clap::{Parser, Subcommand};

/// SteamOS Mount CLI tool.
#[derive(Parser)]
#[command(name = "steamos-mount-cli")]
#[command(about = "CLI for SteamOS mount operations", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as a privileged daemon, accepting commands via stdin.
    ///
    /// This mode is intended to be launched via pkexec or sudo,
    /// allowing the parent process to execute multiple privileged
    /// commands without repeated authentication.
    Daemon,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon => {
            if let Err(e) = daemon::run_daemon() {
                eprintln!("Daemon error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
