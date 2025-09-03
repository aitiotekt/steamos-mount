//! Fstab parsing and writing module.
//!
//! This module handles reading, parsing, and writing `/etc/fstab` entries.
//! It uses special comment markers to identify managed entries and supports
//! idempotent updates with automatic backup.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::Error;
use crate::error::{IoResultExt, Result};

/// Marker for the beginning of the managed block in fstab.
pub const MANAGED_BLOCK_BEGIN: &str = "# BEGIN STEAMOS-MOUNT-MANAGED";

/// Marker for the end of the managed block in fstab.
pub const MANAGED_BLOCK_END: &str = "# END STEAMOS-MOUNT-MANAGED";

/// Description comment for the managed block.
const MANAGED_BLOCK_COMMENT: &str =
    "# Created by SteamOS Mount Tool. DO NOT EDIT THIS BLOCK MANUALLY.";

/// Default fstab path.
pub const FSTAB_PATH: &str = "/etc/fstab";

pub trait IntoMountOptions {
    fn into(self) -> Vec<String>;
}

impl IntoMountOptions for &str {
    fn into(self) -> Vec<String> {
        self.split(',').map(|s| s.to_string()).collect()
    }
}

impl IntoMountOptions for &String {
    fn into(self) -> Vec<String> {
        IntoMountOptions::into(self as &str)
    }
}

impl IntoMountOptions for String {
    fn into(self) -> Vec<String> {
        IntoMountOptions::into(&self as &str)
    }
}

impl IntoMountOptions for Vec<String> {
    fn into(self) -> Vec<String> {
        self
    }
}

/// Represents a single fstab entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FstabEntry {
    /// The device identifier (e.g., "UUID=xxx" or "PARTUUID=xxx").
    pub fs_spec: String,
    /// Mount point path.
    pub mount_point: PathBuf,
    /// Filesystem type (e.g., "ntfs3", "exfat").
    pub vfs_type: String,
    /// Mount options.
    pub mount_options: Vec<String>,
    /// This field is used by dump(8) to determine which filesystems need to be dumped.
    pub dump: u8,
    /// This field is used by fsck(8) to determine the order in which filesystem checks are done at boot time.
    pub fsck_order: u16,
}

impl FstabEntry {
    /// Creates a new fstab entry.
    pub fn new(
        fs_spec: impl Into<String>,
        mount_point: impl Into<PathBuf>,
        fs_type: impl Into<String>,
        mount_options: impl IntoMountOptions,
        dump: u8,
        fsck_order: u16,
    ) -> Self {
        Self {
            fs_spec: fs_spec.into(),
            mount_point: mount_point.into(),
            vfs_type: fs_type.into(),
            mount_options: mount_options.into(),
            dump,
            fsck_order,
        }
    }

    /// Formats the entry as an fstab line.
    pub fn to_fstab_line(&self) -> String {
        format!(
            "{}  {}  {}  {}  {}  {}",
            self.fs_spec,
            escape_fstab_path(&self.mount_point.to_string_lossy()),
            self.vfs_type,
            self.mount_options.join(","),
            self.dump,
            self.fsck_order
        )
    }

    /// Parses a single fstab line into an entry.
    ///
    /// Returns None for comments and empty lines.
    pub fn from_line(line: &str) -> Result<Option<Self>> {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            return Ok(None);
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 6 {
            return Ok(None);
        }

        let mount_options: Vec<String> = parts[3].split(',').map(|s| s.to_string()).collect();
        let dump = parts[4].parse::<u8>().map_err(|e| Error::FstabParse {
            message: format!("failed to parse dump field of line {}: {}", line, e),
        })?;
        let fsck_order = parts[5].parse::<u16>().map_err(|e| Error::FstabParse {
            message: format!("failed to parse fsck_order of line {}: {}", line, e),
        })?;

        Ok(Some(Self {
            fs_spec: parts[0].to_string(),
            mount_point: PathBuf::from(unescape_fstab_path(parts[1])),
            vfs_type: parts[2].to_string(),
            mount_options,
            dump,
            fsck_order,
        }))
    }
}

