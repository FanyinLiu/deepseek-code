# OpenAI Codex CLI / DeepSeek TUI Comparison

Date: 2026-05-09

This document compares `deepseek-code` against the current public OpenAI Codex
CLI and the community DeepSeek TUI project. The goal is not to clone either
project, but to identify the product gaps that matter if `deepseek-code` should
feel like a polished terminal agent while borrowing the best interaction ideas
from Codex CLI and DeepSeek TUI.

## Sources Checked

- OpenAI Codex CLI GitHub: <https://github.com/openai/codex>
- OpenAI Codex CLI features: <https://developers.openai.com/codex/cli/features>
- OpenAI Codex slash commands: <https://developers.openai.com/codex/cli/slash-commands>
- OpenAI Codex MCP docs: <https://developers.openai.com/codex/mcp>
- OpenAI Codex subagents docs: <https://developers.openai.com/codex/subagents>
- OpenAI Codex hooks docs: <https://developers.openai.com/codex/hooks>
- OpenAI Codex skills docs: <https://developers.openai.com/codex/skills>
- DeepSeek TUI GitHub: <https://github.com/Hmbown/DeepSeek-TUI>
- DeepSeek TUI site: <https://deepseek-tui.com/en>

## Current Position

`deepseek-code` already has a credible core: Ratatui TUI, DeepSeek streaming,
tools, approval policy, plan mode, subagents, MCP stdio, sessions, memory
listing, rollback, LSP client, syntax highlighting, and dual binaries
(`deepseek-code` / `dscode`).

The gap is product completeness. Codex CLI and DeepSeek TUI are stronger at
making every capability discoverable, resumable, configurable, and safe under
one consistent runtime model.

## Comparison Matrix

| Area | OpenAI Codex CLI | DeepSeek TUI | deepseek-code now | Gap / decision |
|---|---|---|---|---|
| Install / launch | `npm`, Homebrew, releases; `codex` starts TUI. | `npm`, Cargo, Homebrew, direct release, Docker; dispatcher plus companion binary. | Cargo/source works; `dscode` now installable locally; README still has placeholders. | Ship real release artifacts and make `dscode` the clean user-facing command. |
| First-run auth | ChatGPT sign-in or API key. | First launch prompts for DeepSeek key; `auth status`, `doctor`. | TUI asks for key if missing; keyring/project fallback. | Add `dscode auth status`, doctor-lite in first run, and clearer key-source precedence. |
| TUI composer | Full-screen TUI, screenshots, queued follow-ups, history search. | Keyboard-driven TUI, reasoning blocks, command-heavy workflow. | Multiline input, repeat-key fixes, `!` shell mode, basic history. | Add command palette, Ctrl-R history search, paste handling, file mention completion. |
| Slash commands | Large built-in command surface: permissions, model, agents, status, MCP, diff, review, init, compact, resume, etc. | Commands for model auto, auth, doctor, restore, theme, MCP-related flows. | Several built-ins exist; no palette or custom command discovery. | Build `/` popup and `.deepseek-code/commands` loader. |
| Permission modes | Explicit permissions/sandbox controls, approval overlays, subagent inheritance. | Plan / Agent / YOLO modes with sandbox protection. | Tool-level policy, approve once/session, auto_mode flag, yolo command. | Convert flags into first-class modes: read-only, accept-edits, default, auto, bypass. |
| Shell mode | `!` commands can be queued and controlled inside TUI. | Shell execution through typed registry and sandbox. | `!cmd` now routes through approved `run_command`. | Add queued `!cmd` while a turn is running and better command output panes. |
| Plan mode | Plans visible before execution; slash-driven control. | Plan is read-only explore; Agent asks; YOLO auto-approves. | Plan generation/review/options/tracker exist. | Add refine/edit plan, explicit read-only enforcement, plan history in transcript. |
| Subagents | Parallel specialized agents, `/agent`, custom TOML agents, max thread/depth settings. | Sub-agents and durable task queue. | Built-in/custom-ish registry, isolated sessions, cards, no nested subagents by default. | Add `/agents` thread switcher, `/tasks`, permission inheritance, resumable task records. |
| Skills / plugins | Skills are first-class, progressive-disclosure `SKILL.md`; plugins distribute skills/apps. | Skills system is listed as installable instruction packs. | No real skills loader yet. | Implement `.deepseek-code/skills/<name>/SKILL.md` and later plugin packaging. |
| MCP | STDIO and Streamable HTTP; bearer/OAuth; CLI management; `/mcp`. | MCP servers; HTTP/SSE runtime work is active. | Stdio MCP, filters, timeout, status. | Add HTTP/SSE transports, auth placeholders, `/mcp add/list/remove/status`. |
| Hooks | Lifecycle hooks: prompt submit, pre/post tool, permission request, stop. | Not the main differentiator, but product has workflow integrations. | Config schema only. | Add HookRunner and wire `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`. |
| Runtime API | App server / remote TUI / automation paths. | `deepseek serve --http` for headless workflows. | No serve mode. | Add `dscode serve --http` after MCP/permissions stabilize. |
| Rollback | Review/diff/status are tightly integrated. | Side-git snapshots and `/restore` without touching repo git. | In-memory undo/restore by turn. | Make rollback durable and show checkpoints in TUI. |
| LSP diagnostics | Codex has strong review/tooling ecosystem; exact LSP behavior varies by client. | Inline diagnostics after edits via common LSP servers. | LSP client exists but not deeply wired into post-edit loop. | Feed diagnostics into self-verification and TUI diff/workbench. |
| Distribution | Mature releases and docs. | Prebuilt Windows/Linux/macOS, Docker, npm wrapper. | CI exists; installed `dscode` locally; release docs thin. | Add release archives, checksums, Windows install path, Scoop manifest, shell completions. |

