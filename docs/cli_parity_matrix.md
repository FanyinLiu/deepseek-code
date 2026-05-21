# CLI Parity Matrix

Date: 2026-05-13

This matrix compares `octocode` against current public CLI-agent product
patterns. It is intentionally product-focused: the goal is not to clone another
tool, but to identify which CLI surfaces make long-running coding work
discoverable, resumable, safer, and easier to verify.

## Sources Checked

- Reference docs (omitted)
- Reference subagents docs (omitted)
- Reference subagents docs (omitted)
- OpenAI Peer B CLI help article: <https://help.openai.com/en/articles/11096431>
- Kimi Code CLI quick start: <https://www.kimi.com/code/docs/en/kimi-code-cli/getting-started.html>
- Kimi Code CLI core operations: <https://www.kimi.com/code/docs/en/kimi-code-cli/core-operations.html>
- Kimi CLI guide: <https://www.kimi.com/code/docs/en/kimi-cli.html>
- Qwen Code commands docs: <https://qwenlm.github.io/qwen-code-docs/en/users/features/commands/>
- Qwen Code MCP docs: <https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/>
- Gemini CLI commands docs: <https://google-gemini.github.io/gemini-cli/docs/cli/commands.html>
- Gemini CLI custom commands docs: <https://google-gemini.github.io/gemini-cli/docs/cli/custom-commands.html>
- Gemini CLI plan mode docs: <https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/plan-mode.md>
- Factory Droid CLI overview: <https://docs.factory.ai/cli/getting-started/overview>
- Factory Droid CLI reference: <https://docs.factory.ai/reference/cli-reference>
- OpenCode CLI docs: <https://opencode.ai/docs/cli/>
- OpenCode commands docs: <https://opencode.ai/docs/commands/>
- Existing octocode docs and source tree in this repository.

## Capability Matrix

