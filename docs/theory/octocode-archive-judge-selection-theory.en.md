# Octocode Archive / Lineage / Judge / Selection Theory

Status: local theory draft  
Scope: self-evolution candidate archives, lineage, judges, and selection functions  
Purpose: define how Octocode chooses which candidate changes are worth preserving, reusing, and evolving.

## 1. Core Position

`archive / lineage / judge / selection` is the layer that moves Octocode from "can repair code" to "can continuously self-evolve".

Repair answers:

```text
How do we fix this task now?
```

Archive and Selection answer:

```text
Which changes deserve long-term preservation?
Which candidate should the next generation grow from?
Which failures must be remembered to avoid repetition?
```

## 2. Core Principles

### Principle 1: Archive all candidates, not only successful candidates

Failed, rejected, rolled-back, and judge-disagreement candidates must be preserved.

Why:

- Successes enable reuse.
- Failures prevent repeated mistakes.
- Rollbacks reveal real-world risk.
- Disagreements calibrate judges.
- All candidates together form lineage data.

### Principle 2: Lineage is long-term memory, not log decoration

Lineage must answer:

- Which parent produced this candidate?
- What changed?
- Why did it pass or fail?
- How did descendants perform?
- Which branch has stronger future evolution potential?

### Principle 3: Judges provide evidence, not permission to bypass gates

Model judges cannot override:

- Build failure.
- Test failure.
- Blocked patterns.
- Policy hard gates.
- Core apply without checkpoint.

### Principle 4: Selection must be multi-objective

Do not select only by current score. The function must consider:

- Quality improvement.
- Safety.
- Maintainability.
- Cost.
- Latency.
- Rollback safety.
- Descendant potential.
- User value.

## 3. Archive Theory

Archive is the long-term record of candidate changes.

Suggested directory:

```text
.octocode/evolution/archive/
  candidates/
    <candidate-id>.json
  lineage.jsonl
  scores.jsonl
  judge-events.jsonl
  incidents.jsonl
```

Suggested candidate record:

```json
{
  "id": "candidate_...",
  "created_at": "...",
  "source": "repair|evolve|skill|manual",
  "parent_id": "candidate_...",
  "proposal_id": "proposal_...",
  "run_id": "run_...",
  "patch_hash": "sha256:...",
  "touched_files": ["..."],
  "risk_level": "medium",
  "status": "passed|failed|rejected|rolled_back|disputed",
  "gate_summary": {},
  "judge_summary": {},
  "score_summary": {},
  "cost_summary": {},
  "failure_reason": null
}
```

## 4. Lineage Theory

Lineage is the evolution relationship between candidates.

It should support:

- Parent-child tracking.
- Multiple branches.
- Rollback records.
- Descendant performance aggregation.
- Clade-level scores.

Suggested lineage event:

```json
{
  "event": "candidate_created|candidate_tested|candidate_applied|candidate_rolled_back|candidate_rejected",
  "time": "...",
  "candidate_id": "candidate_...",
  "parent_id": "candidate_...",
  "proposal_id": "proposal_...",
  "summary": "..."
}
```

## 5. Judge Theory

Octocode needs at least six judges.

| Judge | Type | Role |
|---|---|---|
| Rule Judge | deterministic | checks paths, blocked patterns, safety rules |
| Diff Judge | deterministic/LLM | checks scope and proposal fit |
| Test Judge | deterministic | build, tests, scenario replay |
| Cost Judge | deterministic | tokens, API cost, duration |
| Historical Judge | deterministic/LLM | compares failure memory and rollbacks |
| Cross-model Judge | LLM | reviews important patches with a different model |

Judge output must be structured:

```json
{
  "judge": "diff",
  "verdict": "pass|warn|fail",
  "confidence": 0.82,
  "reasons": ["..."],
  "risk_flags": ["..."],
  "required_next_gates": ["..."]
}
```

## 6. Judge Priority

Judges are not equal votes.

Priority:

```text
blocked rule > deterministic gate failure > test failure > rollback history > diff warning > model judge approval
```

Meaning:

- Blocked rules veto.
- Build/test failures cannot be overridden by model explanations.
- Model judges can only adjust review priority or suggest more validation.
- High-risk changes must pass hard gates.

## 7. Selection Theory

Selection decides which candidate is applied, promoted to an active skill, or used as the next parent.

Base utility:

```text
U(candidate) = quality_delta
             + safety_delta
             + maintainability_delta
             + user_value
             + skill_reuse_value
             + lineage_potential
             - cost
             - latency
             - risk_penalty
             - rollback_complexity
```

### quality_delta

From tests, benchmarks, and real task success rate.

### safety_delta

From risk map, policy gates, and historical incidents.

### maintainability_delta

From diff size, complexity, coupling, and duplicated logic.

### skill_reuse_value

Whether the candidate creates a reusable skill or improves an existing skill.

### lineage_potential

Whether descendants of this candidate keep producing strong candidates. This maps to HGM's clade-metaproductivity idea.

## 8. Clade-Metaproductivity Theory

High current score does not imply high descendant potential.

Octocode should track a branch's descendant productivity:

```text
clade_score = descendants_pass_rate
            + average_quality_delta
            + average_reuse_value
            - average_cost
            - incident_penalty
```

Uses:

- Select the next parent.
- Discover branches with modest short-term score but high long-term potential.
- Down-rank high-score but high-risk branches.

## 9. Multi-Candidate Strategy

One proposal may generate multiple candidates:

- Deterministic candidate.
- Model A candidate.
- Model B candidate.
- Skill-guided candidate.
- Minimal patch candidate.

Selection compares not explanations, but:

- Diff scope.
- Gate results.
- Risk level.
- Cost.
- Similar historical failures.
- Maintainability.

## 10. Relationship to Repair

Repair produces candidates. Archive preserves candidates.

Flow:

```text
repair run
-> candidate patch
-> validation
-> candidate record
-> judge events
-> archive
-> optional skill promotion
```

Repair v1 does not need complex selection, but it must preserve enough data for later selection.

## 11. Relationship to Evolve

Evolve is the main consumer of archive, lineage, and selection.

Flow:

```text
select parent
-> generate self-improvement proposal
-> create candidates
-> run gates
-> judge
-> archive
-> apply or reject
-> update lineage score
```

Evolve should not save only the applied version. It should save the full candidate family.

## 12. Safety Boundary

These cases must be rejected or escalated:

- Deleting tests.
- Modifying policy/sandbox to weaken gates.
- Hiding failures or logs.
- Removing approvals.
- Reading or leaking secrets.
- Modifying core code without a checkpoint.
- Modifying judges to pass itself.
- Modifying risk map to lower its own risk level.

## 13. Recommended First Version Scope

First version should include:

```text
candidate archive
lineage events
rule judge
diff judge
test judge summary
cost summary
basic utility score
```

Defer:

```text
full clade search
long-running autonomous evolution
cross-provider judge by default
automatic high-risk apply
complex benchmark leaderboard
```

## 14. Acceptance Criteria

First version acceptance:

- Every repair/evolve candidate has an archive record.
- Every candidate links to parent/proposal/run.
- Success, failure, and rejection are all preserved.
- Judge output is structured.
- Deterministic gate failures cannot be overridden by model judges.
- Selection score is explainable.
- Lineage can show basic parent-child relationships.

## 15. Final Judgment

The Archive / Lineage / Judge / Selection theory is complete enough for first-version data structures and gates.

HGM/DGM-style long-running autonomous lineage search should wait until repair, knowledge, failure memory, and basic archive have real data.
