# Release checklist

## 身份与仓库

- [ ] 用户已明确确认 GitHub 用户名/组织、目标仓库地址与 npm 包名；仓库内不再含发布占位符。
- [ ] Cargo 与 npm 的 `repository`、`homepage`、`bugs` 信息指向目标公开仓库。
- [ ] `Cargo.toml`、`npm/package.json`、Changelog 与 tag 均为 `0.1.0` / `v0.1.0`。
- [ ] 首次 commit 的作者信息正确，提交内容仅来自本独立仓库。

## 隐私与安全

- [ ] 无真实 Cursor 数据库、transcript、诊断 JSON、日志、构建产物、私钥、token、`.env` 或私人绝对路径。
- [ ] 仅在合成数据和临时目录上运行写入测试；未对真实 Cursor 数据执行破坏性测试。
- [ ] GitHub Actions 的默认权限为 `contents: read`；仅创建 Release 的 job 使用 `contents: write`，npm job 另有 `id-token: write`。
- [ ] GitHub `npm` Environment 已配置 required reviewers，并限制部署分支/tag。

## 验证

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo build --locked --release -p cursor-cleaner`
- [ ] `node --test npm/test/cli.test.js`
- [ ] `npm pack --dry-run`（在 `npm/` 中）
- [ ] 三份 GitHub 原生压缩包及 npm tarball 均存在，`SHA256SUMS` 校验通过。
- [ ] macOS ARM64、macOS Intel、Windows x64 小范围实机测试完成；反馈已经人工脱敏。

## 需用户明确授权的外部操作

- [ ] 创建第一次 Git commit。
- [ ] 创建 GitHub 远程仓库并添加 `origin`。
- [ ] 推送 `main`。
- [ ] 创建并推送 `v0.1.0` tag，触发 GitHub Release。
- [ ] 首次 npm 发布；发布后配置并验证 trusted publisher，再删除并吊销 bootstrap token。
