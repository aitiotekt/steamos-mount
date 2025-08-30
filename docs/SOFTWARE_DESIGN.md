# steamos-mount Software Design Document

[中文 (Chinese)](SOFTWARE_DESIGN_zh.md)

This document details the design philosophy, technical architecture, and core implementation details of the `steamos-mount` tool. This project aims to solve the pain point for SteamOS users mounting NTFS/exFAT disks in Game Mode and recognizing them as Steam game libraries, focusing on **Ergonomics**, **Safety**, and **Automated Experience**.

## 1. Design Goals & Principles

- **Ergonomics First**: Hide complex Linux mount parameters through intelligent Presets, making configuration easy for non-technical users.
- **Backend First Architecture**: Decouple core logic from UI, supporting multiple frontends like TUI, GUI (Desktop), and Decky Plugin (Game Mode) via a unified Core Binary.
- **Safety & Stability**:
  - Core logic written in Rust to ensure memory safety.
  - Modifications to `/etc/fstab` must be Idempotent and include backup mechanisms.
  - Intelligently handle "Dirty Volume" states to prevent data corruption.
- **SteamOS Native Compatibility**: Adhere to SteamOS immutable filesystem characteristics, utilizing `/home` directory and Systemd mechanisms for persistence.

---

## 2. Software Architecture Design

Adopting a **Core-Client** separation architecture, where core logic sinks down and the UI layer is responsible only for display and interaction.

### 2.1 Technology Stack

- **Core Library + CLI**: `Rust`
  - **Responsibilities**: Disk scanning (`lsblk`), Fstab parsing & writing, `ntfsfix` repair, Steam VDF injection, Systemd control.
  - **Advantages**: High performance, type safety, easy cross-process invocation.
- **TUI (Terminal Interface)**: `Rust` + `Ratatui`
  - **Scenarios**: SSH remote management, Konsole, and terminal fallback within Decky plugins.
- **Game Mode GUI**: `Python` (Decky Backend) + `React` (Frontend)
  - **Scenarios**: Native experience in Game Mode. The plugin invokes `steamos-mount-cli` downloaded by `install.sh` (Decky CI lacks a Rust toolchain).
- **Desktop Mode GUI**: `Rust` + `Tauri`
  - **Scenarios**: Full configuration tool in Desktop Mode. UI components are maintained locally (not shared with Decky).

```plaintext
steamos-mount/
├── .git/
├── .gitignore
├── mise.toml                  # [Manager] Environment management
├── Cargo.toml                 # [Rust Workspace] Aggregates all scattered Cargo.toml
├── pnpm-workspace.yaml        # [JS Workspace] Aggregates all package.json
├── pyproject.toml             # [Python Workspace] Aggregates all scattered pyproject.toml
├── apps/                      # [Executable App Layer]
│   ├── cli/                   # Command-line interface
│   ├── tauri-app/             # Desktop GUI
│   │   ├── src-tauri/         # [Rust] Depends on packages/core
│   │   └── src/               # [React] Local UI components (not shared with Decky)
│   │
│   ├── decky-plugin/          # Game Mode Plugin
│   │   ├── src/               # [React] Local UI components
│   │   └── py_modules/        # [Python] Decky Python entry point
│   │
│   └── tui-app/               # [Rust] Pure Terminal App
│       └── src/               # Depends on packages/core
│
└── packages/                  # [Shared Library Layer]
    └── core/                  # [Rust] Core business logic (lsblk, fstab, mount)
```

### 2.2 Data Flow

1.  **Scan**: Core calls `lsblk` to get disk information in JSON format.
2.  **Present**: UI reads structured data returned by Core (including Label, UUID, FSTYPE, MOUNTPOINT).
3.  **Configure**: User selects disk and Preset in UI.
4.  **Execute**: Core receives instructions -> Backups fstab -> Writes fstab -> Reloads Systemd -> Mounts -> (Optional) Injects Steam Library -> Restarts UI.

### 2.3 Repository Notes (Development)

- **Main repo**: https://github.com/aitiotekt/steamos-mount
- **Decky subtree repo**: https://github.com/aitiotekt/steamos-mount-decky
- **Why subtree**: decky-plugin-database uses submodules that can only target repo roots, so the Decky plugin is synced as a git subtree.
- **UI components**: Tauri and Decky UIs do not share a UI kit due to Decky pipeline constraints.

---

## 3. Core Solution Details

### 3.1 Mount Point Strategy

- **Path Standard**: Unify under `/home/deck/Drives/<Label_or_UUID>`
  - **Why not `/mnt`**: System updates clear `/mnt`, causing mount point loss.
  - **Why not `/run/media`**: This is `udisks2` dynamic mount directory and is tmpfs (RAM disk), unable to achieve persistence with fstab, and prone to permission conflicts.
  - **Advantages**: `/home` partition is retained during SteamOS upgrades, and Flatpak apps have default access permissions.
  - **Conflict Handling**: If Label is empty or duplicate, automatically downgrade to use the first 8 characters of UUID as directory name.

### 3.2 Disk Identifiers

