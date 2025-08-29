# steamos-mount

[English](README.md)

> [!WARNING] > **原型阶段 (PROTOTYPE STAGE)**
>
> 本项目目前处于原型阶段。**尚未**准备好投入生产使用。功能可能不完整、不稳定或发生重大变更。使用风险自负。

一个旨在解决 SteamOS 上挂载 NTFS/exFAT 磁盘并自动将其配置为 Steam 游戏库的工具。

## 文档

- [软件设计](docs/SOFTWARE_DESIGN_zh.md) (中文) | [Software Design](docs/SOFTWARE_DESIGN.md) (English)
- [技术规范](docs/TECH_SPEC_zh.md) (中文) | [Technical Specification](docs/TECH_SPEC.md) (English)

## 特性

- **人体工程学优先**：针对不同磁盘类型（SSD、SD 卡）提供简单的预设。
- **Steam 集成**：自动将新磁盘注入到 Steam 库中。
- **安全性**：优雅处理脏 NTFS 卷，防止数据损坏。

## 许可证

[MIT](LICENSE.md)

## 关于开发

本项目主要出于个人兴趣和探索 AI 辅助开发，会大量使用 AI 参与。