| Capability | Peer A | Peer B | Kimi | Qwen | Gemini | Droid | OpenCode | octocode current | Action |
|---|---|---|---|---|---|---|---|---|---|
| project memory file | peer convention and memory scopes | AGENTS.md/project instructions | project context docs | `/init`, `/memory`, durable memory | GEMINI.md and memory reload | project/org context | project config and instructions | project guidance injection exists, diagnostics thin | already-have |
| custom agents | markdown agents, CLI `--agents` | emerging subagent patterns | not prominent in public docs | agents/skills surface | `/agents` registry | enterprise agent workflows | `opencode agent create` | markdown/TOML registry exists, no CLI surface | implement-now |
| subagents | built-in and custom subagents | subagents in current product docs | autonomous planning/execution, no clear custom subagents | agents/skills features | local/remote agents list | background process/workflow model | primary/subagent modes | built-in executor, supervisor, TUI cards | already-have |
| parallel execution | background subagents and fork/team patterns | multi-step autonomous mode | agent plans actions autonomously | task/session tooling | research subagents in plan mode | background process management | subtask/task permissions | supervisor, swarm, background queue | already-have |
| task delegation | explicit delegate to named agents | agent chooses tools/tasks | autonomous task steps | skills/commands delegate | agents and plan tools | workflow/exec model | task tool and subagents | router/supervisor/decomposer present | already-have |
| plan mode | permission mode includes plan | suggest/full-auto workflows; plans visible in product | read-only plan mode | `/plan`, approval-mode plan | first-class plan approval mode | plan-to-implementation workflow | planning via agents/commands | plan CLI and tracker exist | already-have |
| full-auto mode | `dangerously-skip-permissions` | `--full-auto` sandboxed | autonomous agent execution | yolo approval mode | YOLO/approval modes | non-interactive exec | run/headless automation | policy has auto/yolo concepts but not unified | implement-later |
| approval modes | default, acceptEdits, auto, bypass, plan | suggest, auto-edit, full-auto | plan vs agent/shell mode | plan/default/auto-edit/yolo | default/auto-edit/plan/yolo | transparent review workflows | permission allow/deny/ask | tool policy and approvals exist | already-have |
| patch preview | interactive patch review | prints proposed patches | code edits shown in terminal | restore/diff flows | checkpoint/diff UX | review workflow | diff/session review commands | diff viewer and apply policy exist | already-have |
| diff apply | edit/apply tools | patch application | edits files | file restore/checkpoint | checkpoint/restore | implementation lifecycle | edit tools | workspace apply and diff modules | already-have |
| run history | sessions and resume | local sessions | browser UI session management | `/chat save/resume/list` | `/chat save/resume/list` | session ids in exec | session commands | session store and transcripts exist | already-have |
| resume session | `--resume`, `--continue` | resume current work | browser/session UI | `/resume`, `/chat resume` | `/resume`, `--session` | `droid exec -s` | `--continue`, `--session` | `octocode resume` and export exist | already-have |
| replay logs | transcript/history review | visible inline history | export/import task docs | `/chat share`, recap | chat share/session retention | bug/log workflows | export/import/session db | transcript export exists, replay thin | implement-now |
| MCP support | MCP tools and scoped MCP settings | MCP support in product docs | MCP support | `qwen mcp` commands | MCP reload/read tools | integrations model | MCP commands | stdio MCP registry/client | already-have |
| custom commands | slash commands, commands/skills | slash commands | slash commands listed | custom markdown/TOML commands | custom TOML commands | `/commands` | markdown commands | built-in registry only | implement-later |
| skills/hooks | skills and lifecycle hooks | skills/hooks in product docs | not prominent | `/skills`, extensions | `/skills reload` | workflow integrations | skills permissions | hooks schema only, no skills loader | implement-later |
| TUI dashboard | full interactive terminal | terminal UI | terminal and browser UI | terminal UI | terminal UI | REPL | TUI default | Ratatui TUI exists | already-have |
| plain terminal/SSH mode | headless `-p`, JSON output | terminal local CLI | terminal CLI, ACP | terminal CLI | headless `-p` | `droid exec` | `opencode run` | normal CLI commands exist | already-have |
| JSON output | `--output-format json` | machine-readable modes | export/import, no universal JSON | share JSON | headless output options | exec automation | CLI automation outputs | limited JSON currently | implement-now |
| provider flexibility | Peer models | OpenAI models | Kimi models | Qwen/coding plan/API key | Gemini models | Factory service, integrations | provider/model string | DeepSeek-first only | implement-later |
| model selection | `--model`, subagent model | `-m` | product model defaults | `/model`, auth plans | `--model`, `/model` | service-managed | `--model provider/model` | pro/flash options | already-have |
| background tasks | background subagents | full-auto long tasks | task automation | task/session commands | session/task trackers | `/bg-process` | subtask/task tools | background queue exists | already-have |
| permissions engine | detailed permission modes | approval/sandbox modes | plan/shell restrictions | approval-mode | approval/sandboxing | review workflows | per-resource permissions | policy modules for commands/paths/sandbox | already-have |
| protected paths | settings and permissions | sandbox to cwd in full-auto | plan restrictions | restore/safety docs | sandbox/checkpointing | enterprise controls | permission patterns | protected path policy exists | already-have |
| shell command policy | Bash permissions | command approval | shell mode Ctrl-X | `!` shell commands and approval modes | `!` commands with confirmations | bash mode `!` | bash permission type | command policy exists | already-have |
| code search | Read/Grep/Glob agents | local read/search | read/edit/search/fetch | grep/tools | grep/glob/search | codebase understanding | grep/glob/lsp tools | search modules and packer exist | already-have |
| context packing | separate subagent contexts | local context summaries | autonomous context | `/compress` | `/compress` | compress/session workflows | session/context commands | search packer and prompt builder | already-have |
| token/cost display | usage/status surfaces | model/usage in CLI | not central | `/context` token usage | context display | billing/account | stats command | not first-class | implement-later |
| validation commands | doctor/help | help/upgrade | version/task docs | auth status/tools | `/about`, checks | CLI reference/exec | debug/stats | doctor, tests in docs | already-have |
| security audit agent | custom/security agents possible | review/safety possible | not explicit | custom skills possible | agents possible | enterprise security | custom agents possible | security-auditor exists but needs defaults | implement-now |
| worktree isolation | subagent worktree isolation | sandbox/full-auto | not prominent | restore/checkpoints | sandbox/checkpointing | workflow isolation | sessions/subtasks | no worktree isolation | implement-later |
| long-running workflow support | background/resume/max turns | full-auto and approvals | agent execution and browser UI | resume/compress/recap | plan/session retention | bg-process, exec session | session/run/serve | no mission record yet | implement-now |
| custom templates | subagent examples/templates | prompts/templates emerging | docs/templates | command templates | custom command TOML | prompt file exec | agent/command templates | none for agent CLI | implement-now |
| install/doctor command | doctor/help/update docs | install/upgrade | installer and help | auth status/setup | about/settings | install/reference | upgrade/uninstall/debug | `octocode doctor` exists | already-have |
| documentation quality | extensive official docs | help/GitHub docs | official docs | detailed command docs | detailed docs | reference docs | strong docs | repo docs partial | implement-now |
| agent management CLI | `/agents`, `--agent` | subagent commands/docs | limited | `/agents` docs emerging | `/agents list/reload` | no explicit agent CLI in reference | `opencode agent list/create` | missing `octocode agent` command | implement-now |
| mission/run records | resumable sessions and background work | session logs | task/browser session management | recap/share/checkpoints | plans/session retention | exec sessions/logs | session db/export/import | missing mission store | implement-now |
| feature discovery | help, slash command listings | help article/CLI help | quickstart/core operations | `/tools`, `/skills`, `/commands` | `/help`, command docs | CLI reference | rich CLI command list | no `octocode features` | implement-now |
| TUI command hints | command palette/agents UI | in-terminal command help | status bar modes | command help and shortcuts | command help | slash commands | TUI commands | welcome/help need new commands | implement-now |

