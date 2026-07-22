# cursor-cleaner

`cursor-cleaner` 是面向 macOS 与 Windows 10/11 x64 的 Cursor 本地对话管理 TUI。它默认只读访问数据，写操作只支持已验证的本地结构：

- 可选的 `conversation-search.db`：`user_version=7`，包含 `conversations`、`conversation_fts`、`conversation_search_candidates`；
- `state.vscdb`：`user_version=1`，包含 `ItemTable`、`cursorDiskKV`、`composerHeaders`；
- `~/.cursor/projects/*/agent-transcripts/<conversation-id>/`。

macOS 通常使用搜索库与状态库双库模式；已验证的 Windows 数据可能只有 `state.vscdb`，此时程序从 `composerHeaders` 和 `cursorDiskKV` 加载记录并执行单库事务。未知 schema 只允许诊断。程序不会写入真实项目目录。

## 运行

```bash
cargo run --release -p cursor-cleaner
```

可选配置：

```bash
cursor-cleaner --config /absolute/path/config.toml
```

配置字段见 `config.example.toml`。未提供配置时自动使用当前平台的 Cursor 默认路径：macOS 为 `~/Library/Application Support/Cursor/User`，Windows 为 `%APPDATA%\Cursor\User`。

## 安全流程

永久清理严格执行：环境检查 → 计划 → 影响预览 → 默认取消确认 → 所选 transcript 临时回滚副本 → 清理 → SQLite 事务 → 完整性/数量校验 → 清理临时数据 → 回执。数据库不生成整库副本，失败时依靠事务回滚。

执行前若不能可靠确认 Cursor 已退出、数据库未被其他进程打开、schema 仍受支持或计划仍有效，程序会拒绝写入。macOS 使用进程与文件占用检查；Windows 使用 `tasklist.exe` 进程检查和 SQLite 即时写锁探测。日志和错误不会包含完整对话正文；当前版本不生成持久运行日志。

程序不提供独立备份或恢复功能。所选 transcript 的临时回滚数据位于操作系统临时目录（macOS 默认 `$TMPDIR/cursor-cleaner-recovery`，Windows 默认 `%TEMP%\cursor-cleaner-recovery`），只用于当前执行失败时自动恢复；不会复制整个数据库。正常完成或成功回滚后会连同空目录一起删除，仅当 transcript 自动恢复本身失败时才会保留并在错误页报告位置。

## 当前边界

已实现按工作目录分组浏览、目录内对话列表、详情预览、搜索、归档状态过滤、多选和批量永久清理。工作目录优先使用对话正文和 Composer Header 的明确路径，其次使用 workspaceStorage、标签页状态、Git 仓库和 transcript 历史线索；无法可靠还原的记录保留在特殊分组中。尚未迁移“删除整个工作目录注册与 workspaceStorage”、独立归档写入、云端会话、共享缓存清理；这些对象缺少足够稳定的归属和跨版本证据。