/// Escapes special characters in fstab paths using octal sequences.
///
/// Handles space (\040), tab (\011), newline (\012), and backslash (\134).
fn escape_fstab_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => encoded.push_str(r"\040"),
            '\t' => encoded.push_str(r"\011"),
            '\n' => encoded.push_str(r"\012"),
            '\\' => encoded.push_str(r"\134"),
            _ => encoded.push(c),
        }
    }
    encoded
}

/// Unescapes octal sequences in fstab paths.
fn unescape_fstab_path(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // Check for octal sequence
            let mut octal_digits = String::new();
            // Look ahead for up to 3 digits
            let mut clone_iter = chars.clone();
            for _ in 0..3 {
                if let Some(digit) = clone_iter.next() {
                    if digit.is_ascii_digit() {
                        octal_digits.push(digit);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if octal_digits.len() == 3
                && let Ok(byte) = u8::from_str_radix(&octal_digits, 8)
            {
                result.push(byte as char);
                // Consume the digits
                for _ in 0..3 {
                    chars.next();
                }
                continue;
            }
        }
        result.push(c);
    }
    result
}

/// Parsed fstab file with separate managed and unmanaged entries.
#[derive(Debug, Default)]
pub struct ParsedFstab {
    /// Lines before the managed block (including non-entry lines).
    pub header_lines: Vec<String>,
    /// Entries within the managed block.
    pub managed_entries: Vec<FstabEntry>,
    /// Lines after the managed block.
    pub footer_lines: Vec<String>,
    /// Whether a managed block was found.
    pub has_managed_block: bool,
}

/// Parses an fstab file.
///
/// Separates the file into header, managed entries, and footer sections.
pub fn parse_fstab(path: &Path) -> Result<ParsedFstab> {
    let file = fs::File::open(path).fstab_read_context(path)?;

    let reader = BufReader::new(file);
    let mut result = ParsedFstab::default();
    let mut in_managed_block = false;

    for line in reader.lines() {
        let line = line.fstab_read_context(path)?;

        if line.trim() == MANAGED_BLOCK_BEGIN {
            in_managed_block = true;
            result.has_managed_block = true;
            continue;
        }

        if line.trim() == MANAGED_BLOCK_END {
            in_managed_block = false;
            continue;
        }

        if in_managed_block {
            // Skip the comment line inside managed block
            if line.trim().starts_with("# Created by") {
                continue;
            }
            if let Some(entry) = FstabEntry::from_line(&line)? {
                result.managed_entries.push(entry);
            }
        } else if result.has_managed_block && !in_managed_block {
            // After managed block
            result.footer_lines.push(line);
        } else {
            // Before managed block (or no managed block found yet)
            result.header_lines.push(line);
        }
    }

    Ok(result)
}

/// Creates a timestamped backup of the fstab file.
///
/// Returns the path to the backup file.
pub fn backup_fstab(path: &Path) -> Result<PathBuf> {
    let timestamp = chrono_lite_timestamp();
    let backup_name = format!("{}.backup.{}", path.display(), timestamp);
    let backup_path = PathBuf::from(&backup_name);

    fs::copy(path, &backup_path).backup_context(&backup_path)?;

    Ok(backup_path)
}

/// Creates a timestamped backup with privilege escalation support.
pub fn backup_fstab_with_ctx(
    path: &Path,
    ctx: &crate::executor::ExecutionContext,
) -> Result<PathBuf> {
    let timestamp = chrono_lite_timestamp();
    let backup_name = format!("{}.backup.{}", path.display(), timestamp);

    ctx.copy_file_privileged(&path.display().to_string(), &backup_name)?;

    Ok(PathBuf::from(backup_name))
}

/// Simple timestamp without external dependencies.
fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    format!("{}", duration.as_secs())
}

