# Octocode 自我进化项目报告

状态：本地阶段报告  
分支：feature/core-evolution-engine  
范围：自我进化理论、架构、路线图、Phase A/B/C/D 最小实现  
说明：当前代码仅保存在本机本分支，未上传。

## 1. 总体结论

Octocode 自我进化方向已经从理论讨论进入可执行工程阶段。

当前已经完成三件关键事情：

1. 理论体系已经足够支撑第一阶段实现。
2. 项目内已经落地 repair / knowledge / failure-memory / skill / archive 的最小闭环。
3. 当前实现保持安全边界：不自动 apply、不自动改核心高风险代码、不绕过测试和风险门禁。

当前系统已经具备一个最小自我进化外壳：

```text
repair proposal
-> risk-map
-> context-bundle
-> deterministic validation
-> report
-> failure-memory
-> archive candidate
-> lineage event
-> skill draft
```

这不是完整 AGI，但已经是工程型自我改进系统的第一层基础。

## 2. 已完成的文档体系

### 2.1 研究与理论文档

已完成完整中英双语研究和理论材料：

- `docs/research/octocode-self-evolution-research-map.zh.md`
- `docs/research/octocode-self-evolution-research-map.en.md`
- `docs/theory/octocode-self-evolution-theory.zh.md`
- `docs/theory/octocode-self-evolution-theory.en.md`
- `docs/theory/octocode-repair-agent-theory.zh.md`
- `docs/theory/octocode-repair-agent-theory.en.md`
- `docs/theory/octocode-knowledge-skill-theory.zh.md`
- `docs/theory/octocode-knowledge-skill-theory.en.md`
- `docs/theory/octocode-archive-judge-selection-theory.zh.md`
- `docs/theory/octocode-archive-judge-selection-theory.en.md`

### 2.2 架构与路线图

已完成：

- `docs/design/octocode-self-evolution-architecture.zh.md`
- `docs/design/octocode-self-evolution-architecture.en.md`
- `docs/roadmap/octocode-self-evolution-roadmap.zh.md`
- `docs/roadmap/octocode-self-evolution-roadmap.en.md`

### 2.3 总整合文档

已完成：

- `docs/octocode-self-evolution-complete.md`

该文档是当前理论、研究、架构、路线图的单一入口。

## 3. 当前已实现能力

### 3.1 Repair Agent 最小闭环

新增命令：

```bash
octocode repair propose "..."
octocode repair run <proposal-id>
octocode repair report <run-id>
octocode repair status
```

能力：

- 创建 repair proposal。
- 生成 proposal markdown 和 JSON。
- 创建 repair run。
- 执行 deterministic validation。
- 生成 diff judge 结果。
- 生成 report。
- 不自动应用补丁。

相关文件：

- `src/repair/mod.rs`
- `src/cli/repair.rs`

### 3.2 Knowledge / Risk Map / Failure Memory

新增命令：

```bash
octocode knowledge update
octocode knowledge show project
octocode knowledge show risk-map
octocode knowledge show failures
```

能力：

- 生成 `.octocode/knowledge/project.md`。
- 生成 `.octocode/knowledge/risk-map.json`。
- 写入 `.octocode/knowledge/failure-memory.jsonl`。
- 在 repair propose 阶段检索相似失败。

相关文件：

- `src/knowledge/mod.rs`
- `src/cli/knowledge.rs`

### 3.3 Context Bundle

repair proposal 现在会生成上下文包：

```text
.octocode/repair/proposals/<proposal-id>.context-bundle.json
```

内容包括：

- `risk_findings`
- `similar_failures`
- `candidate_skills`

意义：

- 后续 patch/judge 不需要全量读取历史。
- 为 token-light evaluation 打基础。
- 为 skill/failure-memory 复用打基础。

### 3.4 Skill Skeleton / Skill Promotion

新增命令：

```bash
octocode skill list
octocode skill show <skill-id>
octocode skill add <repair-run-id> --name <name>
octocode skill test <skill-id>
```

能力：

- 从 repair run 生成 draft skill。
- 写入 `.octocode/skills/<skill-id>/SKILL.md`。
- 写入 `metadata.json`。
- 写入 `traces.jsonl`。
- 支持 skill list/show/test。
- repair propose 可以召回相关 skill。

相关文件：

- `src/skill/mod.rs`
- `src/cli/skill.rs`

### 3.5 Archive / Lineage / Utility Score

新增命令：

```bash
octocode archive status
octocode archive list
octocode archive show <candidate-id>
octocode archive lineage
```

能力：

- repair run 自动写入 archive candidate。
- repair run 自动写入 lineage event。
- archive candidate 有基础 utility score。
- 支持查看 candidate 列表、详情和 lineage。

相关文件：

- `src/archive/mod.rs`
- `src/cli/archive.rs`

## 4. 当前数据目录结构

当前实现会生成这些本地运行资产：

```text
.octocode/
  knowledge/
    project.md
    risk-map.json
    failure-memory.jsonl
  repair/
    proposals/
      <proposal-id>.json
      <proposal-id>.md
      <proposal-id>.context-bundle.json
    runs/
      <run-id>/
        run.json
        diff-judge.json
        patch.diff
        report.md
  skills/
    <skill-id>/
      SKILL.md
      metadata.json
      traces.jsonl
      examples/
      tests/
  evolution/
    archive/
      candidates/
        <candidate-id>.json
      lineage.jsonl
```

