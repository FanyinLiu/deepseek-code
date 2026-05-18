# Octocode Knowledge / Failure Memory / Skill 理论设计

状态：本地理论草案  
范围：项目知识、失败记忆、风险图、技能库  
目的：定义 Octocode 如何把一次性任务经验转化为可复用的长期能力。

## 1. 核心定位

`knowledge`、`failure-memory`、`skill` 是 Octocode 从普通 coding agent 走向自我进化 agent 的关键中间层。

它们解决的问题是：

```text
模型每次都很聪明，但系统每次都从零开始。
```

Octocode 必须把历史任务留下来，变成下一轮可使用的资产。

## 2. 三种长期资产

Octocode 的长期资产分三类：

| 资产 | 作用 | 来源 | 消费方 |
|---|---|---|---|
| Project Knowledge | 理解当前项目 | 扫描、用户规则、历史任务 | planner / repair / judge |
| Failure Memory | 避免重复失败 | 失败 run、回滚、测试失败 | repair / evolve / judge |
| Skill Library | 复用成功能力 | 成功 run、人工整理、论文机制 | planner / repair / evolve |

三者关系：

```text
Project Knowledge 让 agent 知道环境。
Failure Memory 让 agent 避免旧错误。
Skill Library 让 agent 复用成功方法。
```

## 3. Project Knowledge 理论

Project Knowledge 是项目级环境地图。

它应该回答：

- 这个项目是什么？
- 主要命令是什么？
- 核心模块在哪里？
- 哪些文件高风险？
- 哪些测试验证哪些能力？
- provider、TUI、CLI、agent、mission 等模块如何连接？
- 用户有哪些固定偏好？
- 哪些操作成本高或容易破坏状态？

建议文件：

```text
.octocode/knowledge/project.md
.octocode/knowledge/modules.json
.octocode/knowledge/commands.json
.octocode/knowledge/validation.json
```

### project.md 建议结构

```text
# Project Knowledge

## Product Identity
## Architecture Map
## Core Modules
## CLI Commands
## Provider System
## TUI System
## Agent System
## Mission System
## Validation Commands
## High-Risk Areas
## User Preferences
## Known Constraints
## Last Updated
```

## 4. Risk Map 理论

Risk Map 是 Project Knowledge 中最重要的机器可读部分。

建议文件：

```text
.octocode/knowledge/risk-map.json
```

建议结构：

```json
{
  "version": 1,
  "paths": [
    {
      "pattern": "src/policy/**",
      "risk_level": "high",
      "reason": "approval and safety policy",
      "required_gates": ["cargo_check", "full_tests", "security_scan"],
      "auto_apply_allowed": false
    }
  ]
}
```

风险等级：

| 等级 | 含义 | 策略 |
|---|---|---|
| low | 文档、注释、非核心提示 | 可低成本验证 |
| medium | 普通业务逻辑 | 需要 check 和 targeted tests |
| high | 安全、权限、密钥、命令执行、核心调度 | 需要完整门禁，不自动 apply |
| blocked | 绕过安全、隐藏失败、删除测试 | 直接拒绝 |

Risk Map 来源：

- 静态规则。
- AGENTS.md。
- 项目结构。
- 用户偏好。
- 历史失败。
- rollback 记录。
- judge 争议记录。

## 5. Failure Memory 理论

Failure Memory 是 Octocode 的负经验系统。

它不是错误日志，而是可检索的失败知识。

建议文件：

```text
.octocode/knowledge/failure-memory.jsonl
```

建议记录：

```json
{
  "id": "failure_...",
  "created_at": "...",
  "source": "repair|evolve|test|manual",
  "proposal_id": "...",
  "run_id": "...",
  "symptom": "...",
  "root_cause": "...",
  "touched_files": ["..."],
  "failed_gate": "cargo_check",
  "validation_excerpt": "...",
  "failed_patch_hash": "...",
  "avoid_pattern": "...",
  "lesson": "...",
  "tags": ["provider", "cli", "risk:high"]
}
```

### Failure Memory 使用方式

在 `propose` 阶段：

- 查找相似问题。
- 提醒 planner 不要重复旧方案。

在 `localization` 阶段：

- 提升历史相关文件优先级。

在 `patch` 阶段：

- 阻止重复失败 patch pattern。

在 `judge` 阶段：

- 对相似失败 diff 降权。

在 `skill` 阶段：

- 写入 skill 的 `when_not_to_use` 和 `failure_modes`。

## 6. Skill 理论

Skill 是成功经验的能力包。

它不是普通 prompt，也不是文档摘要。

一个 skill 必须回答：

- 什么时候用？
- 什么时候不用？
- 需要什么上下文？
- 执行步骤是什么？
- 怎么验证？
- 常见失败是什么？
- 历史效果如何？

建议目录：

