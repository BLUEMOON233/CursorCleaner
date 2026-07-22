# Contributing

感谢参与 CursorCleaner。

## 开发要求

- Rust stable；
- Node.js 20 或更高版本（仅 npm 启动器与打包）；
- macOS 或 Windows 10/11 x64。

提交前运行：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p cursor-cleaner
node --test npm/test/cli.test.js
```

## 数据安全

- 不得在真实 Cursor 数据上运行写入测试；
- 测试必须使用临时目录、合成数据库或明确复制的数据库副本；
- Issue、测试夹具和日志不得包含对话正文、密钥或完整私人路径；
- 新 schema 必须先实现只读检测和兼容性测试，再考虑写入支持；
- 渲染代码不得执行数据库或文件 I/O。

提交 Pull Request 时请说明平台、验证命令、数据格式变化与潜在风险。