## Top 15 Product Ideas to Steal

1. A first-class `features` command that explains what the CLI can already do.
2. A visible agent catalog with built-in and project custom agents.
3. Template-based custom agent creation.
4. Agent validation before runtime use.
5. A dedicated security auditor agent with read-only defaults and VETO language.
6. Dry-run mission records for long-running task planning.
7. Replayable mission/event logs for post-run inspection.
8. Compact JSON output for scripting and tests.
9. Mode recommendations based on task shape.
10. Explicit local capability status including config paths.
11. Documentation that maps product ideas to concrete commands.
12. TUI welcome/help copy that advertises advanced commands without clutter.
13. Read-only planning and audit surfaces as safe defaults.
14. Durable local stores under `.octocode/`.
15. Copy-paste command examples that cover the core workflow.

## Octocode Existing Strengths

- Rust-native CLI and Ratatui TUI.
- DeepSeek-specific model handling with `Pro` and `Flash` modes.
- Existing policy modules for approvals, commands, sandboxing, paths, and redaction.
- Existing session storage and transcript export.
- Existing search, code packing, and workspace diff/apply modules.
- Existing subagent architecture: registry, supervisor, executor, swarm, and background queue.
- Existing MCP stdio client/registry foundation.
- Existing local task queue commands.
- Existing doctor and login flows.

## P0 Implementation Targets

- Add `octocode features matrix/status/recommend`.
- Add `octocode agent list/show/run/create/validate`.
- Finish `security-auditor` built-in defaults.
- Add minimal dry-run mission runtime and persistent event/state store.
- Add JSON outputs for the new commands.
- Add tests covering command output, stores, and validation.

## P1 Implementation Targets

- Improve command help and TUI welcome/status copy for the new command surfaces.
- Document feature discovery, custom agents, mission dry-run, and safety model.
- Add richer replay/log views for mission events.
- Add `octocode doctor` checks for custom agent and mission store health.
- Normalize custom-agent frontmatter compatibility beyond the minimal TOML-like subset.

## P2 Implementation Targets

- Add custom slash-command loading.
- Add first-class skills loader and hook runner.
- Add worktree isolation for subagents or mission steps.
- Add token/cost display and command-level usage summaries.
- Add richer provider/model abstraction beyond DeepSeek-first operation.
