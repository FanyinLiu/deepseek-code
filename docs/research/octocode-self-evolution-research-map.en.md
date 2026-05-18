# Octocode Self-Evolution Research Map

> Date: 2026-05-17  
> Goal: Build a research and implementation map for Octocode's ability to improve its own core code.  
> Thesis: Octocode should not aim for unconstrained AGI. It should become a constrained self-improving engineering system: models propose candidates, executable gates judge evidence, low-risk changes can be automated, and high-risk changes are quarantined.

## 1. Core Problem

The desired capability is not a normal coding agent. It is an engineering version of recursive self-improvement:

```text
observe its own behavior
-> identify failures and capability gaps
-> create an improvement proposal
-> generate candidate patches
-> run tests and safety gates
-> apply or quarantine
-> rollback on failure
-> consolidate lessons into rules, tests, skills, or code
```

The key risk is allowing the same model to generate code, define the objective, judge correctness, and approve deployment. That creates self-confirmation bias, reward hacking, test deletion, and safety bypass incentives.

Octocode's baseline principle should be:

```text
Models may propose evolution, but they must not approve evolution alone.
```

## 2. Research Threads

| Direction | Representative Work | Core Idea | Implication for Octocode |
|---|---|---|---|
| Reflective improvement | Reflexion | Convert failure feedback into verbal memory instead of updating weights. | Store failed runs, rollbacks, interruptions, and test failures as evolution memory. |
| Self-feedback refinement | Self-Refine | Use the model to generate, critique, and refine outputs at test time. | Useful for low-risk docs, prompts, agent specs; not sufficient as final core-code judge. |
| Skill library growth | Voyager | Automatic curriculum, executable skill library, environment feedback, self-verification. | Successful workflows should become skills, agents, and test recipes, not only core code patches. |
| Software engineering agents | SWE-agent | Solve real GitHub issues through tool use and executable validation. | `evolve` should rely on real repo state, real tests, and real CLI smoke runs. |
| Minimal SWE repair | Agentless | Solve SWE tasks with retrieval, localization, repair, and validation instead of a large agent framework. | Self-evolution does not require a huge swarm; precise localization and validation matter more. |
| Self-editing agents | A Self-Improving Coding Agent | An agent edits its own code and validates performance on benchmarks. | Octocode can edit itself, but every candidate must remain benchmarked and rollbackable. |
| Open-ended self-improvement | Darwin Gödel Machine | Maintain an archive of agents, sample old agents, generate new versions, validate with benchmarks. | Octocode should keep an evolution archive instead of a single linear latest version. |
| Evolutionary code search | AlphaEvolve | LLMs generate programs; automated evaluators verify and select promising variants. | Put evaluators above generators; executable evidence beats model confidence. |
| Open-source AlphaEvolve-like systems | OpenEvolve | Open evolutionary coding agent with prompt constraints and program database. | Borrow program database, prompt stochasticity, and domain-specific evaluator ideas. |
| Experience lifecycle | EvolveR / CodeEvolve style systems | Runtime experience drives target selection and code optimization. | Octocode should select future evolution targets from logs and failure events. |

## 3. Related Project Map

### 3.1 yoyo / yoyo-evolve

- Project: `yologdev/yoyo-evolve`
- Positioning: A public experiment in a self-evolving coding agent that started from an approximately 200-line CLI.
- Mechanism: Each session chooses one improvement, edits its own source, runs build/tests, writes a journal, and grows publicly.
- Key lessons:
  - Self-evolution needs a narrative journal and growth trace; otherwise long-term trend is hard to judge.
  - One focused change per session is more stable than broad self-modification.
  - Community issues provide external reality checks and reduce self-optimization around hallucinated goals.
- Octocode stance:
  - Borrow journal, CI, and one-change-per-session discipline.
  - Do not copy the pure growth-experiment shape; Octocode is a user-facing tool and needs stronger gates, rollback, and risk tiers.

### 3.2 EvoMap / Evolver

- Positioning: A GEP-powered self-evolving engine with Genes, Capsules, Events, and auditable evolution.
- Key lessons:
  - Evolution units should be auditable, versioned, and replayable.
  - Evolution does not always need to directly edit source code; it can first produce prompts, genes, or capsules.
- Octocode stance:
  - Borrow event streams and evolution-gene representation.
  - Keep source patches as the final verifiable artifact.

### 3.3 Hermes Agent Self-Evolution

- Positioning: Skill-file evolution rather than direct core-runtime modification.
- Key lessons:
  - Skill-level self-improvement is lower risk than core code evolution.
  - Prompts, agents, workflows, and skills should be optimized before core code.
- Octocode stance:
  - Add a skill/agent evolution lane.
  - Keep the core-code lane stricter.

### 3.4 OpenEvolve / AlphaEvolve-like Systems

- Positioning: LLMs produce program variants; evaluators select better variants.
- Key lessons:
  - Evaluators matter more than generators.
  - A program database avoids getting stuck on one bad trajectory.
  - Model ensembles can improve both breadth and depth.
- Octocode stance:
  - Do not feed the full repository to the model on every run.
  - Feed only proposal, diff, target excerpt, and test results.
  - Build a run archive and candidate patch archive.

## 4. Recommended Octocode Architecture

### 4.1 Five Layers of Self-Improvement

