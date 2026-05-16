# DS CLI 竞品调研、模块对齐与 P0 改造记录

日期：2026-05-16
仓库：`/Users/klein/ds/deepseek-code`
目标：提升 DS CLI 的性能、功能、UI、可用性与工程结构，并把每个主模块/子模块与 Codex CLI、Gemini CLI、Claude Code、Kimi CLI、Factory Droid、Qwen Code、OpenCode 对齐。

Canonical：本文件是当前 7-tool 竞品对齐报告。它取代 2026-05-15 期间仍写“6 CLI”的旧报告口径；旧报告保留为历史草稿，不作为本轮定稿依据。

本轮改动边界：当前工作区已有大量并行/历史未提交改动。本文件只声明和验证本轮新落地的两个 P0 补丁：`ds commands` 命令目录入口与 `@file` mention 索引化；其他未提交改动需要单独追溯。

## 0. 结论

DS 的问题不是“缺几个按钮”，而是能力已经散落在 `cli/tui/agent/tools/mcp/storage/policy` 里，但缺少统一入口、统一输出契约、统一权限语义和长会话性能护栏。竞品共同趋势很清楚：

- Codex、Gemini、Qwen、Kimi 都把 interactive/headless、命令发现、会话恢复、工具事件做成强入口。
- Claude、Droid、OpenCode 把权限、插件、hooks、skills、MCP、后台任务做成用户能看懂和能恢复的产品面。
- OpenCode 和 Qwen 明显往 workbench/ACP/channel 方向走，终端只是入口之一。

本轮先落地两个 P0 改动：

1. 新增 `ds commands`：把 TUI slash command registry 暴露为非交互命令，支持 JSON、过滤、项目/用户自定义 command 目录扫描。
2. 优化 `@file` 补全性能：文件 mention 从“每次按键递归扫目录”改为 `FileTree` 维护索引并在内存中过滤。

这两项直接对应竞品里的 command palette/custom commands 和 file mention 基础能力，也是后续 `/` 面板、插件命令、文档生成、命令测试同源的前置工程。

## 1. 资料边界

### 源码已核对

| 工具 | 仓库 / commit | 可信边界 |
|---|---|---|
| Codex CLI | `openai/codex@326e31a` | 核心 Rust CLI/TUI/exec/MCP/skills 源码可验证 |
| Gemini CLI | `google-gemini/gemini-cli@77e65c0` | TypeScript CLI/core/TUI/commands/policy/scheduler 源码可验证 |
| Qwen Code | `QwenLM/qwen-code@435f711` | TypeScript CLI/core/agent runtime/channel/ACP/source 可验证 |
| OpenCode | `sst/opencode@548648a` | Bun/TypeScript core/app/plugin/session/tool 源码可验证 |
| Kimi CLI | `MoonshotAI/kimi-cli@33d7b4f` | Python CLI/runtime/TUI/tool/session/subagent 源码可验证 |

### 官方资料核对，核心 CLI 源码不可验证

| 工具 | 资料来源 | 可信边界 |
|---|---|---|
| Claude Code | 官方 docs + `anthropics/claude-code` examples/plugins | 公开仓库不是核心 CLI 源码，只能验证 docs、plugins、hooks、skills 示例 |
| Factory Droid | Factory docs + `Factory-AI/factory` docs/site | 公开仓库不是 Droid CLI 源码，只能验证官方文档描述 |

## 2. 竞品信号

### Codex CLI

源码结构：`codex-rs/core`、`codex-rs/tui`、`codex-rs/execpolicy`、`codex-rs/thread-store`、`codex-rs/rmcp-client`、`codex-rs/mcp-server`、`codex-rs/memories`、`codex-rs/skills`。

对 DS 的启发：

- 把 tool/exec policy 变成独立、可测试的 policy engine。
- 会话使用 thread/event store，而不是散落状态。
- MCP、skills、plugin、memory extraction 分包，边界清晰。
- 输出截断、session prewarm、memory extraction 并发化，避免长会话卡住。

### Gemini CLI

