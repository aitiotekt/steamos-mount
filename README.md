# steamos-mount

[中文 (Chinese)](README_zh.md)

> [!WARNING] > **PROTOTYPE STAGE**
>
> This project is currently in the prototype stage. It is **NOT** ready for production use. Features may be incomplete, unstable, or subject to breaking changes. Use at your own risk.

A tool designed to ergonomically mount NTFS/exFAT drives on SteamOS and automatically configure them as Steam libraries.

## Documentation

- [Software Design](docs/SOFTWARE_DESIGN.md)
- [Technical Specification](docs/TECH_SPEC.md)

## Components

- **steamos-mount-core**: Core library for disk scanning, fstab management, and Steam library injection.
- **steamos-mount-cli**: Command-line interface for automation and scripting.
- **steamos-mount-tauri**: Desktop Mode app built with Tauri.
- **steamos-mount-decky**: Game Mode Decky plugin.
- **steamos-mount-tui**: Terminal interactive UI.

## Features

- **Ergonomics First**: Simple presets for different drive types (SSD, SD Card).
- **Steam Integration**: Automatically injects new drives into any Steam library.
- **Safety**: Handles dirty NTFS volumes gracefully and prevents data corruption.

## License

[MIT](LICENSE.md)

## About Development

This project is primarily driven by personal interest and the exploration of AI-assisted development. AI is heavily involved in the development process.