- **Recommended Strategy**: Provide both UUID and PARTUUID choices, with explanation in UI.
  - **UUID (Default Recommended)**: Bound to filesystem. Invalid after formatting, but persists if partition moves. Suitable for most users.
  - **PARTUUID**: Bound to partition table. Valid even after formatting filesystem (as long as partition isn't deleted). Suitable for advanced users.
  - **Note**: Must convert to **lowercase** when writing config to be compatible with `/dev/disk/by-uuid/` path lookup.

### 3.3 Filesystem Parameters & Presets

The tool should not ask users to write parameters manually, but provide scenario-based presets.

#### General Configuration

- `uid=1000,gid=1000`: Force mapping ownership to `deck` user, solving issue where NTFS/exFAT lack POSIX permissions preventing Steam from writing.
- `umask=000`: Grant 777 permissions to ensure Proton compatibility layer has no permission blockers.
- `nofail`: Do not block system boot if mount fails.

#### Preset A: Internal/Fixed SSD (Game High Performance Mode)

- **Scenario**: NVMe SSD or permanently connected SSD external drive.
- **Parameters**:
  - `ntfs3`: Use kernel driver, high performance.
  - `rw,noatime`: Reduce metadata writes.
  - `discard`: Enable TRIM, extend SSD life.
  - `prealloc`: (NTFS only) Pre-allocate space, reduce fragmentation, optimize large game downloads.
  - `x-systemd.device-timeout=3s`: Quickly skip non-existent devices.

#### Preset B: Hot-swappable SD Card/USB Drive (Portable Mode)

- **Scenario**: Devices requiring frequent plugging/unplugging.
- **Parameters**:
  - `noauto`: **Critical**. Do not mount at boot to avoid blocking.
  - `x-systemd.automount`: **Critical**. Trigger mount only when mount point is accessed (e.g., Steam scanning library).
  - `x-systemd.idle-timeout=60s`: (Optional) Automatically unmount after 60s of inactivity, reducing risk of "dirty volume" from direct removal.

### 3.4 "Dirty Volume" Handling

This is a key point for improved ergonomics. NTFS driver refuses to mount when Dirty Bit is detected (usually caused by Windows Fast Startup or forced shutdown).

- **Strategy**:
  1.  Core attempts mount -> Fails.
  2.  Core reads `dmesg` for keywords (`volume is dirty`, `force flag is not set`).
  3.  **UI Feedback**: Popup prompting user "Unsafe removal detected, attempt repair?".
  4.  **Repair**: Upon user confirmation, call `ntfsfix -d /dev/disk/by-uuid/<UUID>` to clear dirty flag.
  5.  **Retry**: Automatically retry mount after successful repair.
  6.  **Error Handling**: If repair fails, Core logs error, UI shows error message advising user to use `chkdsk` in Windows.
- **Warning**: Although `force` parameter can force mount, it highly risks data corruption. This project **strictly prohibits** default use of `force` and must resolve via repair flow.

---

## 4. Configuration Management (`/etc/fstab`)

Although `/etc` is a system directory, `fstab` is migrated and preserved in SteamOS.

### 4.1 Write Standard

Entries managed by the tool must contain special comment markers for program identification and updates, without destroying user-added entries.

```fstab
# /etc/fstab

# ... System original config ...

# BEGIN STEAMOS-AUTO-MOUNT-MANAGED
# Created by SteamOS Auto-Mount Tool. DO NOT EDIT THIS BLOCK MANUALLY.
UUID=1234-5678  /home/deck/Drives/GamesSSD  ntfs3  uid=1000,gid=1000,rw,umask=000,discard,prealloc,nofail  0  0
# END STEAMOS-AUTO-MOUNT-MANAGED
```

- **FSCK Option**: Set to `0 0`. Linux support for NTFS fsck is poor; enabling checks causes boot hang (dependency failed).

---

## 5. Steam Game Library Injection

After successful mount, Steam needs to recognize the path.

### 5.1 Automated Solution (Recommended)

Modify Steam's VDF configuration file via script.

- **File Path**: `~/.local/share/Steam/steamapps/libraryfolders.vdf`
- **Precondition**: Must **close Steam process** first, otherwise modifications are overwritten by Steam's in-memory cache.
- **Process**:
  1.  Core sends signal achieving effect similar to `killall -TERM steam`, kills steam process and waits for termination.
  2.  Parse VDF file, calculate next available `LibraryID` (Numeric string).
  3.  Check if truly killed (e.g., `pgrep -x steam`), inject new JSON block:
      ```json
      "{NEXTID}" {
          "path" "{MOUNT_PATH}"
          "label" ""
          "contentid" "0"
          "totalsize" "0"
          "apps" {}
      }
      ```
  4.  Restart UI service (`systemctl restart sddm` or `steamos-session-select gamescope`) to reload Steam.

### 5.2 Assisted Manual Solution (Desktop Mode)

If user prefers not to restart UI, provide guided manual addition:

1.  Core calls `steam steam://open/settings/storage` to open settings window.
2.  UI copies mount path to clipboard.
3.  User manually clicks `+` to add.

---

## 6. User Journey

1.  **Launch**: User opens Decky Plugin or Desktop App.
2.  **Overview**: See list of mounted and unmounted partitions (showing Label, Size, Type).
3.  **Configure**:
    - Select an NTFS partition.
    - Choose preset scheme (e.g., "External Game Drive").
    - (Advanced) Fine-tune mount parameters.
4.  **Apply**: Click "Mount and Add to Steam".
    - _System Check_: Is Dirty? -> Prompt Repair.
    - _Execute_: Write fstab -> mount.
    - _Inject_: Prompt "UI Restart Imminent" -> Inject VDF -> Restart.
5.  **Complete**: After restart, new disk appears directly in Steam storage space.