源码结构：`packages/cli`、`packages/core`、`CommandService`、`FileCommandLoader`、`McpPromptLoader`、`SkillCommandLoader`、`scheduler`、`policy`、`memoryService`、`agents`。

对 DS 的启发：

- 命令发现由多个 loader 并行加载，并处理冲突。
- tool scheduler 负责批处理、并行 tool calls、confirmation、hooks。
- TOML policy engine 支持 alias、MCP qualified name、subagent 匹配。
- memory extraction 有 lock、throttle、idle、batch 限制，避免热路径阻塞。

### Qwen Code

源码结构：`packages/core/src/agents/runtime`、`packages/cli/src/services`、`packages/channels`、`packages/core/src/subagents`、`packages/core/src/followup`、`packages/core/src/extension`。

对 DS 的启发：

- interactive/headless 共享 AgentCore。
- queued input、cancellation level、pending approvals/live outputs/shell pids 是一等状态。
- 支持 Markdown/TOML custom commands、MCP prompts、bundled skills。
- file read cache、microcompaction、terminal-bench 直接服务长会话性能。

### Kimi CLI

源码结构：`src/kimi_cli/cli`、`app.py`、`soul/toolset.py`、`soul/approval.py`、`session*.py`、`subagents`、`background`、`cli/mcp.py`。

对 DS 的启发：

- 入口参数完整：workdir/add-dir/session/resume/model/yolo/print/acp/wire/stream-json。
- 空 prompt 可 defer MCP loading，加快首屏。
- phase timing 能定位启动慢点。
- tool-call 去重、approval source 分层、wire/context 双日志、fork/undo truncate 都值得抄。

### OpenCode

源码结构：`packages/opencode/src/session`、`tool`、`permission`、`plugin`、`mcp`、`storage`，以及 `packages/app/src/pages/session`。

对 DS 的启发：

- 不是纯 TUI，而是 CLI + workbench：composer、request tree、todo dock、permission dock、terminal panel、file tree、review tab。
- 权限是独立模块，插件能 hook permission/tool/command/compaction。
- SQL-backed session + projector/sync/event 更适合长会话和 UI 回放。
- repository cache、LSP、ripgrep 集成是性能基础。

### Claude Code

官方资料信号：

- `/agents`、`/background`、`/batch`、`/compact`、`/context`、`/hooks`、`/ide`、`/mcp`、`/permissions`、`/plan`、`/plugin`、`/resume`、`/review`、`/rewind` 等命令面很完整。
- permission modes 包含 default、acceptEdits、plan、auto、dontAsk、bypassPermissions。
- plugins 可打包 skills、agents、hooks、MCP、LSP、monitors、themes。

对 DS 的启发：

- 权限模式必须产品化，不应只表现为零散审批弹窗。
- Skills/Hooks/MCP/Plugin 要分层，让用户知道能力来自哪里。
- background/batch 任务要可 attach/logs/stop/respawn。

### Factory Droid

官方资料信号：

- `droid exec`、session id、worktree、custom droids、missions、settings、plugins、skills、MCP、Droid Computers 是主要卖点。
- autonomy tiers、Droid Shield、diff 审批、cloud session sync 体现“长任务 + 高自治 + 可恢复”。

对 DS 的启发：

- Mission 应该是一等对象，而不是 dry-run 后直接 completed。
- worktree 隔离和 cloud/local runtime 边界要提前设计。
- readiness report、wiki、missions 可以成为交付检查面。

## 3. DS 模块对齐表