## 5. 已验证结果

已经执行并通过：

```bash
cargo check --all-targets --all-features
cargo test --test repair_knowledge_cli_tests --all-features
```

结果：

```text
repair_knowledge_cli_tests: 5 passed
```

覆盖场景：

- knowledge update 写入 project/risk-map。
- repair propose 写入 proposal 和 context-bundle。
- repair run 写入 report，且不自动应用 patch。
- blocked repair run 写入 failure-memory。
- repair run 可以生成 skill 草案。
- archive status/list/show/lineage 可读取 repair run 产生的 candidate 和 lineage。

尚未执行：

- 全量 `cargo test --all-targets --all-features`。
- clippy。
- 真实 API 调用。
- 真实 TUI 动态验证。

## 6. 当前安全边界

当前实现刻意没有做这些危险动作：

- 不自动 apply repair patch。
- 不自动 rollback。
- 不自动修改 policy/sandbox/approval/keyring。
- 不允许 judge 覆盖 deterministic gate。
- 不做长期无人监督 lineage search。
- 不默认调用真实模型 API。
- 不上传 GitHub。

这是正确的边界。当前阶段应该优先保证系统可观测、可复放、可验证。

## 7. 当前成熟度判断

| 能力 | 状态 | 判断 |
|---|---|---|
| 理论体系 | 完成 | 足够支撑 Phase A-D 最小实现 |
| repair proposal | 完成 | 可继续扩展 patch generation |
| deterministic validation | 初版完成 | 需要 targeted test selection |
| failure-memory | 初版完成 | 需要相似度和摘要优化 |
| risk-map | 初版完成 | 需要更多项目感知规则 |
| context-bundle | 初版完成 | 可作为 patch/judge 输入 |
| skill skeleton | 初版完成 | 需要 skill activation 策略 |
| archive/lineage | 初版完成 | 需要 selection 和 parent choice |
| utility score | 初版完成 | 需要多目标评分增强 |
| auto apply | 未做 | 暂时不应做高风险自动 apply |

## 8. 之后计划如何执行

建议继续按 4 个阶段推进。

### Phase 1：把 repair 从“记录/验证”推进到“生成候选 patch”

目标：让 repair run 能生成候选补丁，但仍不自动 apply。

要做：

1. 在 `repair run` 中增加 patch generation 模式。
2. 支持 deterministic candidate 和 model candidate。
3. model candidate 使用现有 provider 系统。
4. patch 必须是 unified diff。
5. patch 必须限制在 proposal allowed_paths 内。
6. patch 写入 `patch.diff`，但不应用。
7. diff judge 对 patch 做越界检查。

验收：

- `repair run <proposal-id>` 能生成 patch.diff。
- patch 不会自动应用到工作区。
- 越界 patch 被拒绝或标记失败。
- archive candidate 记录 patch hash。

### Phase 2：增强 validation 和 judge

目标：让系统更可靠地判断 patch 是否值得进入 archive。

要做：

1. 增加 patch path parser。
2. 增加 test deletion scan。
3. 增加 safety regression scan。
4. 增加 targeted validation plan。
5. 增加 diff judge 的 fail/warn/pass 原因。
6. 把 validation 输出写进 archive candidate。

验收：

- 删除测试的 patch 被拒绝。
- 修改 high-risk 文件会要求 manual review。
- deterministic gate fail 不能被 judge 覆盖。

### Phase 3：skill 和 failure-memory 真正参与 repair

目标：从“能召回”变成“能影响计划”。

要做：

1. repair proposal markdown 明确列出召回的 skill/failure。
2. patch generation prompt 消费 context-bundle。
3. skill 增加 `when_not_to_use` 结构化字段。
4. failure-memory 增加 avoid pattern 匹配。
5. 成功复用 skill 后更新 traces。

验收：

- 相似问题会显示历史失败提醒。
- 相关 skill 会进入 repair run 报告。
- skill 使用成功/失败计数可更新。

### Phase 4：进入受限 self-evolution

目标：让 `evolve` 复用 repair pipeline，而不是自己重新生成补丁。

要做：

1. evolve proposal 可以转成 repair proposal。
2. evolve patch 调用 repair pipeline 生成候选 patch。
3. evolve archive 使用统一 archive store。
4. lineage 记录 parent/child。
5. 增加 basic parent selection。

验收：

- evolve 和 repair 共享 archive/lineage。
- evolve 候选也有 utility score。
- high-risk evolve 仍不能自动 apply。

## 9. 推荐下一步

最推荐马上做：

```text
Phase 1: repair patch generation without apply
```

原因：

- 这是从“报告系统”变成“真正修复系统”的关键一步。
- 风险可控，因为 patch 只写入 `patch.diff`，不应用。
- 后续 validation、skill、archive 都依赖真实 patch。

具体下一步任务：

1. 给 `repair run` 增加 `--model` 或 `--model-patch`。
2. 接入现有 provider。
3. 生成 unified diff。
4. 限制 touched files。
5. 写入 `patch.diff`。
6. archive 记录 patch hash。
7. 新增 CLI 测试。

## 10. 当前最终判断

Octocode 自我进化项目目前处在正确阶段：

```text
理论完整 -> 第一层工程闭环完成 -> 可以开始生成真实候选 patch
```

不要继续无限扩理论。下一步应该进入真实 repair patch generation，但继续保持“不自动应用”的安全边界。
