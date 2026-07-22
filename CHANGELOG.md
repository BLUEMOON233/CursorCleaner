# Changelog

本项目的重要变更记录在此。版本号遵循 Semantic Versioning；预发布版本仍可能调整兼容性和本地数据处理策略。

## [0.1.0-alpha.1] - Unreleased

### Added

- macOS Apple Silicon、macOS Intel、Windows 10/11 x64 原生 TUI。
- 按工作目录组织的浏览、详情、搜索、过滤、多选、影响预览、确认、进度、回执与错误页面。
- 已知 Cursor schema 的只读检测，以及写入前进程、SQLite 锁、计划有效性和完整性检查。
- 事务化数据库清理与所选 transcript 的短期临时回滚数据。
- GitHub prerelease 构建、SHA-256 校验和、npm/npx 原生启动器与手动 npm 发布流程。

### Known limitations

- 仅支持当前已验证的 Cursor 数据库结构；未知 schema 会拒绝写入。
- 不支持 Linux、Windows ARM64、云端会话、独立归档写入、独立备份/恢复、整个工作目录注册或 workspaceStorage 清理。
- Windows 首版仍需真实机器覆盖 Windows Terminal、传统控制台、非 ASCII 用户目录及 Cursor 单库数据布局。