1. Configuration: provider, model, context, routing, and test-matrix tuning.
2. Skills: agents, skills, workflows, and prompt recipes.
3. Peripheral code: docs, tests, examples, provider adapters, UI polish.
4. Core code: orchestrator, tools, policy, provider, and TUI runtime.
5. Objective function: the definition of “better”.

Default automation boundary:

```text
automatable: configuration, skills, low-risk peripheral code
semi-automated: normal feature code
quarantined by default: core code
never auto-apply: policy / defense / keyring / command dispatch / approval bypass
```

### 4.2 Evolution Loop

```text
Observe
  Collect failed tasks, rollbacks, test failures, token waste, interruptions, and UI friction.

Diagnose
  Classify issues as model, context, tool, test, UI, or provider problems.

Propose
  Create an evolution proposal with goal, scope, risk, and expected metric.

Generate
  Use local rules plus model planner/implementer/safety reviewer to generate candidate patches.

Verify
  Run path rules, compilation, test matrix, safety scans, CLI smoke, and adversarial review.

Apply
  Auto-apply low-risk changes; quarantine core changes; rollback failed applications.

Consolidate
  Write successful patterns into tests, docs, rules, skills, or routing heuristics.
```

### 4.3 Avoid Token-Heavy Judging

The test matrix should not mean sending the whole repository to a model. Most gates are zero-token:

- `cargo check`
- `cargo test`
- clippy / fmt
- patch path scan
- protected path scan
- secret scan
- approval bypass scan
- CLI smoke
- TUI snapshot

Models should only see compact context:

```text
proposal + diff + touched file excerpt + test result + constitution
```

### 4.4 Avoid Same-Model Self-Confirmation

- The generator cannot be the final judge.
- The judge should not see the generator's explanation; only spec, diff, and evidence.
- Prefer falsification: ask “how can this patch fail?” rather than “is this patch correct?”
- Prefer heterogeneous models: Qwen generates, Kimi/DeepSeek reviews, or the reverse.
- Final admission should be based on executable evidence.

## 5. Draft Octocode Constitution

1. The model is not the final judge.
2. Every core evolution must be testable, rollbackable, and auditable.
3. Evolution must not bypass approval, sandbox, keyring, or protected paths.
4. Evolution must not improve metrics by deleting tests, weakening safety, or hiding errors.
5. Low-risk changes may be auto-applied; high-risk changes must be quarantined.
6. Evolution must serve multi-model, multi-agent, multi-capability, and self-verifying behavior.
7. Every automated change must leave proposal, patch, gate, apply, and rollback records.
8. Changes to the objective function or constitution itself are highest-risk changes.

## 6. Octocode Implementation Roadmap

### Phase A: Research and Documentation

- Build paper and related-project indexes.
- Convert each paper into Octocode capabilities.
- Establish the `Octocode Constitution`.

### Phase B: Automatic Test Matrix

- Select tests from touched files:
  - `src/provider/**` -> provider tests + API mock
  - `src/evolution/**` -> evolution CLI tests
  - `src/policy/**` / `src/defense/**` -> security tests
  - `src/tui/**` -> TUI snapshot / preview smoke
- Add `evolve test --matrix auto`.

### Phase C: Heterogeneous Judging

- Add `evolve judge`.
- Support `--reviewer-provider qwen/kimi/deepseek/openrouter`.
- Judge only checks falsification, missing tests, and constitution violations.

### Phase D: Autopilot Tiers

- `tier low`: auto-apply docs/tests/examples.
- `tier medium`: auto-apply normal feature code with rollback.
- `tier core-safe`: allow core code except safety-boundary files.
- `tier blocked`: never auto-apply policy/defense/keyring/dispatch changes.

### Phase E: Evolution Memory

- Save features of successful and failed runs.
- Write failure modes into a counterexample library.
- Write successful patterns into skills, tests, and routing rules.

## 7. Recommended Reading Order

1. Reflexion: verbal feedback and episodic memory.
2. Self-Refine: test-time self-feedback and its limits.
3. Voyager: skill library and lifelong learning.
4. SWE-agent / Agentless: real software engineering tasks and tool interfaces.
5. A Self-Improving Coding Agent: self-editing agent code.
6. Darwin Gödel Machine: archive-based open-ended self-improvement.
7. AlphaEvolve / OpenEvolve: evaluators, program databases, and evolutionary search.
8. yoyo-evolve: public growth, self-journaling, and CI-constrained evolution.
9. EvoMap / Hermes: skill-level and auditable evolution.

## 8. References

- Reflexion: https://arxiv.org/abs/2303.11366
- Self-Refine: https://arxiv.org/abs/2303.17651
- Voyager: https://arxiv.org/abs/2305.16291
- SWE-agent: https://github.com/princeton-nlp/SWE-agent
- Agentless: https://arxiv.org/search/?query=Agentless+software+engineering+agent&searchtype=all
- A Self-Improving Coding Agent: https://arxiv.org/abs/2504.15228
- Darwin Gödel Machine: https://arxiv.org/abs/2505.22954
- AlphaEvolve: https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/
- OpenEvolve: https://github.com/algorithmicsuperintelligence/openevolve
- EvoMap Evolver: https://github.com/EvoMap/evolver
- yoyo-evolve: https://github.com/yologdev/yoyo-evolve
- yoyo public journal: https://yologdev.github.io/yoyo-evolve/
