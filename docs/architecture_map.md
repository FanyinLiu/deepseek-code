# DS CLI Architecture Map

This map freezes the current architecture before the team-agent workbench
refactor. It is intentionally implementation-facing: every subsystem has one
owner, one public responsibility, and a clear boundary for future changes.

## Main Modules

| Boundary | Current modules | Responsibility | Next direction |
|---|---|---|---|
| Core runtime | `src/runtime`, `src/cli_entry.rs` | Shared config/session/audit/policy/tools/model services for every surface | Make CLI, TUI, task, and mission runners acquire services through one kernel |
| Agent runtime | `src/agent` | Turn orchestration, routing, planning, subagents, swarm, event emission | Split orchestrator into turn, plan, team, tool, and event controllers |
| Team agents | `src/agent/team.rs`, `src/agent/swarm.rs`, `src/agent/subagent` | Coordinator plan, role-specific tasks, subagent execution, result aggregation | Make `TeamPlan -> TeamRun -> TeamTask -> AgentRun` the canonical flow |
| Tools | `src/tools`, `src/workspace`, `src/mcp` | Tool schema, dispatch, local workspace changes, MCP calls | Route main agent, subagent, MCP, and tasks through one `ToolRuntime` |
| Plan / Mission / Task | `src/plan`, `src/mission`, `src/storage/scheduled_tasks.rs` | Read-only plans, replayable mission records, saved local task definitions | Keep plan read-only, mission resumable, task triggerable |
| TUI workbench | `src/tui` | Terminal input, transcript, approval, status, plan/swarm views | Split workbench panels and default to copyable classic terminal mode |
| Storage / audit | `src/storage` | Config, sessions, transcripts, events, missions, tasks, keyring | Treat `events.jsonl` as facts and Markdown artifacts as summaries |
| Policy / defense | `src/policy`, `src/defense` | Tool approvals, path/command/network risk, redaction, perimeter checks | Make policy decisions source-aware: main agent, subagent, MCP, task |
| Integrations | `src/deepseek`, `src/lsp`, `src/search`, `src/telemetry` | Model API, language service hooks, code search, runtime telemetry | Keep integrations behind runtime-owned adapters |

## Current Data Flow

1. `src/cli_entry.rs` chooses CLI command or TUI.
2. CLI/TUI loads config, API key, session, client, and project root.
3. `agent::Orchestrator` handles turn routing, context, model streaming, tools, plan, swarm, and events.
4. Tools execute through local dispatch or MCP, with policy decisions and approval events.
5. TUI consumes `AgentEvent`; audit uses durable `SessionEvent`.
6. Mission and task flows currently sit beside the main runtime instead of sharing a kernel.

## Target Data Flow

1. CLI/TUI/task/mission creates a `RuntimeKernel`.
2. Runtime starts a `TurnController`, `PlanRuntime`, `TeamRuntime`, or task runner.
3. All tool calls go through `ToolRuntime`, which owns policy, approvals, backend dispatch, result summaries, and file-change events.
4. All user-visible progress is derived from typed runtime events.
5. `events.jsonl` remains the durable source of truth; artifacts summarize complex runs.

## Refactor Rules

- Keep existing CLI command names and `AgentEvent` compatible during the transition.
- Do not let `run_subagent` silently become team/swarm execution; add explicit team entrypoints.
- Do not add another tool execution path; wrap existing dispatch until all callers converge.
- Keep raw model reasoning, raw subagent errors, and protocol details out of the main TUI transcript.
