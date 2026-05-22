# Octocode CLI 竞品吸收优化决策 2026-05

目标：把 Octocode 打磨成 Rust、DeepSeek-native、中文/Windows 优先的本地 CLI 平台。`octo` 继续是主入口，`octocode` 只保留兼容；项目约定继续放在 `.octocode/`，不从其他 CLI 复制专有实现。

## 调研来源

- Kimi Code CLI: core operations, sub-agents
- Qwen Code: commands, architecture, fork subagent design
- OpenCode: agents, custom commands
- Gemini CLI: commands, plan mode
- Droid CLI: CLI reference
- 本机对照：`D:\octocode` 当前源码和 `D:\main` 的目录级功能学习

## Adopt Now

| 能力 | 竞品启发 | Octocode 落点 | 本轮实现/测试入口 |
| --- | --- | --- | --- |
| 侧问不中断主线 | Kimi/Qwen/Gemini 都强调命令面和任务主线分离 | TUI slash command | `/btw <question>`：只读、禁工具、不写主会话；TUI 可用时用 Flash 旁路回答，失败给本地说明。测试在 `src/commands/mod.rs` |
| 手动会话摘要 | Gemini plan/recap 类工作流和主流 compact 思路 | TUI local output + compact state | `/recap`：不压缩、不改历史；TUI 可用时用 Flash 刷新，失败保留本地摘要。测试在 `src/commands/mod.rs` |
| 后台任务统一查看 | Droid/OpenCode 的后台执行可观测性 | `BackgroundQueue` + `background_shells` | `/tasks` 汇总 subagent 和 background shell；`/tasks --json` 输出 `kind/id/status/started/duration/summary/latest_output` |
| 权限模式可读标签 | 多数 CLI 用少量模式表达审批策略 | `policy::PermissionMode` 和 `/permissions` | 可见模式统一为 `ask | accept-edits | plan | yolo`；旧 `read_only/accept_edits/bypass` 继续兼容 |
| 简单项目命令扩展 | Qwen/OpenCode custom commands | `.octocode/commands/*.md` | 保持纯提示词模型，不引入 shell/file 注入面 |

## Keep Current

| 能力 | 原因 | 当前模块 |
| --- | --- | --- |
| Rust 本地单体架构 | Windows 分发、TUI、工具审批都更容易控制 | `src/cli_entry.rs`, `src/tui`, `src/runtime` |
| `ToolRuntime` 统一工具执行通道 | 已覆盖 main/subagent/MCP 的审批、hook、audit 汇聚方向 | `src/runtime/tool_runtime.rs`, `src/tools/dispatch.rs` |
| `.octocode/agents/*.md` TOML frontmatter | 足够简单，符合本地可审计扩展 | `src/cli/agent.rs`, `src/agent/subagent/registry.rs` |
| Provider/model 派生上下文窗口 | 避免显示虚假的大上下文 | `src/provider/mod.rs`, `/context`, statusline |
| mission dry-run 可重放 | 长任务不急于引入远程 channel 或 worktree 隔离 | `src/mission`, `.octocode/missions/<id>/events.jsonl` |

## Defer

| 能力 | 推迟原因 | 未来入口 |
| --- | --- | --- |
| ACP/IDE bridge | 需要稳定协议和权限边界 | `src/runtime`, future `octo serve` |
| 插件市场 | 当前 skills/hooks/MCP 已够用；市场会扩大信任面 | `src/cli/command_catalog.rs`, plugin manager |
| 跨重启后台任务恢复 | shell/subagent 当前是进程内生命周期；持久恢复需要日志、PID、重放语义 | `src/tools/background_shells.rs`, `src/agent/background.rs` |
| worktree 隔离 | 对写入/合并/冲突策略要求高 | `src/workspace`, `src/agent/swarm.rs` |
| 命令文件 shell 注入 | 风险高，先保持提示词命令 | `.octocode/commands` |

## Reject

| 能力 | 原因 |
| --- | --- |
| 复制 `D:\main` 专有实现或命名细节 | 只做功能级学习，避免泄露/专有代码污染 |
| 把 `octocode` 重新作为主入口 | 产品主入口已经收敛到 `octo` |
| 用配置值伪造上下文窗口 | `/context` 必须来自 provider/model 能力与本地预算 |
| 默认开启 destructive MCP 工具 | 必须继续受 `allow_destructive` 和审批约束 |
| 在本轮引入持久后台 shell 恢复承诺 | 需要进程监督和日志边界，本轮只承诺进程内 |

## 本轮验证清单

- `cargo check --all-targets --all-features`
- `cargo test --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`
- 若后续触及 mission 存储，再额外跑 `cargo test --test mission_store_tests --all-features` 和 `cargo test --test mission_cli_tests --all-features`
