# npm launcher

这个目录用于组装携带三个原生二进制的 npm 包。启动器不下载文件、不运行安装脚本，只把终端和参数交给对应平台的 `cursor-cleaner`。

## 发布前设置

1. 确认 `package.json` 中的包名为 `@bluemoon233/cursor-cleaner`，且该 npm scope 归发布账号所有。
2. 同步 Cargo、npm 与 Git tag 的版本。
3. 运行 `node --test test/cli.test.js` 和 `npm pack --dry-run`。
4. 在 GitHub 仓库变量 `NPM_PACKAGE_NAME` 中设置同一个完整包名。
5. 配置名为 `npm` 的 GitHub Environment，并添加 required reviewers。首次发布因包尚不存在，需在该 Environment 中临时添加 `NPM_TOKEN` secret；成功后在 npm 包设置中绑定 trusted publisher（工作流文件名 `publish-npm.yml`、Environment `npm`），验证 OIDC 发布后删除并吊销 token。
6. trusted publishing 需要 Node.js 22.14.0+ 与 npm CLI 11.5.1+；工作流使用 Node.js 24 并在发布前显式校验。

配置字段和版本要求以 [npm trusted publishing 官方文档](https://docs.npmjs.com/trusted-publishers/) 为准。

Release 工作流会把三个平台产物放入 `vendor/<platform-arch>/`，生成可测试的 npm tarball。npm 发布工作流只允许手动运行，并验证版本、tag 和包名一致。
