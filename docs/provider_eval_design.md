# Provider Eval Design

Last verified: 2026-05-21

The provider eval is for Octocode behavior, not public benchmark ranking. It
measures whether a provider can run local coding-agent workflows through the
same command, tool, policy, and session paths users rely on.

## Eval dimensions

| Dimension | Scenario | Expected signal |
|---|---|---|
| Chinese instruction following | User asks in Chinese for a concrete repo inspection. | Response language and action plan stay Chinese unless code requires English. |
| Windows paths | User references `D:\project\src\main.rs` and PowerShell commands. | Model preserves Windows path semantics and avoids POSIX-only advice. |
| Repo search | Agent must find a symbol before answering. | Uses search/read tools before making claims. |
| Small patch | Agent edits one small bug and explains verification. | Patch is minimal, compiles, and does not touch unrelated files. |
| Tool loop | Model receives tool results, then makes the next call or final answer. | No lost tool-call IDs; assistant/tool protocol stays valid. |
| Thinking preservation | Provider emits reasoning fields during tool loops. | Internal reasoning is preserved only while needed and not rendered as user text. |
| JSON output | CLI final output must be one valid JSON object in JSON mode. | Machine contract is stable even on preflight failure. |
| Streaming | Stream-json emits structured error/content events. | No raw provider event leakage. |
| Context budget | Large input approaches provider context limit. | Octocode reports true provider/local budget and compact threshold. |
| Provider diagnostics | Missing key or bad model is diagnosed. | Doctor output names provider, env key, model, and likely fix. |

## First product milestone

- Keep existing `evals/baseline_cases.jsonl` as the cheap always-on CLI contract suite.
- Add provider-oriented cases that do not require real API keys: `models --json`, provider settings, and doctor missing-key output.
- Keep real API evals manual/opt-in until provider credentials are available in CI.

## Manual smoke matrix

For each P0 provider with a local API key configured:

1. `octo models --json`
2. `octo settings set provider.default <provider>`
3. `octo doctor`
4. `octo doctor --smoke`
5. One simple `octo chat --output-format json`
6. One repo-local coding task in a disposable fixture repo

Record provider, model, pass/fail, error class, token usage, latency, and whether reasoning/tool-call state was preserved.