## Priority Backlog

### P0: Make the Current TUI Feel Dependable

1. Add `/` command palette with filtering and short descriptions.
2. Add Ctrl-R prompt history search.
3. Add `@` file/folder mention completion.
4. Add paste handling tests, especially multi-line paste on Windows.
5. Add event playback snapshots for approval, plan, subagent, shell, and MCP states.

### P1: Bring Permissions Up to Modern Agent Expectations

1. Introduce one permission mode enum and route all policy decisions through it.
2. Add `/permissions` editor instead of only status output.
3. Add explicit approved verification after edits.
4. Persist session approvals and show what is approved.
5. Add durable rollback checkpoints instead of in-memory-only undo.

### P2: Make Subagents a Product Surface

1. Add `/agents` to list/switch/close agent threads.
2. Add `/tasks` for background and completed subagent work.
3. Store subagent task records for resume.
4. Add custom agent manifests compatible with TOML and markdown/YAML formats.
5. Split subagent deltas into structured phases: reasoning summary, content, tool, result.

### P3: Ecosystem Parity

1. Add skills loader with progressive disclosure.
2. Add hooks runner and lifecycle events.
3. Add MCP HTTP/SSE transport and auth placeholders.
4. Add custom slash-command files.
5. Add `dscode serve --http` after permissions and MCP are stable.

### P4: Distribution Polish

1. Replace README placeholder install commands with real local/source commands first.
2. Add release archives for Windows/Linux/macOS.
3. Add checksums.
4. Add PowerShell/Shell completions.
5. Add Scoop/Homebrew/npm-wrapper plan only after binaries are reliable.

## Immediate Next Implementation Order

The next code batch should be:

1. `/` command palette in TUI.
2. Ctrl-R history search.
3. `@` file mention completion.
4. `/agents` and `/tasks` command surfaces.
5. Skills loader.

This order improves everyday interaction first, then exposes the existing
multi-agent architecture instead of adding another hidden backend feature.
