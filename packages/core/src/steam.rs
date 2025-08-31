//! Steam VDF parsing and library folder injection.
//!
//! This module handles parsing Steam's `libraryfolders.vdf` configuration file
//! and injecting new library folder entries.
//!
//! Uses the `keyvalues-serde` crate for robust VDF parsing with serde support.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoResultExt, Result};

/// Represents a Steam library folder entry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibraryFolder {
    /// Path to the library folder.
    pub path: PathBuf,
    /// Optional label for the library.
    #[serde(default)]
    pub label: String,
    /// Content ID (typically "0" for custom folders).
    #[serde(default)]
    pub contentid: String,
    /// Total size (typically "0" for custom folders).
    #[serde(default)]
    pub totalsize: String,
    /// Map of app IDs to sizes.
    #[serde(default)]
    pub apps: HashMap<String, String>,
}

/// Root structure for libraryfolders.vdf.
#[derive(Debug, Deserialize)]
struct LibraryFoldersRoot {
    #[serde(flatten)]
    folders: HashMap<String, LibraryFolder>,
}

/// Returns the path to Steam's libraryfolders.vdf file.
pub fn steam_library_vdf_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(Error::HomeDirNotFound)?;
    let vdf_path = home
        .join(".local")
        .join("share")
        .join("Steam")
        .join("steamapps")
        .join("libraryfolders.vdf");

    if !vdf_path.exists() {
        return Err(Error::SteamVdfNotFound {
            path: vdf_path.clone(),
        });
    }

    Ok(vdf_path)
}

/// Checks if Steam is currently running.
pub fn is_steam_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "steam"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Shuts down Steam gracefully.
///
/// Sends a shutdown signal and waits for the process to terminate.
pub fn shutdown_steam() -> Result<()> {
    if !is_steam_running() {
        return Ok(());
    }

    // Use Steam's built-in shutdown command
    let output = Command::new("steam")
        .arg("--shutdown")
        .output()
        .command_context("steam --shutdown")?;

    // Give Steam time to shut down
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Verify Steam has stopped
    for _ in 0..10 {
        if !is_steam_running() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if is_steam_running() {
        return Err(Error::SteamProcess {
            message: "Steam did not shut down within timeout".to_string(),
        });
    }

    // Check if the command reported an error
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Steam --shutdown often returns non-zero even when successful
        if !stderr.is_empty() && !stderr.contains("Steam is not running") {
            return Err(Error::SteamProcess {
                message: stderr.to_string(),
            });
        }
    }

    Ok(())
}

/// Parses the libraryfolders.vdf file.
pub fn parse_library_folders(path: &Path) -> Result<Vec<(String, LibraryFolder)>> {
    let content = fs::read_to_string(path).vdf_write_context(path)?;

    parse_library_folders_content(&content)
}

/// Parses libraryfolders.vdf content.
fn parse_library_folders_content(content: &str) -> Result<Vec<(String, LibraryFolder)>> {
    let root: LibraryFoldersRoot =
        keyvalues_serde::from_str(content).map_err(|e| Error::VdfParse {
            message: format!("Failed to parse VDF: {}", e),
        })?;

    // Filter to only numeric IDs (actual library folders) and sort by ID
    let mut folders: Vec<(String, LibraryFolder)> = root
        .folders
        .into_iter()
        .filter(|(id, _)| id.chars().all(|c| c.is_ascii_digit()))
        .collect();

    folders.sort_by(|(a, _), (b, _)| {
        a.parse::<u32>()
            .unwrap_or(0)
            .cmp(&b.parse::<u32>().unwrap_or(0))
    });

    Ok(folders)
}

/// Injects a new library folder into libraryfolders.vdf.
///
/// Note: Steam must be shut down before calling this function.
pub fn inject_library_folder(vdf_path: &Path, mount_path: &Path, label: &str) -> Result<()> {
    let folders = parse_library_folders(vdf_path)?;

    // Check if the path already exists
    if folders.iter().any(|(_, f)| f.path == mount_path) {
        // Already registered, nothing to do
        return Ok(());
    }

    // Calculate next ID
    let next_id = folders
        .iter()
        .filter_map(|(id, _)| id.parse::<u32>().ok())
        .max()
        .map(|n| n + 1)
        .unwrap_or(1);

    // Read original content
    let content = fs::read_to_string(vdf_path).vdf_write_context(vdf_path)?;

    // Build new folder entry
    let new_entry = format!(
        r#"	"{}"
	{{
		"path"		"{}"
		"label"		"{}"
		"contentid"		"0"
		"totalsize"		"0"
		"apps"
		{{
		}}
	}}"#,
        next_id,
        mount_path.display(),
        label
    );

    // Insert before the final closing brace
    let output = if let Some(last_brace) = content.rfind('}') {
        let (before, after) = content.split_at(last_brace);
        format!("{}\n{}\n{}", before.trim_end(), new_entry, after)
    } else {
        return Err(Error::VdfParse {
            message: "Could not find closing brace in VDF file".to_string(),
        });
    };

    fs::write(vdf_path, output).vdf_write_context(vdf_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_VDF: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"/home/deck/.local/share/Steam"
		"label"		""
		"contentid"		"1234567890"
		"totalsize"		"0"
		"apps"
		{
			"730"		"12345678"
			"440"		"87654321"
		}
	}
	"1"
	{
		"path"		"/run/media/mmcblk0p1"
		"label"		"SD Card"
		"contentid"		"0"
		"totalsize"		"0"
		"apps"
		{
		}
	}
}"#;

    #[test]
    fn test_parse_library_folders() {
        let folders = parse_library_folders_content(SAMPLE_VDF).unwrap();

        assert_eq!(folders.len(), 2);

        // Check first folder (ID "0")
        let (id0, folder0) = &folders[0];
        assert_eq!(id0, "0");
        assert_eq!(folder0.path, PathBuf::from("/home/deck/.local/share/Steam"));
        assert_eq!(folder0.label, "");
        assert_eq!(folder0.apps.len(), 2);
        assert_eq!(folder0.apps.get("730"), Some(&"12345678".to_string()));

        // Check second folder (ID "1")
        let (id1, folder1) = &folders[1];
        assert_eq!(id1, "1");
        assert_eq!(folder1.path, PathBuf::from("/run/media/mmcblk0p1"));
        assert_eq!(folder1.label, "SD Card");
    }
}