| DS 模块 | 当前职责 | 竞品基准 | 主要差距 | 优先级 |
|---|---|---|---|---|
| `src/cli` | Clap 入口，chat/run/ask/agent/mission/mcp/settings/session | Codex/Gemini/Qwen/Kimi 均有稳定 headless + JSON/stream | dispatch 重复，最终 JSON 未实现，命令发现不完整 | P0 |
| `src/tui` | Ratatui UI、输入、欢迎页、状态栏、文件树、审批、转录 | Claude/Kimi/Qwen 输入/命令/文件 mention 是基础体验 | `app.rs` 过大，转录重排、文件 mention 性能风险 | P0 |
| `src/commands` | TUI slash commands | Gemini/Qwen/OpenCode 支持 loader、冲突检测、自定义命令 | 只有内建 registry，自定义命令还不是一等执行面 | P0/P1 |
| `src/agent` | Orchestrator、tool loop、subagent、swarm、router | Qwen AgentCore、Gemini scheduler、Codex core | Orchestrator 过大，interactive/headless 未统一 runtime | P1 |
| `src/tools` | read/write/edit/run/git/web/github 等工具 | OpenCode/Qwen/Gemini 工具都有结构化元数据 | 工具结果元数据弱，部分 slash command 绕开 policy | P0/P1 |
| `src/mcp` | stdio/http/sse config、registry、client | Codex/Gemini/Qwen/Kimi 都把 MCP 作为扩展入口 | trust/include/exclude 没完整进入审批和 UI | P0/P1 |
| `src/policy` | approvals、commands、paths、sandbox | Codex execpolicy、Gemini/Qwen policy engine、Claude modes | 模式语义不够产品化，字符串规则偏脆弱 | P0/P1 |
| `src/search` | files/code/symbol/git/session/semantic/packer | Qwen/OpenCode 使用 rg/LSP/cache，Gemini context pipeline | agent 搜索限制硬编码，semantic/session 仍弱 | P1 |
| `src/storage` | config/keyring/session/event/mission/task/cache | Codex thread-store、OpenCode SQL projector、Kimi wire/context | hot path 反复 load config，session events 不够耐坏 | P1 |
| `src/provider` | DeepSeek/OpenRouter/OpenAI-compatible 映射 | Qwen/OpenCode 多 provider capability 数据化 | ChatCompletions-shaped，capability/pricing/tool calling 不完整 | P2 |
| `src/mission` | dry-run mission/state/events | Droid/Kimi/Qwen 长任务与后台 session | 任务语义弱，dry-run completed 像假执行 | P1 |
| `src/runtime` | RuntimeKernel 占位 | Qwen AgentCore、Codex core、Kimi Runtime | 还没承接 config/session/policy/tools/model 服务 | P2 |

## 4. 本轮 P0 改动

### 4.1 `ds commands`

新增文件：

- `src/cli/command_catalog.rs`
- `tests/command_catalog_cli_tests.rs`

接入文件：

- `src/cli/mod.rs`
- `src/cli_entry.rs`

能力：

- `ds commands list`
- `ds commands list --json`
- `ds commands list --filter <query>`
- `ds commands locations`
- `ds commands locations --json`

输出包含：

- built-in slash command：name、aliases、group、description、usage。
- custom command files：扫描项目 `.deepseek-code/commands` 和用户 `~/.deepseek-code/commands`，支持 `.md`、`.markdown`、`.toml`。
- builtin 主命令和 alias 冲突检测，例如自定义 `/h` 会标记为与 `/help` alias 冲突。
- locations：显示 command 搜索路径是否存在。

为什么是 P0：

- Gemini/Qwen/Kimi/Claude/Droid/OpenCode 都把 command discovery 当成用户入口。
- DS 之前 `/commands` 只显示位置和数量，非交互 CLI 没有同源 command catalog。
- 后续 `/` 面板、custom commands、MCP prompts、skills commands 都可以挂到这个 catalog 上。

### 4.2 `@file` mention 索引化

修改文件：

- `src/tui/file_tree.rs`
- `src/tui/app.rs`

旧行为：

- 每次输入 `@src/...` 时，`file_mention_candidates` 都递归 `read_dir` 扫项目树。
- 大仓库下按键会触发同步 IO，容易卡住 TUI。

新行为：

- `FileTree` 新增 `mention_paths: Vec<String>`。
- `FileTree::refresh()` 用 `ignore::WalkBuilder` 建一次文件索引，遵守 `.gitignore`，跳过 `.git`、`target`、`node_modules`、`.deepseek-code`、`.cache`。
- mention 索引默认跳过隐藏文件和带空格路径，避免把 `.env` 等敏感文件或当前 resolver 不能正确解析的路径注入上下文。
- 输入过程中只对内存索引过滤、排序和截断。
- 仍保留最近 mention 优先、前缀匹配、basename 匹配；Tab 补全，Enter 在精确文件路径时提交消息。

