# DeepSeek-Code 调研优化落地路线图

生成日期：2026-05-14

## 目标

本文档把两份调研材料转化为 `deepseek-code` 的可执行优化路线：

- `CLI编程助手架构设计.md`
- `AI终端编程助手调研报告.docx`

调研中的目标不是要求项目重写，而是把现有 Rust CLI/TUI 编程助手推进到更接近 Claude Code、Codex CLI、Kimi CLI、Qwen Code、OpenCode、Droid、Gemini CLI 的通用终端智能体形态。

## 当前结论

`deepseek-code` 可以按照这两份调研继续优化，而且当前代码基础已经覆盖了很多核心模块：

- DeepSeek 原生 API 客户端、SSE 流式输出、reasoning/thinking 生命周期
- ReAct/tool loop、工具调用、递归工具循环和审批事件
- Ratatui TUI、欢迎页、输入框、审批弹窗、计划进度、subagent 卡片
- 文件读写、patch、shell、搜索、Git、Web、GitHub、图片输入等工具
- Plan mode、复杂度路由、只读分析和执行选项
- Sub-agent executor、supervisor、message bus、background queue、内置角色
- MCP stdio client、工具聚合、include/exclude 过滤、超时限制
- 会话保存、恢复、导出、输入历史和项目规则注入
- LSP client 基础能力，包括 initialize、hover、definition

因此推荐采用“产品化补齐”路线，而不是进行大规模重构。

## 当前健康基线

最近一次本地验证结果（2026-05-14，P0 provider/model 收口后）：

```powershell
cargo fmt --all --check
cargo check --all-targets --all-features
cargo test --all-features
git diff --check
cargo install --path . --bin ds --force
ds --version
```

以上命令本轮均执行并通过。库测试数量为 475 个通过，集成测试同步通过。

当前工作树处于 P0 实施中，包含 provider/model 相关未提交改动；既有未跟踪审计资料保持不动：

```text
?? docs/audit/
```

## 差距概览

| 方向 | 当前状态 | 主要差距 |
| --- | --- | --- |
| Provider / 模型抽象 | DeepSeekClient 已成熟 | 缺统一 Provider trait、Provider registry、OpenAI-compatible / Anthropic / Gemini / Ollama / custom provider 适配 |
| 模型选择 | CLI/TUI 有 model 参数和 `/model` | 模型选择没有完全贯穿到真实请求链路 |
| TUI 命令体验 | slash registry、补全、`@`、`!` 已有 | 缺完整命令面板、首屏发现、TUI resume/export、真正 `/diff` 面板 |
| 权限系统 | 审批、路径保护、命令风险、session allow 已有 | 权限模式分散在 policy、TUI、YOLO、subagent 中，缺统一状态机 |
| 回滚/checkpoint | edit/write 有内存历史，checkpoint 类型存在 | `apply_patch` 未完整进入 durable rollback，checkpoint manager 未贯穿执行链 |
| MCP | stdio 可用，工具可暴露给模型 | 缺 HTTP/SSE transport、auth/OAuth、交互式 `/mcp add/list/remove` |
| Hooks | config schema 已有 | 缺 HookRunner，未接入 prompt submit、pre/post tool、stop |
| Skills / 自定义命令 | `/skills`、`/commands` 有展示 | 缺 `SKILL.md` loader 和自定义 slash command loader |
| LSP / IDE | LSP client 存在 | 未作为 agent tool 暴露，也未进入 post-edit diagnostics |
| Plan / Subagent 产品面 | 后端能力较强 | 计划执行仍偏 prompt-driven，subagent 管理界面偏被动展示 |

## 推荐实施顺序

### P0：模型选择和 Provider 抽象

这是两份调研里最关键的基础能力，也最适合第一批落地。

目标：

- 让 `chat --model`、TUI `/model`、配置默认模型真正影响请求。
- 引入最小 Provider 抽象，但第一版只实现 DeepSeek。
- 为后续 OpenAI-compatible、Anthropic、Gemini、Ollama/custom provider 预留接口。

建议文件：