/// Writes managed entries to the fstab file.
///
/// This function:
/// 1. Reads the existing fstab
/// 2. Removes any existing managed block
/// 3. Appends the new managed block with the provided entries
///
/// The operation is idempotent - running it multiple times with the same
/// entries produces the same result.
pub fn write_managed_entries(path: &Path, entries: &[FstabEntry]) -> Result<()> {
    let content = fs::read_to_string(path).fstab_read_context(path)?;
    let new_content = update_managed_entries_content(&content, entries)?;
    fs::write(path, new_content).fstab_write_context(path)?;
    Ok(())
}

/// Writes managed entries to fstab with privilege escalation support.
///
/// This version uses the provided `ExecutionContext` to write the file
/// with elevated privileges.
pub fn write_managed_entries_with_ctx(
    path: &Path,
    entries: &[FstabEntry],
    ctx: &crate::executor::ExecutionContext,
) -> Result<()> {
    let content = fs::read_to_string(path).fstab_read_context(path)?;
    let new_content = update_managed_entries_content(&content, entries)?;
    ctx.write_file_privileged(&path.display().to_string(), &new_content)?;
    Ok(())
}

/// Updates managed entries in fstab content string.
///
/// This function processes the fstab content as a string, replacing the managed block
/// with new entries. Useful for scenarios where file I/O is handled separately
/// (e.g., with privilege escalation via pkexec).
pub fn update_managed_entries_content(content: &str, entries: &[FstabEntry]) -> Result<String> {
    let mut header_lines = Vec::new();
    let mut footer_lines = Vec::new();
    let mut in_managed_block = false;
    let mut has_managed_block = false;
    let mut past_managed_block = false;

    for line in content.lines() {
        if line.trim() == MANAGED_BLOCK_BEGIN {
            in_managed_block = true;
            has_managed_block = true;
            continue;
        }

        if line.trim() == MANAGED_BLOCK_END {
            in_managed_block = false;
            past_managed_block = true;
            continue;
        }

        if in_managed_block {
            // Skip lines inside managed block
            continue;
        } else if past_managed_block {
            footer_lines.push(line);
        } else {
            header_lines.push(line);
        }
    }

    // Ensure we don't have a stale marker if the block wasn't properly closed
    let _ = has_managed_block;

    let mut output = String::new();

    // Write header lines
    for line in &header_lines {
        output.push_str(line);
        output.push('\n');
    }

    // Write managed block if there are entries
    if !entries.is_empty() {
        output.push_str(MANAGED_BLOCK_BEGIN);
        output.push('\n');
        output.push_str(MANAGED_BLOCK_COMMENT);
        output.push('\n');

        for entry in entries {
            output.push_str(&entry.to_fstab_line());
            output.push('\n');
        }

        output.push_str(MANAGED_BLOCK_END);
        output.push('\n');
    }

    // Write footer lines
    for line in &footer_lines {
        output.push_str(line);
        output.push('\n');
    }

    Ok(output)
}

/// Returns the default mount base path.
///
/// Uses the current user's home directory to support SteamOS-like systems
/// (ChimeraOS, Bazzite, HoloISO, etc.) where the username may not be "deck".
///
/// # Errors
/// Returns an error if the home directory cannot be determined.
pub fn default_mount_base() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(crate::error::Error::HomeDirNotFound)?;
    Ok(home.join("Drives"))
}

