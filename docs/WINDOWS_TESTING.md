# Windows x64 实机测试清单

只使用非关键账户、可丢弃测试数据或明确制作的数据副本。不要把真实数据库或 transcript 发送给维护者；执行永久清理前退出 Cursor。

## 环境覆盖

- Windows 10 x64 与 Windows 11 x64；Windows Terminal 和传统 `conhost.exe` 各至少一次。
- PowerShell 5.1 或 PowerShell 7；常见缩放比例，窗口缩放及窄终端。
- ASCII 与非 ASCII Windows 用户名/工作目录；含空格、CJK 和长路径的测试目录。
- Cursor 标准版；若使用 Insiders 或自定义/便携目录，仅通过显式配置副本测试。

## 只读流程

- 校验程序能启动、退出后终端状态和中文显示正常。
- 记录 Windows SmartScreen 或安全软件是否警告、阻止或隔离程序；不要关闭系统级安全防护来绕过提示。
- Cursor 运行时确认 preflight 明确阻止清理。
- 浏览工作目录分组、详情、搜索、归档过滤、多选和取消选择。
- 核对未知 schema、缺失数据库、数据库不可读等情况只显示诊断，不允许写入。
- 核对默认确认选项为取消，影响预览中的对话数、状态行数、transcript 目录数与大小合理。

## 清理流程

- 仅选择专门创建且可丢弃的测试对话；先保留界面截图或人工计数，不复制真实内容。
- 测试取消确认后数据不变；确认后观察进度、回执和再次启动后的记录数量。
- 检查非目标测试对话、真实项目目录与 `workspaceStorage` 均未改变。
- 正常完成后确认 `%TEMP%\cursor-cleaner-recovery` 不存在或为空，且未生成持久日志或 Application Support 数据。
- 可在数据库副本上持有 SQLite 写锁，确认程序拒绝执行且 transcript 仍存在；不要在真实数据库上做锁/故障注入。

## 允许反馈的脱敏信息

请反馈版本/tag、Windows 版本与架构、Cursor 版本、终端/PowerShell 类型、安装方式、执行阶段、预期与实际行为、退出码，以及人工脱敏后的错误文字。可附诊断脚本生成的 JSON，但分享前必须再次人工检查。

不得反馈：数据库或 WAL/SHM 文件、对话标题/正文、conversation ID、transcript、密钥/token、Windows 用户名、完整用户目录或工作目录路径。路径统一改写成 `%USERPROFILE%\<redacted>`、`<drive>:\<redacted>`；截图应遮盖标题、路径、用户名和其他应用内容。

运行脱敏诊断：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-cursor-diagnostics.ps1
```

建议反馈模板：

```text
CursorCleaner: v0.1.0
Windows / arch:
Cursor version:
Terminal / PowerShell:
Install method:
Stage: launch | preflight | browse | plan | confirm | execute | receipt
Expected:
Actual:
Exit code:
Redacted error/diagnostics:
```
