# CursorCleaner

在终端里浏览并清理 Cursor 本地对话记录。

CursorCleaner 会按工作目录整理对话，让你在真正删除前完成搜索、选择和影响确认。它面向 macOS 与 Windows，所有操作都在本机完成，不上传对话内容。

> [!NOTE]
> CursorCleaner 是非官方社区工具，与 Cursor 或 Anysphere 没有隶属、授权或背书关系。

> [!WARNING]
> 清理是永久操作，当前版本不提供独立备份或恢复功能。执行前请退出 Cursor，并认真核对影响预览。程序遇到未知数据库结构、Cursor 仍在运行或数据库被占用时会拒绝写入。

## 主要功能

- 按工作目录归类本地对话，快速查看记录和脱敏详情；
- 搜索标题、ID 与工作目录，按全部、未归档和已归档切换显示范围；
- 多选对话，在清理前预览数据库记录和 transcript 文件的影响范围；
- 默认取消最终确认，避免误触直接执行；
- 写入前检查 Cursor 进程、SQLite 锁、数据库结构和清理计划；
- 使用数据库事务执行清理，并在结束前校验结果；
- 支持中文、长路径、响应式终端和 `NO_COLOR`。

## 支持平台

| 系统 | 架构 |
| --- | --- |
| macOS | Apple Silicon（ARM64） |
| macOS | Intel（x64） |
| Windows 10/11 | x64 |

Linux、Windows ARM64 和移动平台暂不支持。

## 快速开始

确保已安装 Node.js 18 或更高版本以及 npm，然后运行：

```bash
npx --yes @bluemoon233/cursor-cleaner@latest
```

如需固定到当前稳定版本：

```bash
npx --yes @bluemoon233/cursor-cleaner@0.1.1
```

npm 包内已经包含对应平台的原生程序。启动器不会执行安装脚本，也不会在运行时下载额外二进制文件。

你也可以从 [GitHub Releases](https://github.com/BLUEMOON233/CursorCleaner/releases/latest) 下载原生压缩包并核对 `SHA256SUMS`，解压后直接在终端运行 `cursor-cleaner`（Windows 为 `cursor-cleaner.exe`）。

## 基本操作

| 按键 | 操作 |
| --- | --- |
| `↑` / `↓`、`j` / `k` | 移动或滚动 |
| `Enter` | 打开当前项或继续下一步 |
| `Space` | 选择或取消选择对话 |
| `/` | 搜索标题、ID 或工作目录 |
| `F` | 切换全部、未归档和已归档 |
| `X` / `Delete` | 进入清理安全流程，不会立即删除 |
| `Esc` | 返回 |
| `?` | 打开或关闭帮助 |
| `Q` | 退出 |

清理流程为：

```text
环境检查 → 生成计划 → 影响预览 → 默认取消确认 → 准备临时回滚数据 → 执行 → 校验 → 回执
```

## 安全与隐私

CursorCleaner 默认以只读方式打开数据库，只有在你完成选择和确认后才会尝试写入。写入期间会再次确认数据库结构和计划没有变化，并使用事务保证数据库修改要么完整提交，要么完整回滚。

程序不会：

- 上传对话、诊断结果或设备信息；
- 创建持久运行日志；
- 向 Cursor 的 `Application Support` 目录写入自身数据；
- 修改真实项目目录；
- 为整个数据库创建隐式副本。

执行清理时，程序可能在系统临时目录保存所选 transcript 的短期回滚数据。正常完成或成功回滚后会自动删除；只有自动恢复失败时才会保留，并在错误页面明确显示位置。

## 数据位置

未指定配置时，程序会自动查找 Cursor 的默认用户目录：

- macOS：`~/Library/Application Support/Cursor/User`
- Windows：`%APPDATA%\Cursor\User`

如需检查其他位置，可以指定配置文件：

```bash
npx --yes @bluemoon233/cursor-cleaner@latest --config /absolute/path/config.toml
```

配置示例见 [`apps/cursor-cleaner/config.example.toml`](apps/cursor-cleaner/config.example.toml)。

## 当前边界

CursorCleaner 只处理已经验证、且能可靠归属到所选对话的数据。以下功能目前不在支持范围内：

- 修改对话的归档状态；
- 删除整个工作目录注册或 `workspaceStorage`；
- 清理云端会话和共享缓存；
- 独立备份与手动恢复；
- 解释或修改未知版本的 Cursor 数据库结构。

更详细的数据结构和兼容性说明见 [`apps/cursor-cleaner/README.md`](apps/cursor-cleaner/README.md)。

## Windows 问题反馈

如果 Windows 版本无法识别本地数据，请先退出 Cursor，再在仓库目录运行脱敏诊断脚本：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-cursor-diagnostics.ps1
```

脚本只会在当前目录生成 `cursor-cleaner-windows-diagnostics.json`。提交 Issue 前仍请人工检查内容，不要上传对话正文、用户名、完整私人路径或其他敏感信息。

Windows 实机测试和允许反馈的字段见 [`docs/WINDOWS_TESTING.md`](docs/WINDOWS_TESTING.md)。安全问题请按照 [`SECURITY.md`](SECURITY.md) 私下报告。

## 从源码运行

需要 Rust stable 工具链：

```bash
cargo run --release -p cursor-cleaner
```

贡献指南见 [`CONTRIBUTING.md`](CONTRIBUTING.md)，版本变化见 [`CHANGELOG.md`](CHANGELOG.md)。

## License

[MIT](LICENSE)
