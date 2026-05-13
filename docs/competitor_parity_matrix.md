# DS CLI Competitor Parity Matrix

This matrix records what DS should absorb from the six target tools without
copying their entire product shape. Closed-source behavior is only used when it
is documented by official sources.

| Subsystem | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | DS decision |
|---|---|---|---|---|---|---|---|
| Command spine | `exec`, `resume`, TUI, approvals | `-p`, stream JSON, slash commands | interactive + headless flags | shell-like modes, `--plan`, `--print` | `droid`, `droid exec` | Gemini-like `qwen -p` | Keep DS command spine small; add stable JSON/stream output where needed |
| Team agents | bounded subagents | agents/skills/extensions | markdown agents and tool scopes | agent files and loop limits | public details limited | no team guarantee beyond Gemini base | DS uses explicit team runtime with role boundaries and max parallel |
| Plan mode | planning and automation separated | plan/checkpoint concepts | read-only plan mode | `--plan` read-only | planning workflow docs | `/plan` style commands | DS plan stays read-only; execution moves to mission/team runtime |
| Task automation | App automations/worktrees | headless + checkpoints | background/subagent docs | loop with retry limits | `exec -s` sessions | headless/CI-friendly | DS v1 stays local-first: saved task + manual run + mission replay |
| Permissions | sandbox + approval | sandbox backends + approval | permission modes/hooks | yolo warning and retries | insufficient public detail | Docker sandbox and yolo | DS uses policy + sandbox + visible source of request |
| TUI | status/approval/diff focused | theme/accessibility options | natural transcript feel | shell-like terminal | simple exec surface | Gemini-style terminal | DS defaults to copyable classic terminal, with fullscreen optional |
| Config | TOML and project trust | layered JSON settings | user/project/local settings | TOML + MCP JSON | unknown from public docs | layered `.qwen` settings | DS defines explicit precedence and trusted project loading |
| Errors | JSONL events and failures | exit codes and error events | hooks/debug docs | debug log and retries | unknown | command error docs | DS exposes human summary + machine event log |

## Absorb

- Codex: approval/sandbox separation, event logs, bounded subagents, automation isolation.
- Gemini: extension/skill shape, plan/checkpoint ideas, accessibility/theme configuration.
- Claude Code: read-only planning, project/user agent files, permission prompts with clear tool source.
- Kimi: shell-like terminal feel, loop limits, debug logs, agent-file import.
- Qwen: open Gemini-derived command/config structure and sandbox documentation.
- Droid: minimal `exec` mental model, but only where public docs are clear.

## Avoid

- Hidden permission side effects or undocumented safety claims.
- Unlimited automatic fan-out.
- UI that hides current model, mode, cwd, permissions, or token/context state.
- Mixing plan, mission, task, and team execution into one ambiguous command.
- More slash/CLI commands before the core surfaces are stable.
