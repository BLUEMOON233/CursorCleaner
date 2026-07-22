# CursorCleaner

`cursor-cleaner` 是一个面向 macOS 与 Windows 10/11 x64 的 Cursor 本地对话管理 TUI。它按工作目录组织对话，支持只读浏览、搜索、过滤、多选、影响预览和经严格确认的批量清理。

这是非官方社区工具，与 Cursor 或 Anysphere 没有隶属、授权或背书关系。

> [!WARNING]
> 这是会修改 Cursor 本地数据的早期测试版本。请先退出 Cursor，并先在非关键账户或数据副本上验证。程序仅支持已识别的数据库结构，遇到未知结构会拒绝写入。

## 功能

- 自动发现 Cursor 数据源并检测 SQLite schema；
- 按工作目录分组，浏览记录与脱敏详情；
- 搜索、归档状态过滤、多选和批量计划；
- 数据库事务、计划有效性复验、并发写入防护与结果校验；
- CJK、长路径、响应式终端、`NO_COLOR` 和异常终端恢复；
- macOS Apple Silicon、macOS Intel 与 Windows x64 原生程序。

程序不会创建持久日志、独立备份或 Application Support 数据。清理时只可能在系统临时目录创建所选 transcript 的短期回滚副本；成功或成功回滚后自动移除，仅在自动恢复失败时保留并明确报告路径。

## 运行

从源码运行：

```bash
cargo run --release -p cursor-cleaner
```

指定配置文件：

```bash
cursor-cleaner --config /absolute/path/config.toml
```

默认路径：

- macOS：`~/Library/Application Support/Cursor/User`
- Windows：`%APPDATA%\\Cursor\\User`

配置示例位于 [`apps/cursor-cleaner/config.example.toml`](apps/cursor-cleaner/config.example.toml)。完整数据结构与限制见 [`apps/cursor-cleaner/README.md`](apps/cursor-cleaner/README.md)。

Windows 兼容性问题可在退出 Cursor 后运行隐私脱敏诊断脚本：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-cursor-diagnostics.ps1
```

脚本只在调用位置生成 `cursor-cleaner-windows-diagnostics.json`，分享前仍应人工检查内容。
Windows 首版实机测试步骤与允许反馈的脱敏字段见 [`docs/WINDOWS_TESTING.md`](docs/WINDOWS_TESTING.md)。

## npx 分发

发布 npm 包后，用户可以一行启动：

```bash
npx --yes @bluemoon233/cursor-cleaner@0.1.0-alpha.2
```

npm 包只是薄启动器：根据 `process.platform` 和 `process.arch` 运行包内原生二进制，不执行 `postinstall`、不联网下载，也不写入 Application Support。发布前请按 [`npm/README.md`](npm/README.md) 设置你实际拥有的 npm 包名。

## 开发与验证

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p cursor-cleaner
```

所有写入测试只使用临时目录和 SQLite 副本，禁止针对真实 Cursor 数据执行破坏性测试。

## 发布

1. 将 `Cargo.toml`、`npm/package.json` 与 Git tag 版本保持一致。
2. 确认 npm 包名为 `@bluemoon233/cursor-cleaner`，并在 GitHub 仓库变量 `NPM_PACKAGE_NAME` 中使用相同值。
3. 推送形如 `v0.1.0-alpha.2` 的 tag；Release 工作流会构建三个原生压缩包、校验和与 npm tarball 草稿产物。
4. 小范围验证 GitHub prerelease 后，再手动运行 npm 发布工作流。

完整的首发门禁和人工确认项见 [`RELEASE_CHECKLIST.md`](RELEASE_CHECKLIST.md)，版本变更见 [`CHANGELOG.md`](CHANGELOG.md)。

安全问题请按 [`SECURITY.md`](SECURITY.md) 私下报告。贡献方式见 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

## License

MIT