- `src/deepseek/client.rs`
- `src/deepseek/models.rs`
- `src/deepseek/mod.rs`
- `src/storage/config.rs`
- `src/agent/orchestrator.rs`
- `src/cli/chat.rs`
- `src/tui/app.rs`
- `src/commands/mod.rs`

本轮已新增：

- `src/provider/mod.rs`
- `ProviderConfig` / `ProviderKind` / `ModelSelection`

后续如扩展多 provider，再拆分 `src/provider/deepseek.rs`、provider registry、endpoint、key-env 和 model catalog。

验收标准：

- 配置里的默认模型能影响 chat/run/tui 请求。
- CLI `--model` 优先级高于配置。
- TUI `/model flash|pro` 对后续 turn 生效。
- Provider factory 能从配置创建 DeepSeek provider。
- 所有现有 DeepSeek-native 特性不回退。

建议测试：

- `config_default_model_reaches_request`
- `chat_model_override_reaches_request`
- `tui_model_command_updates_active_session`
- `provider_config_parses_deepseek`
- `provider_factory_builds_deepseek_client`

### P1：TUI 产品化补齐

目标是让已有能力更容易被用户发现和控制。

建议任务：

1. 实现真正 `/diff` 命令和 diff viewer 面板。
2. 增强 `/` 命令面板，支持搜索、描述、快捷选择。
3. 增强 `@` 文件/目录引用补全。
4. 增加 `Ctrl-X` 持久 shell mode，并在状态栏显示当前 shell/agent 模式。
5. 把 TUI session resume/export 从提示变成可操作界面。

建议文件：

- `src/commands/mod.rs`
- `src/tui/app.rs`
- `src/tui/input.rs`
- `src/tui/diff_viewer.rs`
- `src/tui/status_bar.rs`
- `src/tui/statusline.rs`
- `src/storage/sessions.rs`
- `src/storage/transcripts.rs`

验收标准：

- `/diff` 能查看 working tree diff 和本轮变更。
- `Ctrl-X` 后普通输入按 shell 命令处理，退出后恢复 agent 输入。
- slash command 能在 TUI 内被浏览、过滤、补全。
- session 列表能在 TUI 里展示并选择恢复。

### P2：统一权限模式和 durable rollback

调研中的主流产品都把权限模式作为一等公民。当前项目已有审批基础，但模式分散。

建议统一为：

- `Plan`：只读探索，不允许写入和 shell mutation。
- `Default`：读自动放行，写入/命令需确认。
- `AcceptEdits`：编辑自动放行，命令仍需确认。
- `Auto`：普通工具自动放行，高风险操作确认。
- `Bypass`：全部自动放行，仅建议隔离环境使用。

建议任务：

1. 新增统一 `PermissionMode`，并让 TUI、policy、subagent 共用。
2. 将 `/permissions` 从状态展示升级为可切换模式。
3. 把 `apply_patch`、`edit_file`、`write_file` 接入 durable checkpoint。
4. 对 `apply_patch` 增加 patch size limit 和执行 timeout。
5. 增加显式“编辑后验证”审批流。

建议文件：

- `src/policy/approvals.rs`
- `src/policy/sandbox.rs`
- `src/storage/config.rs`
- `src/agent/tool_loop.rs`
- `src/agent/subagent/executor.rs`
- `src/workspace/apply.rs`
- `src/agent/checkpoints.rs`
- `src/commands/mod.rs`

验收标准：

- 主 agent 和 subagent 权限模式语义一致。
- `/permissions` 能清晰展示当前模式和 session approvals。
- 所有文件修改都能被 checkpoint/restore 覆盖。
- 高风险 patch 和超大 patch 在执行前被阻止或要求确认。

### P3：MCP、Hooks、Skills、自定义命令

这一阶段补生态能力。

建议任务：

1. MCP 增加 transport enum：`stdio`、`http`、`sse`。
2. `/mcp` 支持 `list/status/add/remove/test`。
3. 引入 HookRunner，先接入 `pre_tool`、`post_tool`、`stop`。
4. 实现 `.deepseek-code/skills/<name>/SKILL.md` loader。
5. 实现 `.deepseek-code/commands/*.md` 或 `.toml` 自定义命令。

