//! steamos-mount-core: Core library for SteamOS drive mounting.
//!
//! This library provides the core functionality for mounting NTFS/exFAT
//! drives on SteamOS and integrating them with Steam's library system.
//!
//! # Modules
//!
//! - [`disk`]: Disk scanning using `lsblk`
//! - [`preset`]: Mount preset definitions (SSD, Portable)
//! - [`fstab`]: Fstab parsing and writing
//! - [`mount`]: Mount/unmount operations and dirty volume handling
//! - [`steam`]: Steam VDF parsing and library injection
//! - [`systemd`]: Systemd control (daemon-reload, session switching)
//! - [`error`]: Error types
//!
//! # Example
//!
//! ```no_run
//! use steamos_mount_core::{disk, preset, fstab, mount, steam, systemd};
//!
//! // Scan for available devices
//! let devices = disk::list_block_devices().unwrap();
//! let mountable = disk::filter_mountable_devices(&devices);
//!
//! // Get the first NTFS device
//! if let Some(device) = mountable.first() {
//!     // Generate mount configuration
//!     let config = preset::PresetConfig::new(preset::MountPreset::Ssd);
//!     let options = config.generate_options(device.fstype.as_deref().unwrap_or("ntfs"));
//!
//!     // Create fstab entry
//!     let mount_name = device.suggested_mount_name();
//!     let mount_point = fstab::generate_mount_point(&mount_name);
//!     let entry = fstab::FstabEntry::new(
//!         device.fstab_spec().unwrap(),
//!         &mount_point,
//!         "ntfs3",
//!         &options,
//!     );
//!
//!     // This would update fstab (requires root):
//!     // fstab::backup_fstab(Path::new("/etc/fstab")).unwrap();
//!     // fstab::write_managed_entries(Path::new("/etc/fstab"), &[entry]).unwrap();
//! }
//! ```

pub mod disk;
pub mod error;
pub mod fstab;
pub mod mount;
pub mod preset;
pub mod steam;
pub mod systemd;

// Re-export commonly used types
pub use disk::BlockDevice;
pub use error::{Error, Result};
pub use fstab::FstabEntry;
pub use preset::{MountPreset, PresetConfig};
pub use steam::LibraryFolder;