为什么是 P0：

- 文件引用是 Claude/Kimi/Qwen/OpenCode 的基础能力。
- DS 已经有 UI，但热路径同步递归扫描会让“功能存在”变成“实际不好用”。

### 4.3 Patch Ledger

| 文件 | 改动 | 竞品依据 | 验证 | 风险 |
|---|---|---|---|---|
| `src/cli/command_catalog.rs` | 新增 built-in/custom command catalog，支持 JSON、filter、locations、builtin alias 冲突标记 | Gemini/Qwen 的 command loader，Claude/Droid/OpenCode 的 command palette | `cargo test --test command_catalog_cli_tests`，`cargo run --quiet -- commands list --json --filter mcp` | 当前只发现自定义命令文件，不执行自定义命令；冲突处理只标记不改名 |
| `src/cli/mod.rs` | 暴露 `command_catalog` 模块 | CLI 子命令同源可发现 | `cargo check` | 与已有并行 CLI 模块改动共存，提交前需看整体 diff |
| `src/cli_entry.rs` | 新增 `ds commands list|locations` 路由 | headless command discovery 是 Codex/Gemini/Qwen/Kimi 基础能力 | `cargo check`，命令 smoke | 当前命令名为 `commands`，与 TUI `/commands` 语义相近但不完全等价 |
| `tests/command_catalog_cli_tests.rs` | 覆盖 JSON builtins、project custom markdown、builtin alias conflict、locations | 防止命令入口和 schema 漂移 | `cargo test --test command_catalog_cli_tests` | 未覆盖 TOML frontmatter 的全部格式 |
| `src/tui/file_tree.rs` | `FileTree` 新增 `mention_paths`，refresh 时使用 `ignore::WalkBuilder` 建索引，并跳过隐藏/空格路径 | Qwen/OpenCode/Kimi 都避免热路径重复扫文件 | `cargo test -q file_mention` | 大仓库首次 refresh 会多一次索引成本；索引上限当前 20,000 |
| `src/tui/app.rs` | `@file` 候选改为内存索引过滤排序；Enter 精确匹配时提交，Tab 补全 | 文件 mention 是 Claude/Kimi/Qwen/OpenCode 基础交互 | `cargo test -q file_mention` | 新建文件在 refresh 前不会立刻出现在候选中 |

## 5. 下一批建议

### P0 继续

1. 转录渲染裁剪或 memoize：`transcript_view` 现在长历史下仍可能每帧全量 wrap。
2. CLI final JSON：`chat/run/ask --output-format json` 目前声明但拒绝，应输出最终结构化结果。
3. MCP policy：把 server trust、include/exclude、read/write/network 风险接入审批 UI。
4. Slash command 审批收敛：`/commit`、`/test` 等本地副作用命令应走统一 tool policy 或 background task。
5. command catalog 扩展：接入 MCP prompts、skills、custom commands 执行与冲突检测。

### P1

1. RuntimeKernel 承接 config/session/policy/tools/model，减少 orchestrator 和 TUI 直接读散落服务。
2. tool scheduler：并行 tool call、去重、取消、重试、approval source、live output。
3. Session/event store 耐坏加载、event id 去重、resume 时恢复 approval/mode/add-dir/queued input/subagent state。
4. Mission 改成 planned/running/replayable，不再用 dry-run completed 表达假完成。
5. SearchConfig 全链路生效，session/git/semantic 搜索真正参与 ask/run 上下文。

### P2

1. Worktree subagent 隔离。
2. ACP/Wire-like bridge，服务 VS Code/Zed/Web，而不是先做大 Web UI。
3. Plugin packaging：commands、skills、hooks、MCP、agents、themes。
4. Provider capability 数据化：json mode、tool calling、context、pricing、reasoning、streaming 差异。
5. SQL/projector style session storage，为 workbench UI 和长任务回放打基础。

## 6. 验证

### 6.1 2026-05-16 Implementation Status

已完成第一轮 P0 落地：

