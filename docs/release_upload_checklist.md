# 发布上传要求（Release 上传清单）

这个清单用于以后在 GitHub 上发布/上传 `octo` 时的固定流程，避免每次靠经验重复决策。`octocode` 只作为兼容入口保留。

## 1. 版本与标签约定

1. 使用语义化版本号，tag 形如 `vX.Y.Z`（例如 `v0.1.0`）。
2. 发布前先更新版本号：
   - `Cargo.toml` 的 `version`
   - `Cargo.lock`（`cargo update`/`cargo generate-lockfile` 后提交）
3. 确认 `main` 分支处于干净状态并已通过本地 CI 要求检查。

## 2. 发布前自检（建议）

1. `cargo fmt --all --check`
2. `cargo check --all-targets --all-features`
3. `cargo clippy --all-targets --all-features -- -D warnings`
4. `cargo test --all-features`
5. 关键 CLI 与 TUI 启动 smoke：
   - `cargo run -- --help`
   - `cargo run --bin octo -- --help`
   - `cargo run --bin octo -- task --help`
   - `cargo run --bin octo -- preview-tui --api ready --scenario welcome --width 80 --height 24`
   - `cargo run --bin octo -- preview-tui --api ready --scenario workbench --width 80 --height 24`
   - `cargo run --bin octo -- preview-tui --api ready --scenario diff --width 100 --height 28`
   - `cargo run --bin octo -- preview-tui --api ready --scenario approval --width 100 --height 28`
6. npm 包预检：
   - `node scripts/npm-bootstrap.js --dry-run`
   - `npm pack --dry-run`

## 3. 发布执行流程（GitHub tag）

1. 更新版本号并提交：
   - `cargo update` 或 `cargo generate-lockfile`
   - `git add Cargo.toml Cargo.lock`
   - 更新 [CHANGELOG.md](../CHANGELOG.md) 的 `[Unreleased]` 内容
   - 同步更新 `package.json` 的 `version`（如使用 npm 分发）
   - `git commit -m "chore(release): vX.Y.Z"`
2. 打 tag 并推送：
   - `git tag vX.Y.Z`
   - `git push origin vX.Y.Z`
3. 等待 tag-trigger 的 GitHub Actions 运行完成。
4. 在 Release 页面核对四个平台 assets：
   - `octo-vX.Y.Z-<target>.tar.gz` / `.zip`
   - `checksums-<target>.txt`
5. 抽检一个校验文件里的 SHA256 是否与下载文件对应。

### npm 分发（可选）

1. 本仓库提供 `package.json` 作为 npm 包名为 `octo` 的入口。
2. npm 包默认通过 GitHub Release 按版本下载对应平台二进制到 `~/.octo/<version>/<target>`。
3. 推荐发布顺序：
   - 先完成 GitHub tag 发布（生成二进制）
   - 再在相同版本打 `npm publish`
4. 若 Release 版本未发布，npm 安装会提示缺少对应二进制资源。

## 4. 本地发布脚本

1. 执行一键脚本（本地检查 + 版本 bump + tag + 推送）：

```powershell
.\scripts\release.ps1 -Version 0.1.1
```

2. 仅做检查，不提交与推送（用于预演）：

```powershell
.\scripts\release.ps1 -Version 0.1.1 -DryRun
```

3. Linux/macOS 用户可直接使用：

```bash
./scripts/release.sh 0.1.1
```

4. 仅做检查预演（Unix）：

```bash
./scripts/release.sh 0.1.1 --dry-run
```

`npm` 侧可以先本地预览：

```bash
npm pack --dry-run
npm install -g .
npm install -g octo
octo --help
```

## 5. 发布资源要求

1. GitHub Release 使用 tag 触发自动打包。
2. 每个平台需上传：
   - `octo-vX.Y.Z-<target>.tar.gz`（Linux/macOS）
   - `octo-vX.Y.Z-<target>.zip`（Windows）
   - `checksums-<target>.txt`
3. Archive 中必须包含：
   - `octo`
   - `octocode`（兼容入口，可选；npm bootstrap 只要求 `octo`）
   - 说明文件（当前为 `README.md`）
4. `checksums-<target>.txt` 必须包含每个发布文件的 SHA256。

## 6. 变更记录要求

1. 发布页面优先使用自动生成 release notes。
2. `CHANGELOG.md` 需要同步记录：
   - 新增特性
   - 修复项
   - 兼容性/Breaking changes
   - 依赖或安全说明（如有）

## 7. 后续分发（选做）

1. `crates.io`：保持 `cargo publish` 流程可复用。
2. Homebrew/Scoop：后续可接入对应仓库的自动更新清单。
3. 包含 npm 包时，建议作为脚本包装器（下载 release 二进制）而不是直接替代 rust 二进制。

## 8. 回滚规则

1. 若上传失误导致文件损坏，先撤回并重发 Release。
2. 若是功能问题，先发 hotfix tag，待验证后再做下一个正式 tag。
3. 遇到签名/校验问题，先修复文档并补发同版本修订 patch。
