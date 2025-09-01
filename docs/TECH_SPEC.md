# Core Design

[中文 (Chinese)](TECH_SPEC_zh.md)

# Solution

## Configuring `/etc/fstab`

Although this is a system directory, SteamOS treats configuration under `/etc` leniently, considering it important configuration, so in most cases it persists across upgrades.

## Mount Points

### Not Recommended: `/mnt/<mount-point>`

- Because `/mnt/<mount-point>` is a system directory, some software in SteamOS lacks access permissions.
- SteamOS system updates might clear `/mnt`, leading to loss of mount points.

### Not Recommended: `/run/media/deck/<mount-point>`

- This mount point is managed by the `udisks2` background service and may cause conflicts.
- This mount point is designed to solve permissions and cleanup issues in multi-user environments and hot-plug scenarios (like `/mnt` or `/media`).
  - Using `/run` directory means it's a Tmpfs (RAM file system), automatically cleared on reboot; `/media` indicates removable media; `/deck` indicates user directory, system can use ACL to automatically assign mount point to current logged-in user for isolation.
  - Since it is a RAM file system, `/run/media/deck/<mount-point>` may not exist during `/etc/fstab` processing stage, causing mount failure.

### Mount Point Directory Must Exist

```bash
mkdir -p /home/deck/<mount-point>
```

## "File System"

- **Use UUID or PARTUUID instead of /dev/nvme0n1pX**, use `blkid` command to find UUID or PARTUUID.

  - UUID is stored in the filesystem, PARTUUID is stored in the partition table. PARTUUID persists when partition is reformatted, UUID persists when filesystem is moved to another partition.
  - /dev/nvme0n1pX is not persistent; if disk is repartitioned or moved, it changes.

- UUID and PARTUUID must be lowercase, otherwise mount will fail because lookup is done via `/dev/disk/by-uuid` and `/dev/disk/by-partuuid`.

## NTFS and exFAT Configuration

- `ntfs3`:

  - Directly specify kernel driver type. Stop using ntfs-3g. ntfs3 supports kernel-level read/write, higher performance, supports TRIM (important for SSD life and performance), and sparse files.

- `uid=1000,gid=1000`:

  - Must be specified because NTFS does not support Linux POSIX permission bits. Must "masquerade" all file ownership as Steam Deck default user (deck ID is 1000) at mount time, otherwise Steam cannot write data or launch games.

- `umask=000`:

  - Although ntfs3 supports finer ACLs, in SteamOS single-user gaming scenario, giving 777 permissions (umask=000) is safest to avoid Proton compatibility layer failing to start games due to missing permissions.

- `nofail`

  - Even if the mount entry does not exist or fails to mount, system boot will not stop.

- `x-systemd.device-timeout=3s`:

  - Recommended for non-removable devices like internal drives: check mount at boot, if not mounted within 3s, skip directly to avoid sticking at boot screen.

- `x-systemd.automount` and `x-systemd.idle-timeout=60s`:

  - Recommended for removable devices like SD cards.
  - `x-systemd.automount` means mount immediately when attempting to access the filesystem, milliseconds fast.
  - `x-systemd.idle-timeout=60s` means automatically unmount if filesystem is not accessed for 60s, facilitating hot-plugging.

- `force` (Dangerous):

  - ntfs disks are marked Dirty after forced unplug or forced shutdown.
  - `ntfs` defaults to **refusing mount** for Dirty partitions for safety, unless risky `force` option is enabled. Traditional `ntfs-3g` ignored Dirty.
  - Recommendation: Do not enable this. Turn off Windows Fast Startup `powercfg -h off`.
  - Temporary Fix: `sudo ntfsfix /dev/disk/by-uuid/<UUID>` or `sudo ntfsfix /dev/disk/by-partuuid/<PARTUUID>`.

- `discard`:

  - SSD TRIM operation, helps extend SSD life.

- `prealloc` (NTFS only):

  - Pre-allocate file space to avoid frequent allocation during write, improving performance, helpful for large Steam games.

- `noatime`:

  - `noatime` is a common mount option, used to disable access time updates, reducing write operations, improving performance, especially suitable for SSDs.

**FAT32 Not Recommended**, because FAT32 does not support large files, and SteamOS game files often exceed 4GB, causing errors.

## Adding Game Mode "Storage Space"

- SteamOS Game Mode "Storage Space" is managed via `~/.local/share/Steam/steamapps/libraryfolders.vdf`. After adding mount point and mounting, need to edit this file to add mount point path.
- This vdf file is **locked and overwritten** by Steam at runtime, must **close Steam first**.
- VDF format is similar. For new mount points, `NEXTID` auto-increments, only need to set `MOUNT_PATH` and `MOUNT_LABEL`, other fields auto-repair.

```~/.local/share/Steam/steamapps/libraryfolders.vdf
"{NEXTID}"
    {{
        "path"      "{MOUNT_PATH}"
        "label"     "{MOUNT_LABEL}"
        "contentid" "0"
        "totalsize" "0"
        "apps"      {{}}
    }}
```

### Approach 1: Command line switch to Desktop Mode, invoke Steam interface to guide user to add library, then switch back to Game Mode

```bash
# To Desktop Mode
steamos-session-select plasma
# Open Library Interface
steam steam://open/settings/storage
# Back to Game Mode
steamos-session-select gamescope
```

### Approach 2: Kill Steam process, edit VDF file, add mount point path, then restart Steam

```bash
steam --shutdown
cat <<EOF > ~/.local/share/Steam/steamapps/libraryfolders.vdf
"${NEXTID}"
    {{
        "path"      "${MOUNT_PATH}"
        "label"     "${MOUNT_LABEL}"
        "contentid" "0"
        "totalsize" "0"
        "apps"      {{}}
    }}
EOF
steam
```