建议文件：

- `src/mcp/client.rs`
- `src/mcp/registry.rs`
- `src/mcp/protocol.rs`
- `src/storage/config.rs`
- `src/agent/prompt_builder.rs`
- `src/commands/mod.rs`
- 新增 `src/hooks/mod.rs`
- 新增 `src/extensions/skills.rs`
- 新增 `src/commands/custom.rs`

验收标准：

- MCP stdio 行为不退化。
- 配置文件可声明 HTTP/SSE MCP server。
- hook 能在测试中 block 或记录 tool 执行。
- skill 可被发现，并按需注入 prompt。
- 自定义 slash command 可在 `/` 面板里显示并转成 agent prompt。

### P4：LSP/IDE 和分发 polish

这一阶段让项目更接近 OpenCode/Codex 的成熟体验。

建议任务：

1. 暴露 `lsp_hover`、`lsp_definition`、`lsp_diagnostics` 工具。
2. 在文件编辑后自动收集 LSP diagnostics，并进入 self-verification。
3. 增加 release archives、checksums、shell completions。
4. 后续再考虑 HTTP daemon、IDE bridge、WASM 插件、Docker 完整沙箱。

建议文件：

- `src/lsp/client.rs`
- `src/deepseek/tools.rs`
- `src/tools/dispatch.rs`
- `src/agent/orchestrator.rs`
- `.github/workflows/ci.yml`

## 第一批开工建议

推荐第一批只做一件完整闭环：

> 模型选择真正生效 + 最小 Provider 抽象 + 测试覆盖

原因：

- 调研文档中“模型无关”是产品定位第一原则。
- 当前代码中模型选择存在真实断点，属于高价值修复。
- Provider 抽象可以小步做，不需要立刻支持所有厂商。
- 做完后，后续 MCP、subagent、plan 都能复用统一模型/Provider 管理。

第一批具体拆分：

1. 梳理当前 `DeepSeekModel`、`ReasoningState`、`ExecutionLane` 的关系。
2. 新增 provider trait 和 DeepSeek provider adapter。
3. 让 config、CLI、TUI 的 model selection 写入同一个 session/request 状态。
4. 给 chat/run/tui 三条路径补回归测试。
5. 跑完整验证门槛。

## 每批改动后的验证门槛

每批代码修改后必须运行：

```powershell
cargo fmt --all --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

涉及 TUI 的批次，额外运行：

```powershell
cargo run --bin deepseek-code -- preview-tui --scenario welcome --width 80 --height 24
cargo run --bin deepseek-code -- preview-tui --scenario workbench --width 120 --height 28
```

涉及 CLI surface 的批次，额外运行：

```powershell
cargo run --bin deepseek-code -- --help
cargo run --bin ds -- --help
cargo run --bin dscode -- --help
```

## 暂不建议立即做的内容

以下内容可以保留在路线图里，但不建议作为下一步：

- 大规模 workspace/crates 拆分
- WASM 插件运行时
- Docker 完整沙箱
- HTTP daemon 多客户端模式
- IDE bridge / ACP 协议
- npm/Homebrew/Scoop 分发链路

原因是这些改动跨度大，会稀释当前最有价值的产品化工作。先把模型、权限、TUI、回滚、MCP 基础体验打实，再做生态和分发会更稳。

## 风险与注意事项

- 不要把 `D:\deepseek-code\.deepseek-code` 当成项目根目录；真实仓库根目录是 `D:\deepseek-code`。
- Windows 上预览或运行中的 exe 可能锁住构建产物；必要时使用单独 `--target-dir`。
- 涉及 TUI 中文文本时，测试应避免过度依赖宽字符精确快照。
- Provider 抽象第一版应保持 DeepSeek-native 能力，不要为了通用接口丢掉 reasoning、cache、FIM 等特性。
- 权限模式重构要先补测试矩阵，再改执行路径。