```text
.octocode/skills/<skill-id>/
  SKILL.md
  metadata.json
  examples/
  tests/
  traces.jsonl
```

### SKILL.md 建议结构

```text
# Skill Name

## Purpose
## When to Use
## When Not to Use
## Required Context
## Steps
## Validation
## Examples
## Failure Modes
## Related Files
```

### metadata.json 建议结构

```json
{
  "id": "rust-cli-command-wiring",
  "version": 1,
  "created_from_run": "run_...",
  "status": "draft|active|deprecated",
  "success_count": 0,
  "failure_count": 0,
  "last_used_at": null,
  "tags": ["rust", "cli"]
}
```

## 7. Skill Promotion 理论

不是所有成功都应该变成 skill。

适合 promotion 的条件：

- 问题类型会重复。
- 操作步骤可描述。
- 验证方式稳定。
- 成功不是偶然。
- 失败边界可写清楚。
- 复用价值大于维护成本。

不适合 promotion：

- 一次性业务修改。
- 只靠特殊上下文成功。
- 无法验证。
- 风险很高但没有足够失败样本。
- 步骤过于抽象。

Promotion 流程：

```text
successful run
-> extract repeated pattern
-> draft skill
-> attach validation
-> mark draft
-> use in future run
-> update traces
-> activate or deprecate
```

## 8. Knowledge 更新策略

Knowledge 不应该每次完全重写。

推荐策略：

- append first。
- periodic compact。
- high-risk change requires explicit note。
- stale entries should be marked, not silently deleted。
- generated knowledge should include source trace。

状态：

| 状态 | 含义 |
|---|---|
| active | 当前有效 |
| stale | 可能过期 |
| disputed | judge 或测试结果有冲突 |
| deprecated | 不建议继续使用 |

## 9. 检索理论

Planner 不应该全量读取所有 knowledge 和 skills。

推荐检索顺序：

1. 根据任务关键词匹配 tags。
2. 根据 touched files 匹配 risk-map。
3. 根据错误信息匹配 failure-memory。
4. 根据模块匹配 project knowledge。
5. 根据历史成功匹配 skills。
6. 只取 top-k 摘要进入上下文。

默认上下文包：

```text
project summary
risk flags
similar failures top 3
candidate skills top 3
validation hints
```

## 10. 避免知识污染

长期记忆会带来污染风险。

风险：

- 旧知识过期。
- 单次失败被过度泛化。
- 错误 skill 被反复复用。
- 用户偏好被错误记录。
- provider 行为变化后旧经验失效。

控制：

- 每条知识有来源。
- 每条 skill 有成功/失败计数。
- failure memory 有 tags 和相似度限制。
- stale 状态显式标记。
- judge 可以质疑 knowledge。
- 用户可以删除或禁用 skill。

## 11. 与 Repair 的关系

Repair Agent 应消费：

- project knowledge。
- risk-map。
- failure-memory。
- skills。

Repair Agent 应产出：

- repair trace。
- failure-memory。
- skill candidate。
- risk-map incident。
- project knowledge update suggestion。

流程：

```text
repair propose
-> retrieve knowledge/failures/skills
-> repair run
-> validation
-> write trace
-> update failure-memory or skill candidate
```

## 12. 与 Evolve 的关系

Evolve 应消费：

- risk-map。
- failure-memory。
- repair skills。
- project knowledge。

Evolve 应产出：

- archive records。
- lineage records。
- new core repair skills。
- risk incidents。
- rollback lessons。

原则：

```text
evolve 不应绕过 knowledge/failure-memory/skill，而应该基于它们进行自改。
```

## 13. 第一版推荐范围

第一版只做：

```text
.octocode/knowledge/project.md
.octocode/knowledge/risk-map.json
.octocode/knowledge/failure-memory.jsonl
.octocode/skills/<skill-id>/SKILL.md skeleton
```

命令：

```bash
octocode knowledge update
octocode knowledge show risk-map
octocode knowledge show failures
octocode skill list
octocode skill show <skill-id>
```

暂缓：

- 自动 skill activation。
- 向量数据库。
- 大规模 memory compaction。
- 自动删除旧知识。
- 跨项目共享 skill marketplace。

## 14. 验收标准

第一版合格标准：

- 可以生成项目知识文件。
- 可以生成 risk-map。
- repair/evolve 失败可以追加 failure-memory。
- 可以列出和查看 skills。
- planner 可以读取 top-k failure/skill 摘要。
- 知识条目有来源和状态。
- 不会静默删除旧记忆。

## 15. 最终判断

Knowledge / Failure Memory / Skill 理论已经足够支持第一版实现。

推荐实现顺序：

```text
risk-map -> failure-memory -> project.md -> skill skeleton -> retrieval bundle
```

这会让 Octocode 从“每次重新思考”变成“逐步积累工程经验”。
