//! steamos-mount-core: Core library for SteamOS drive mounting.
//!
//! This library provides the core functionality for mounting NTFS/exFAT
//! drives on SteamOS and integrating them with Steam's library system.
//!
//! # Modules
//!
//! - [`device`]: Unified device abstraction (primary API)
//! - [`disk`]: Disk scanning using `lsblk`
//! - [`preset`]: Mount preset definitions (SSD, Portable)
//! - [`fstab`]: Fstab parsing and writing
//! - [`mount`]: Mount/unmount operations and dirty volume handling
//! - [`steam`]: Steam VDF parsing and library injection
//! - [`syscall`]: Systemd control (daemon-reload, session switching)
//! - [`executor`]: Command execution with privilege escalation
//! - [`protocol`]: Daemon communication protocol (HMAC-SHA256)
//! - [`error`]: Error types
//!
//! # Example
//!
//! ```no_run
//! use steamos_mount_core::{disk, preset, fstab, mount, steam, syscall};
//!
//! // Scan for available devices
//! let devices = disk::list_block_devices().unwrap();
//! let mountable = disk::filter_mountable_devices(&devices);
//!
//! // Get the first NTFS device
//! if let Some(device) = mountable.first() {
//!     // Generate mount configuration
//!     let fs = preset::SupportedFilesystem::try_from("ntfs").unwrap();
//!     let config = preset::PresetConfig::new(fs);
//!     let options = config.generate_options(1000, 1000);
//!
//!     // Create fstab entry
//!     let mount_name = device.suggested_mount_name();
//!     let mount_point = fstab::generate_mount_point(&mount_name).unwrap();
//!     let entry = fstab::FstabEntry::new(
//!         device.fstab_spec().unwrap(),
//!         &mount_point,
//!         "ntfs3",
//!         &options,
//!         0,
//!         0,
//!     );
//!
//!     // This would update fstab (requires root):
//!     // let mut ctx = ExecutionContext::with_sudo();
//!     // let fstab_path = Path::new("/etc/fstab");
//!     // fstab::backup_fstab_with_ctx(fstab_path, &mut ctx).unwrap();
//!     // fstab::write_managed_entries_with_ctx(fstab_path, &[entry], &mut ctx).unwrap();
//! }
//! ```

pub mod device;
pub mod disk;
pub mod error;
pub mod executor;
pub mod fstab;
pub mod mount;
pub mod preset;
pub mod protocol;
pub mod steam;
pub mod syscall;

// Re-export commonly used types
pub use device::{
    Device, DeviceConnectionState, ListDevicesConfig, find_online_block_device_by_uuid,
    list_devices,
};
pub use disk::{
    BlockDevice, ManagedDevice, ManagedDevicesResult, OfflineDevice, normalize_fstype,
    vfs_type_to_fstype,
};
pub use error::{Error, Result};
pub use executor::{
    DaemonChild, DaemonSpawner, ExecutionContext, PrivilegeEscalation, PrivilegedSession,
    StdDaemonChild, StdDaemonSpawner,
};
pub use fstab::FstabEntry;
pub use preset::{MountPreset, PresetConfig};
pub use steam::LibraryFolder;
