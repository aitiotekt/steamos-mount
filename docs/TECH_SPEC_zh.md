# 核心设计

[English](TECH_SPEC.md)

# 方案

## 配置 `/etc/fstab`

该目录虽然是系统目录，但是 steamos 对于 `/etc` 目录下的配置会网开一面，认为是重要配置，所以大部分情况下可以跨升级保存

## 挂载点

### 不推荐使用 `/mnt/<mount-point>`

- 因为 `/mnt/<mount-point>` 是系统目录，steamos 的部分软件缺少访问权限
- steamos 系统更新后可能清空 `/mnt`，从而导致丢失挂载点

### 不推荐使用 `/run/media/deck/<mount-point>`

- 该挂载点是下 `udisk2` 的后台服务管理的挂载点，可能产生冲突
- 该挂载点是为了解决传统挂载方式（如 `/mnt` 或者 `/media`）在多用户环境和热拔插场景下的权限与清理问题
  - 使用 `/run` 目录，它是一个 Tmpfs（内存文件系统），在系统重启时会自动清空；`/media` 表示是可移动媒体；`/deck` 表示是在用户目录下，系统可以利用 ACL 把挂载点自动赋给当前登录用户，实现隔离
  - 由于是内存文件系统，在 `/etc/fstab` 的工作阶段，`/run/media/deck/<mount-point>` 不一定存在，无法挂载

### 挂载点位置必须存在一个文件夹

```bash
mkdir -p /home/deck/<mount-point>
```

## "File System"

- **使用 UUID or PARTUUID 而不是 /dev/nvme0n1pX**，使用 `blkid` 命令查找 UUID or PARTUUID。

  - UUID 被存储在文件系统中，PARTUUID 被存储在分区表中。PARTUUID 在重新格式化分区时会保留，UUID 在移动文件系统到另一个分区时会保留
  - /dev/nvme0n1pX 不是持久的，如果磁盘重新分区或移动，它会改变

- UUID 和 PARTUUID 必须使用小写，不然会无法挂载，因为是通过 `/dev/disk/by-uuid` 和 `/dev/disk/by-partuuid` 来查找的

## NTFS 和 exFAT 配置

- `ntfs3`:

  - 这里直接指定内核驱动类型，不再使用 ntfs-3g，ntfs3 支持内核级别的读写，有更高的性能，支持 TRIM（对 SSD 寿命和性能都很重要），支持稀疏文件。

- `uid=1000,gid=1000`:

  - 必须指定，因为 NTFS 不支持 Linux 的 POSIX 权限位，必须在挂载时将所有文件的所有者“伪装”成 Steam Deck 的默认用户 (deck 的 ID 是 1000)，否则 Steam 无法写入数据或启动游戏。

- `umask=000`:

  - 虽然 ntfs3 支持更细致的 ACL，但是在 SteamOS 的单用户游戏场景下，直接给 777 权限（umask=000）是最稳妥的，避免 Proton 兼容层因为没有某些权限而无法启动游戏

- `nofail`

  - 即使该挂载项不存在或无法挂载，系统也不会停止启动

- `x-systemd.device-timeout=3s`：

  - 推荐用于非可拔插设备如内置硬盘：开机检测挂载，如果 3s 内还没有挂载上，直接跳过不会卡在启动界面

- `x-systemd.automount` 和 `x-systemd.idle-timeout=60s`：

  - 推荐用于可拔插设备如 SD 卡
  - `x-systemd.automount` 表示尝试访问该文件系统时立即挂载它，毫秒级很快
  - `x-systemd.idle-timeout=60s` 表示如果 60s 内没有访问该文件系统，自动卸载它，方便热拔插

- `force`（危险）:

  - ntfs 磁盘被在强行拔掉硬盘或者强制关机后，会被标记为 Dirty
  - `ntfs` 出于安全考虑，默认**拒绝挂载**被标记为 Dirty 的分区，除非开启有一定风险的 `force` 选项。传统的 `ntfs-3g` 遇到 Dirty 会直接忽略。
  - 建议：不要开启该选项，而关闭 windows 的快速启动，`powercfg -h off`
  - 临时修复：`sudo ntfsfix /dev/disk/by-uuid/<UUID>` 或者 `sudo ntfsfix /dev/disk/by-partuuid/<PARTUUID>`

- `discard`：

  - SSD 的 TRIM 操作，帮助延长 SSD 的寿命

- `prealloc`（仅 NTFS）:

  - 预分配文件空间，避免文件写入时频繁分配空间，提高性能，对于 Steam 大型游戏很有帮助

**不推荐使用 FAT32**，因为 FAT32 不支持大文件，SteamOS 的游戏文件通常超过 4GB，FAT32 会报错。

## 添加游戏模式 “存储空间”

- Steamos 游戏模式的 “存储空间” 是通过 `~/.local/share/Steam/steamapps/libraryfolders.vdf` 文件来管理的。在添加挂载点并挂载后，需要编辑该文件，添加挂载点的路径。
- 该 vdf 文件在 Steam 运行时会**锁定并覆盖**此文件，必须**先关闭 Steam**。
- vdf 格式类似，对于新增的挂载点，`NEXTID` 自增，只需要设置 `MOUNT_PATH` 和 `MOUNT_LABEL` 即可，其它字段会自动修复

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

### 思路 1：命令行切换回桌面模式，调出 steam 界面引导用户添加游戏库，完成后命令行切换回游戏模式

```bash
# 到桌面模式
steamos-session-select plasma
# 打开游戏库界面
steam steam://open/settings/storage
# 回到游戏模式
steamos-session-select gamescope
```

### 思路 2：杀死 steam 进程，编辑 vdf 文件，添加挂载点的路径，完成后重新启动 steam

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
