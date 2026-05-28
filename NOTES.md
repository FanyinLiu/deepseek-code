# Octo Optimization Notes

## Item 7: session save incrementalization

Status: not implemented.

Attempted:
- Reviewed `SessionStore::save`, `SessionStore::load`, session listing/index rebuild, CLI/TUI save call sites, and the existing session event log.
- Confirmed the current durable session source of truth is the full `session.json` snapshot plus `transcript.md`.
- Confirmed `events.jsonl` is an audit/recovery stream for visible events, unfinished turns, hooks, and swarm state; it is not a complete replay format for `Session`.

Blocker:
- `SessionStore::save` is only called after a turn with a complete `Session`, so adding append-only per-message persistence would require a new ledger boundary inside the turn or a new snapshot/replay contract.
- A safe implementation needs to define ordering between `session.json`, a new `messages.jsonl`, compaction markers, transcript regeneration, session index rebuilds, and partial-line crash recovery.
- Implementing only part of that contract would risk divergent old/new load semantics or data loss, which violates the data-safety requirement for this item.

Remaining TODO:
- Define a `messages.jsonl` record schema with monotonic sequence or turn identifiers and stable message IDs.
- Keep old `session.json` loading as the primary compatibility path.
- Add replay-after-snapshot logic for stale snapshots plus newer jsonl records.
- Regenerate `transcript.md` and index summaries from the same merged session state.
- Add tests for old-format load, new-format round trip, stale snapshot plus jsonl replay, compact-after-replay load, and corrupt final jsonl line handling.
- Decide when to compact and whether to fsync the jsonl ledger, `session.json`, or both.

## Item 8: event log flock optimization

Status: not attempted.

Reason:
- The execution sequence stops after item 7 per the safety rule, so event log buffering was not evaluated or changed in this pass.
