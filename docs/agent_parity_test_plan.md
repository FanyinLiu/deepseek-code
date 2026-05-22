# Agent Parity Test Plan

This project tracks the best current terminal-agent workflows as the target.
The goal is not a pixel clone; it is a quiet terminal agent with strong
planning, permissions, subagents, memory, MCP, hooks, command discovery, and
reliable verification.

## Current Parity Snapshot

| Area | Current state | Gap to close |
| --- | --- | --- |
| Welcome UI | Product-like terminal welcome with project context and launch prompts | Keep iterating mascot, command discoverability, and compact/narrow layouts |
| Typing UI | Multiline input, pending option hint, slash command status hints | Add full command palette, shell mode, mention completion, and richer history search |
| Plan mode | Generates, reviews, confirms, executes, and tracks plan steps | Enforce/read back read-only planning mode and richer execute/refine choices |
| Subagents | Built-in roles, custom registry, isolated sessions, background queue | Add normalized YAML frontmatter, permission inheritance, and skills preload |
| Permissions | Tool risk policy, approvals, approve once/session | Make global modes first-class: default, accept edits, plan, auto, deny, bypass |
| Thinking UI | Reasoning buffer and quiet transcript marker | Prefer live summarized thinking over raw dumps; keep raw reasoning opt-in |
| MCP | Stdio support plus hardening and filters | Add HTTP/SSE transports, auth flow, dynamic MCP prompts |
| Hooks | Config schema and some lifecycle concepts | Wire all key events into execution: prompt submit, pre/post tool, stop, subagent stop |
| Distribution | Dual binaries and CI smoke checks | Add installers, shell integrations, and Windows-native smoke coverage |

## Automated Test Matrix

Run these before claiming agent-parity work is stable:

```powershell
cargo fmt --all --check
cargo check --all-targets --all-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo run --bin octo -- --help
cargo run --bin octo -- preview-tui --api ready --scenario workbench --width 80 --height 24
```

Add or keep focused tests for:

- Slash commands: registry snapshot, prefix matching, aliases, unknown command,
  and command-forwarding behavior.
- TUI input: empty prompt, command/shell/memory hints, pending options, cursor
  on Unicode boundaries, and multiline input.
- TUI render smoke: welcome, workbench, slash command panel, approval popup,
  settings, diff focus, history search, file mentions, and narrow 80x24 layouts.
- Plan mode: read-only planning contract, risk-based options, preview path,
  execute path, cancelled path, and failed step cleanup.
- Subagents: custom agent parsing, allowed tool enforcement, read-only blocks,
  result metadata, background status, cancel behavior, and cleanup.
- Permissions: read/write/git/network/command matrix with Windows paths and
  protected files.
- Hooks: config parsing first, then fake hook runner coverage for allow/block
  and stop/subagent stop behavior once hooks are wired.

## Manual Smoke Tests

Use the real TUI, not a browser mock:

1. Open `octo` in 80x24 and a wide terminal.
2. Check the welcome mascot, project context, starter prompts, and input hints.
3. Type `/`, `/help`, `/status`, `/permissions`, `/agents`, `/compact`.
4. Type multiline input with Shift+Enter and verify the cursor stays readable.
5. Trigger read and write approvals; test approve once, approve session, deny.
6. Trigger plan mode; verify plan summary, options, step progress, preview,
   execute, cancel, and failure cleanup.
7. Trigger subagents; verify running, done, failed, and background task display.
8. Toggle reasoning display and confirm thinking stays quiet and opt-in.
9. Open file tree and diff viewer; confirm they do not crowd plan/subagent UI.
10. Re-run the automated gate above.
