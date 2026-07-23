# Release checklist

## 版本与仓库

- [ ] Cargo 与 npm 的 `repository`、`homepage`、`bugs` 信息指向目标公开仓库。
- [ ] `Cargo.toml`、`Cargo.lock`、`npm/package.json`、Changelog 与 Git tag 版本一致。
- [ ] 发布分支已提交并与 `origin` 同步，目标 tag 尚不存在。

## 隐私与安全

- [ ] 无真实 Cursor 数据库、transcript、诊断 JSON、日志、构建产物、私钥、token、`.env` 或私人绝对路径。
- [ ] 仅在合成数据和临时目录上运行写入测试；未对真实 Cursor 数据执行破坏性测试。
- [ ] GitHub Actions 的默认权限为 `contents: read`；仅创建 Release 的 job 使用 `contents: write`，npm job 另有 `id-token: write`。
- [ ] GitHub `npm` Environment 已配置 required reviewers，并限制部署分支。
- [ ] npm trusted publisher 指向正确的 GitHub 用户、仓库、工作流和 Environment；不依赖长期发布 token。

## 验证

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo build --locked --release -p cursor-cleaner`
- [ ] `node --test npm/test/cli.test.js`
- [ ] `npm pack --dry-run`（在 `npm/` 中）
- [ ] 三份 GitHub 原生压缩包及 npm tarball 均存在，`SHA256SUMS` 校验通过。
- [ ] macOS ARM64、macOS Intel、Windows x64 小范围实机测试完成；反馈已经人工脱敏。

## 发布与验证

- [ ] 推送 `main`。
- [ ] 创建并推送版本 tag，触发 GitHub Release。
- [ ] GitHub Release 的版本、预发布状态、四份发布文件和 `SHA256SUMS` 均正确。
- [ ] 手动运行 npm 发布工作流；稳定版发布到 `latest`，预发布版发布到 `next`。
- [ ] 使用全新 npm cache 验证 Registry 中的版本和 dist-tag。