1. `chat/ask/run --output-format json` 不再被入口拒绝；现在输出单个最终 JSON 对象，字段包含 `status`、`session_id`、`final_message`、`tool_calls`、`usage`、`error`，前置失败也使用同一 schema。
2. Transcript 渲染加入 viewport/scroll aware message window，最新视图不再每帧处理完整历史；深度滚动会自动扩大历史窗口。
3. MCP 工具审批改为 registry 元数据驱动，审批详情包含 `Source: mcp:<server>.<tool>`、server/tool、trust、include/exclude、transport、read/write/network risk；调用期再次拦截未广告或被 include/exclude 禁止的工具。
4. `/commit`、`/test` 不再在 slash handler 中直接执行 git/shell 副作用，而是转成 agent 请求，走统一 tool approval。
5. `commands list --json` 扩展为统一 catalog：内置命令、自定义命令、skills、MCP prompt namespace、冲突分组。

仍未完成的后续项：RuntimeKernel 抽象、统一 tool scheduler、session projector、worktree subagent 隔离、ACP/Wire bridge、完整 MCP prompts/list 协议执行和 plugin packaging。

已运行：

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo test --test command_catalog_cli_tests`
- `cargo test --test output_format_cli_tests`
- `cargo test -q file_mention`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo run --quiet -- commands list --json --filter mcp`

## 7. Superseded Docs

以下文件仍保留 2026-05-15 期间“6 CLI”草稿口径，缺少 OpenCode 或没有当前源码边界。后续提交前应统一更新、归档或在文件头标记 superseded：

- `docs/competitor_parity_final_draft_2026-05-15.md`
- `docs/competitor_parity_matrix_2026-05-15.md`
- `docs/deepseek_ui_competitor_unified_report_2026-05-15.md`
- `docs/competitor_parity_matrix.md`

## 8. 来源

开源源码：

- https://github.com/openai/codex/tree/326e31ab65dcbdf70c4a034b7adc5c8bd335d996
- https://github.com/google-gemini/gemini-cli/tree/77e65c0db5986c559051c1b031a303dfb4829ad1
- https://github.com/QwenLM/qwen-code/tree/435f711e33dc7926fec2af62bbf3c2ec8a5464d2
- https://github.com/sst/opencode/tree/548648a3d9cb6ce37b3e318fbf3997ee8ef77e30
- https://github.com/MoonshotAI/kimi-cli/tree/33d7b4f8a012953e73ed625e45dcbea42048248d

Claude Code 官方资料：

- https://code.claude.com/docs/en/setup
- https://code.claude.com/docs/en/cli-usage
- https://code.claude.com/docs/en/commands
- https://code.claude.com/docs/en/interactive-mode
- https://code.claude.com/docs/en/permission-modes
- https://code.claude.com/docs/en/mcp
- https://code.claude.com/docs/en/plugins-reference
- https://code.claude.com/docs/en/hooks
- https://code.claude.com/docs/en/sub-agents
- https://github.com/anthropics/claude-code/tree/8bdbb7296d3fa2217283d3ef94452dd64097393b

Kimi Code 官方资料：

- https://www.kimi.com/code/docs/en/
- https://www.kimi.com/code/docs/en/kimi-code-cli/getting-started.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/core-operations.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-command.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/reference/slash-commands.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-mcp.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/customization/hooks.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/customization/skills.html
- https://www.kimi.com/code/docs/en/kimi-code-cli/customization/plugins.html

Factory Droid 官方资料：

- https://docs.factory.ai/reference/cli-reference
- https://docs.factory.ai/cli/getting-started/quickstart
- https://docs.factory.ai/cli/configuration/settings
- https://docs.factory.ai/cli/configuration/mcp
- https://docs.factory.ai/reference/hooks-reference
- https://docs.factory.ai/cli/configuration/custom-droids
- https://docs.factory.ai/cli/configuration/custom-slash-commands
- https://docs.factory.ai/cli/configuration/skills
- https://docs.factory.ai/cli/configuration/plugins
- https://docs.factory.ai/cli/features/droid-computers
- https://github.com/Factory-AI/factory/tree/cee4fbaf190dee50b756163da7aa05e8854de353
