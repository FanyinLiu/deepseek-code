# Octo

Octo 是面向多模型、多智能体和本地工具编排的终端编程代理。项目名保留 Octocode，日常命令以 `octo` 为主；`octocode` 仅作为兼容入口。

## 安装

```bash
# 从源码目录安装日常命令 octo
cd octocode
cargo install --path . --bin octo --force

# 也可直接用 npm 安装（需要联网并会拉取发布的二进制）
npm install -g octo
# 首次执行时会尝试从 GitHub Release 下载匹配版本的二进制。
# 如果仓库还没发布该版本，命令会提示先用 cargo 或本地 release 产物先补齐。

# 不安装也可以直接运行
cargo run -- tui

# 构建两个二进制：octo + octocode 兼容入口
cargo build --release
```

## 发布上传要求

发布前请按 [发布上传清单](docs/release_upload_checklist.md) 执行。当前发布流程会在打 tag 时自动输出：
- Linux/macOS：`octo-vX.Y.Z-<target>.tar.gz`
- Windows：`octo-vX.Y.Z-<target>.zip`
- 每个 asset 对应的 `.sha256` 文件

本地也支持一键发布脚本：

```powershell
.\scripts\release.ps1 -Version 0.1.1
```

```bash
./scripts/release.sh 0.1.1
```

默认主版本号可加 `v` 前缀，也可不加；脚本会自动统一处理为 `vX.Y.Z` tag。

## 快速开始

```bash
# 登录，也可以首次打开 TUI 后直接粘贴 API key
octo login --api-key sk-xxx

# 首次本地就绪检查（不联网、不写配置）
octo onboard

# 诊断
octo doctor

# 对话
octo chat "解释这个项目"

# 提问（含项目搜索上下文）
octo ask "auth middleware 在哪里"

# 搜索代码
octo search "orchestrator"

# 计划模式（只读分析）
octo plan "修复登录失败的问题"

# 执行任务
octo run "列出 src 目录"

# FIM 补全（DeepSeek 填空，非 agentic）：在文件里用 <CURSOR> 标出填充点
octo complete src/lib.rs
echo 'fn add(a: i32, b: i32) -> i32 {<CURSOR>}' | octo complete

# 交互式 TUI（推荐）
octo

# 显式子命令
octo tui

# 不安装时从源码直接启动 TUI
cargo run -- tui

# 会话管理
octo resume
octo export <session-id>
```

TUI 中常用本地命令：

```text
/btw <question>   独立只读侧问；禁用工具，不写入主会话历史
/recap            生成当前会话摘要；可用时优先用 Flash 刷新，失败则保留本地摘要
/tasks            汇总后台 subagent 和后台 shell
/tasks --json     输出稳定字段：kind, id, status, started, duration, summary, latest_output
/permissions      显示可见权限模式：ask, accept-edits, plan, yolo
```

`octo` 是主命令，`octocode` 只做兼容入口。如果 PowerShell 提示找不到 `octo`，请确认 Cargo 或 npm 的 bin 目录在 PATH 中。Cargo 通常是 `%USERPROFILE%\.cargo\bin`。

## Feature Discovery

`octo features` 用来查看本地能力、竞争特性矩阵，以及根据任务描述推荐工作模式。

```bash
octo features status
octo features matrix
octo features matrix --json
octo features recommend "review src/agent/orchestrator.rs"
```

推荐模式只使用本地规则，不需要网络或 API key。可返回 `direct`、`plan`、
`agent-run`、`swarm` 或 `mission-dry-run`。

## Agent Commands

`octo agent` 暴露内置 subagent 和项目自定义 agent。

```bash
octo agent list
octo agent list --json
octo agent show code-reviewer
octo agent show security-auditor
octo agent run code-explorer "explain src/agent" --focus src/agent
```

内置 agent 包括 `code-explorer`、`code-reviewer`、`planner`、`test-runner`、
`architect`、`security-auditor` 和 `general-purpose`。

## Custom Agents

项目自定义 agent 存放在：

```text
.octocode/agents/<name>.md
```

可以从模板创建，并在提交前验证：

```bash
octo agent create my-auditor --template auditor
octo agent validate --all
octo agent validate my-auditor --json
```

模板使用 markdown 文件和 TOML frontmatter，支持 `explorer`、`reviewer`、
`auditor`、`tester`、`planner`、`writer`。

## Mission Dry-Run Runtime

`octo mission` 是长任务的最小 dry-run 记录系统。它不会执行真实改动，而是生成本地规则计划，
写入 mission 状态和事件，方便后续检查、恢复和回放。

```bash
octo mission new "refactor src/agent safely" --dry-run
octo mission status latest
octo mission inspect latest --json
octo mission inspect latest --events
octo mission replay latest
octo mission list
```

每个 mission 保存在：

```text
.octocode/missions/<mission-id>/
  mission.json
  events.jsonl
  state.json
  plan.json
```

## Safety and Policy Model

Octocode 默认通过本地 policy 系统评估工具风险：读文件、写文件、shell 命令、网络请求、
Git 操作和受保护路径会走不同的审批路径。`security-auditor` 是只读 agent，默认只允许：

```text
read_file, list_dir, search_files, search_code, git_status, git_diff
```

Mission dry-run、feature recommendation 和 agent validation 都是本地操作，不会访问网络。

## 核心能力

- **DeepSeek-native**: thinking/reasoning_content 生命周期、工具调用、上下文缓存、FIM
- **Plan Mode**: 只读分析 → 生成计划 → 风险审查 → 用户确认后执行
- **安全审批**: 路径保护、命令沙箱、风险分级、每次审批/会话授权
- **会话持久化**: 项目级会话保存、恢复、导出、分叉
- **任务闭环**: `todo_write`、`task_*` 工具和 `/task` 共用 `./.octocode/todos.json`
- **缓存可视化**: 实时显示缓存命中率、token 用量、预估费用
- **本地搜索**: ripgrep 全文搜索、文件 glob、符号搜索

## 配置

```
~/.octocode/config.toml     # 用户全局
./.octocode/config.toml     # 项目共享（提交到 git）
./.octocode/local.toml      # 本机私有（gitignore）
```

## 架构

```
octocode/
├── src/
│   ├── agent/       # 核心循环、推理管理、工具循环
│   ├── deepseek/    # API 客户端、SSE 流解析、模型管理
│   ├── plan/        # 计划生成、审查、执行
│   ├── search/      # 本地搜索、上下文打包、rerank
│   ├── tools/       # 工具实现（读/写/编辑/命令）
│   ├── policy/      # 审批引擎、沙箱、路径保护、脱敏
│   ├── storage/     # 会话/配置/凭证持久化
│   ├── workspace/   # Git/diff/apply/路径管理
│   ├── tui/         # 终端 UI
│   └── cli/         # CLI 子命令
└── tests/           # 集成测试
```

## 许可证

MIT
