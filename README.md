# DeepSeek-Code

DeepSeek 原生编程代理软件 — 围绕 DeepSeek API 特有能力设计的本地编程代理。

## 安装

```bash
# 从源码目录安装日常命令 ds
cd deepseek-code
cargo install --path . --bin ds --force

# 也可以安装兼容长别名 dscode
cargo install --path . --bin dscode --force

# 不安装也可以直接运行
cargo run --bin ds -- tui

# 构建三个二进制：deepseek-code + dscode + ds
cargo build --release
```

## 快速开始

```bash
# 登录，也可以首次打开 TUI 后直接粘贴 API key
deepseek-code login --api-key sk-xxx

# 诊断
deepseek-code doctor

# 对话
deepseek-code chat "解释这个项目"

# 提问（含项目搜索上下文）
deepseek-code ask "auth middleware 在哪里"

# 搜索代码
deepseek-code search "orchestrator"

# 计划模式（只读分析）
deepseek-code plan "修复登录失败的问题"

# 执行任务
deepseek-code run "列出 src 目录"

# 交互式 TUI（推荐）
ds

# 兼容显式子命令
ds tui
dscode tui

# 不安装时从源码直接启动 TUI
cargo run --bin ds -- tui

# 会话管理
deepseek-code resume
deepseek-code export <session-id>
```

`ds` 是面向日常使用的短命令，`dscode` 保留为兼容长别名。如果 PowerShell 提示找不到
`ds`，请确认 Cargo 的 bin 目录在 PATH 中，通常是 `%USERPROFILE%\.cargo\bin`。

## Feature Discovery

`ds features` 用来查看本地能力、竞争特性矩阵，以及根据任务描述推荐工作模式。

```bash
ds features status
ds features matrix
ds features matrix --json
ds features recommend "review src/agent/orchestrator.rs"
```

推荐模式只使用本地规则，不需要网络或 API key。可返回 `direct`、`plan`、
`agent-run`、`swarm` 或 `mission-dry-run`。

## Agent Commands

`ds agent` 暴露内置 subagent 和项目自定义 agent。

```bash
ds agent list
ds agent list --json
ds agent show code-reviewer
ds agent show security-auditor
ds agent run code-explorer "explain src/agent" --focus src/agent
```

内置 agent 包括 `code-explorer`、`code-reviewer`、`planner`、`test-runner`、
`architect`、`security-auditor` 和 `general-purpose`。

## Custom Agents

项目自定义 agent 存放在：

```text
.deepseek-code/agents/<name>.md
```

可以从模板创建，并在提交前验证：

```bash
ds agent create my-auditor --template auditor
ds agent validate --all
ds agent validate my-auditor --json
```

模板使用 markdown 文件和 TOML frontmatter，支持 `explorer`、`reviewer`、
`auditor`、`tester`、`planner`、`writer`。

## Mission Dry-Run Runtime

`ds mission` 是长任务的最小 dry-run 记录系统。它不会执行真实改动，而是生成本地规则计划，
写入 mission 状态和事件，方便后续检查、恢复和回放。

```bash
ds mission new "refactor src/agent safely" --dry-run
ds mission status latest
ds mission inspect latest --json
ds mission inspect latest --events
ds mission replay latest
ds mission list
```

每个 mission 保存在：

```text
.deepseek-code/missions/<mission-id>/
  mission.json
  events.jsonl
  state.json
  plan.json
```

## Safety and Policy Model

DeepSeek-Code 默认通过本地 policy 系统评估工具风险：读文件、写文件、shell 命令、网络请求、
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
- **缓存可视化**: 实时显示缓存命中率、token 用量、预估费用
- **本地搜索**: ripgrep 全文搜索、文件 glob、符号搜索

## 配置

```
~/.deepseek-code/config.toml     # 用户全局
./.deepseek-code/config.toml     # 项目共享（提交到 git）
./.deepseek-code/local.toml      # 本机私有（gitignore）
```

## 架构

```
deepseek-code/
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
