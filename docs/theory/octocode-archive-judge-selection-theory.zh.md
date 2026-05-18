# Octocode Archive / Lineage / Judge / Selection 理论设计

状态：本地理论草案  
范围：自我进化候选档案、谱系、评审器、选择函数  
目的：定义 Octocode 如何从多个候选修改中选择真正值得保留、复用和演化的版本。

## 1. 核心定位

`archive / lineage / judge / selection` 是 Octocode 从“能修代码”走向“能持续自我进化”的核心层。

Repair 解决的是：

```text
这次怎么修好？
```

Archive 和 Selection 解决的是：

```text
哪些修改值得进入长期演化？
下一轮应该从哪个候选继续生长？
哪些失败必须被记住，避免再次发生？
```

## 2. 基本原则

### 原则 1：Archive 保存所有候选，不只保存成功候选

失败候选、被拒绝候选、回滚候选、judge 分歧候选都必须保留。

原因：

- 成功项用于复用。
- 失败项用于避坑。
- 回滚项用于识别真实世界风险。
- 分歧项用于校准 judge。
- 全部候选共同形成 lineage 数据。

### 原则 2：Lineage 是长期记忆，不是日志装饰

Lineage 必须能回答：

- 这个候选从哪个 parent 来？
- 改了什么？
- 为什么通过或失败？
- 后代表现如何？
- 哪一支谱系更有继续进化潜力？

### 原则 3：Judge 只能提供证据，不能替代硬门禁

模型 judge 不能覆盖：

- 编译失败。
- 测试失败。
- blocked pattern。
- policy hard gate。
- 无 checkpoint 的核心 apply。

### 原则 4：Selection 必须多目标

不能只看当前分数。选择函数要同时考虑：

- 质量提升。
- 安全性。
- 可维护性。
- 成本。
- 延迟。
- 可回滚性。
- 后代潜力。
- 用户价值。

## 3. Archive 理论

Archive 是候选修改的长期档案。

建议目录：

```text
.octocode/evolution/archive/
  candidates/
    <candidate-id>.json
  lineage.jsonl
  scores.jsonl
  judge-events.jsonl
  incidents.jsonl
```

Candidate 记录建议结构：

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

## 4. Lineage 理论

Lineage 是候选之间的演化关系。

它应该支持：

- parent-child 追踪。
- 多分支候选。
- 回滚记录。
- 后代表现聚合。
- clade-level 分数。

Lineage event 建议结构：

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

## 5. Judge 理论

Octocode 至少需要六类 judge。

| Judge | 类型 | 作用 |
|---|---|---|
| Rule Judge | deterministic | 检查路径、blocked pattern、安全规则 |
| Diff Judge | deterministic/LLM | 检查补丁是否越界、是否解决 proposal |
| Test Judge | deterministic | 编译、测试、场景回放 |
| Cost Judge | deterministic | 计算 token、API 成本、耗时 |
| Historical Judge | deterministic/LLM | 对照 failure memory 和 rollback 记录 |
| Cross-model Judge | LLM | 用不同模型审查关键补丁 |

Judge 输出必须结构化：

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

## 6. Judge 优先级

Judge 不是平等投票。

优先级：

```text
blocked rule > deterministic gate failure > test failure > rollback history > diff warning > model judge approval
```

也就是说：

- blocked rule 一票否决。
- 编译/测试失败不能被模型语言解释覆盖。
- 模型 judge 只能提高或降低人工/后续验证优先级。
- 高风险修改必须通过硬门禁。

## 7. Selection 理论

Selection 决定哪个候选被应用、进入 active skill、或成为下一轮 parent。

基础 utility：

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

来自测试、benchmark、真实任务成功率。

### safety_delta

来自 risk-map、policy gate、历史 incident。

### maintainability_delta

来自 diff 大小、复杂度、耦合度、是否引入重复逻辑。

### skill_reuse_value

候选是否产生可复用 skill 或改善已有 skill。

### lineage_potential

候选后代是否持续产出高质量候选，对应 HGM 的 clade-metaproductivity。

## 8. Clade-Metaproductivity 理论

当前分数高不等于后代潜力高。

Octocode 应记录一个候选谱系的后代产出能力：

```text
clade_score = descendants_pass_rate
            + average_quality_delta
            + average_reuse_value
            - average_cost
            - incident_penalty
```

用途：

- 选择下一轮 parent。
- 发现短期分数普通但长期潜力高的分支。
- 降低高分但高风险分支权重。

## 9. 多候选策略

对于同一 proposal，可以生成多个候选：

- deterministic candidate。
- model A candidate。
- model B candidate。
- skill-guided candidate。
- minimal patch candidate。

选择时不直接比语言解释，而比：

- diff 范围。
- gate 结果。
- 风险等级。
- 成本。
- 历史相似失败。
- 可维护性。

## 10. 与 Repair 的关系

Repair 产出候选，Archive 保存候选。

流程：

```text
repair run
-> candidate patch
-> validation
-> candidate record
-> judge events
-> archive
-> optional skill promotion
```

Repair 第一版可以先不使用复杂 selection，但必须提前保存足够数据。

## 11. 与 Evolve 的关系

Evolve 是 archive/lineage/selection 的主要消费者。

流程：

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

Evolve 不应该只保存最后应用的版本，而应该保存完整候选族谱。

## 12. 安全边界

这些情况必须直接拒绝或升级人工确认：

- 删除测试。
- 修改 policy/sandbox 以降低门禁。
- 隐藏失败或日志。
- 移除 approval。
- 读取或泄露密钥。
- 无 checkpoint 修改核心代码。
- 修改 judge 让自己通过。
- 修改 risk-map 降低自身风险等级。

## 13. 第一版推荐范围

第一版只做：

```text
candidate archive
lineage events
rule judge
diff judge
test judge summary
cost summary
basic utility score
```

暂缓：

```text
full clade search
long-running autonomous evolution
cross-provider judge by default
automatic high-risk apply
复杂 benchmark 排行榜
```

## 14. 验收标准

第一版合格标准：

- 每个 repair/evolve candidate 都有 archive 记录。
- 每个 candidate 都有 parent/proposal/run 关联。
- 成功、失败、拒绝都能保存。
- judge 输出结构化。
- deterministic gate 失败不能被模型 judge 覆盖。
- selection score 可解释。
- lineage 可以展示基本 parent-child 关系。

## 15. 最终判断

Archive / Lineage / Judge / Selection 理论足够支持数据结构和第一版门禁实现。

但 HGM/DGM 式长期自动谱系搜索必须等 repair、knowledge、failure-memory 和基础 archive 跑出真实数据后再做。