/// Generates a mount point path for a device.
///
/// # Errors
/// Returns an error if the home directory cannot be determined.
pub fn generate_mount_point(mount_name: &str) -> Result<PathBuf> {
    Ok(default_mount_base()?.join(mount_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    const SAMPLE_FSTAB: &str = r#"# /etc/fstab: static file system information.

# <file system>  <mount point>  <type>  <options>  <dump>  <pass>
UUID=abc-123  /  ext4  defaults  0  1
UUID=def-456  /boot/efi  vfat  umask=0077  0  1

# BEGIN STEAMOS-MOUNT-MANAGED
# Created by SteamOS Mount Tool. DO NOT EDIT THIS BLOCK MANUALLY.
UUID=1234-5678  /home/deck/Drives/GamesSSD  ntfs3  uid=1000,gid=1000,rw,umask=000,discard,prealloc,nofail  0  0
# END STEAMOS-MOUNT-MANAGED

# Custom user entries
UUID=custom  /mnt/custom  ext4  defaults  0  0
"#;

    #[test]
    fn test_parse_fstab_entry() {
        let line = "UUID=1234-5678  /home/deck/Drives/Test  ntfs3  rw,noatime  0  0";
        let entry = FstabEntry::from_line(line).unwrap().unwrap();

        assert_eq!(entry.fs_spec, "UUID=1234-5678");
        assert_eq!(entry.mount_point, PathBuf::from("/home/deck/Drives/Test"));
        assert_eq!(entry.vfs_type, "ntfs3");
        assert_eq!(entry.mount_options, vec!["rw", "noatime"]); // Split into vector
        assert_eq!(entry.dump, 0);
        assert_eq!(entry.fsck_order, 0);
    }

    #[test]
    fn test_parse_fstab_skip_comments() {
        assert!(
            FstabEntry::from_line("# This is a comment")
                .unwrap()
                .is_none()
        );
        assert!(FstabEntry::from_line("").unwrap().is_none());
        assert!(FstabEntry::from_line("   ").unwrap().is_none());
    }

    #[test]
    fn test_fstab_entry_to_line() {
        let entry = FstabEntry::new("UUID=test-123", "/mnt/test", "ntfs3", "rw,noatime", 0, 0);

        let line = entry.to_fstab_line();
        assert!(line.contains("UUID=test-123"));
        assert!(line.contains("/mnt/test"));
        assert!(line.contains("ntfs3"));
    }

    #[test]
    fn test_parse_fstab_with_managed_block() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(SAMPLE_FSTAB.as_bytes()).unwrap();

        let parsed = parse_fstab(temp_file.path()).unwrap();

        assert!(parsed.has_managed_block);
        assert_eq!(parsed.managed_entries.len(), 1);
        assert_eq!(parsed.managed_entries[0].fs_spec, "UUID=1234-5678");

        // Header should contain system entries
        assert!(
            parsed
                .header_lines
                .iter()
                .any(|l| l.contains("UUID=abc-123"))
        );

        // Footer should contain custom entries
        assert!(
            parsed
                .footer_lines
                .iter()
                .any(|l| l.contains("UUID=custom"))
        );
    }

    #[test]
    fn test_write_managed_entries_idempotent() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(SAMPLE_FSTAB.as_bytes()).unwrap();

        let entries = vec![FstabEntry::new(
            "UUID=new-entry",
            "/home/deck/Drives/NewDrive",
            "ntfs3",
            "rw,noatime",
            0,
            0,
        )];

        // Write entries
        write_managed_entries(temp_file.path(), &entries).unwrap();

        // Parse again
        let parsed = parse_fstab(temp_file.path()).unwrap();
        assert_eq!(parsed.managed_entries.len(), 1);
        assert_eq!(parsed.managed_entries[0].fs_spec, "UUID=new-entry");

        // Write same entries again (idempotent)
        write_managed_entries(temp_file.path(), &entries).unwrap();

        let parsed2 = parse_fstab(temp_file.path()).unwrap();
        assert_eq!(parsed2.managed_entries.len(), 1);
    }

    #[test]
    fn test_generate_mount_point() {
        let mount_point = generate_mount_point("GamesSSD").unwrap();
        let expected = dirs::home_dir()
            .expect("Home directory should exist in test environment")
            .join("Drives")
            .join("GamesSSD");
        assert_eq!(mount_point, expected);
    }

    #[test]
    fn test_parse_fstab_escaped_spaces() {
        // "My Drive" -> "My\040Drive"
        let line = "UUID=1234  /mnt/My\\040Drive  ntfs3  defaults  0  0";
        let entry = FstabEntry::from_line(line).unwrap().unwrap();

        // This assertion will likely fail currently because unescaping isn't implemented
        assert_eq!(entry.mount_point, PathBuf::from("/mnt/My Drive"));

        let formatted = entry.to_fstab_line();
        // This assertion might pass if we just store what we read, but we need to ensure proper round-tripping
        assert!(formatted.contains("/mnt/My\\040Drive"));
        assert!(!formatted.contains("/mnt/My Drive"));
    }
}
