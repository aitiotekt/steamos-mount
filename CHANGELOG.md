# Changelog

## [0.1.0]

### Added

#### Core & Backend
- **Intelligent Mounting**: Defaults to `~/Drives/<label>` for optimal SteamOS compatibility and user access. Supports custom mount point selection via the UI.
- **Fstab Management**: Safely reads and writes `/etc/fstab`. Introduces "Managed Entries" to persist mounts across reboots without cluttering system configuration.
- **Steam Integration**:
  - `libraryfolders.vdf` parsing and injection logic.
  - Capability to inject new library paths into Steam configuration.
- **Smart Privilege Handling**:
  - Uses `pkexec` strictly when required (e.g., for `mount` syscalls or fstab writes), minimizing root interaction.
  - **Privileged Session Mode**: Single-auth execution for multiple commands via a secure JSON-RPC daemon (HMAC-SHA256 signed).

#### UI/UX (Tauri App)
- **Device Management**:
  - Auto-refreshing list of available block devices.
  - Visual status for Mounted (Success), Dirty (Warning), and Not Mounted states.
  - **Repair** functionality for dirty volumes (e.g., ntfsfix).
- **Steam Deck optimizations**:
  - **Semi-Automatic Configuration**: "Configure Steam Library" button opens Steam's storage settings directly for easy addition of mounted drives.
  - **Library Detection**: Visual badges identifying drives that already contain a Steam Library.
  - **Confirmation Workflow**: Guided dialog to confirm Steam settings changes and auto-refresh device state.
- **Settings System**:
  - Global configuration for Steam VDF path.
  - Auto-detection helper for finding `libraryfolders.vdf`.
  - Persistent storage of user preferences.
- **Safety & Polish**:
  - **Unmount Protection**: "Unmount" button is disabled for devices not managed by this application to prevent accidental system modifications.
  - **Responsive Design**: Unified card layout with bottom-aligned actions for a consistent look.
  - **Visual Feedback**: Toast notifications for operations and detailed error reporting.
  - **Dark Mode**: Fully supported UI with adaptative colors.

#### Build
- **Arch Linux Support**:
  - Fix AppImage bundling on Arch Linux using experimental Tauri CLI branch (`feat/truly-portable-appimage`).
  - Add `just prepare-on-archlinux` helper to install build dependencies (`patchelf`, `squashfs-tools`, etc).
