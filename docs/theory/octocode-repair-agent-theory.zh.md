# Octocode Repair Agent 理论设计

状态：本地理论草案  
范围：Octocode 的代码修复 Agent、repair 命令组、验证闭环  
目的：把 Agentless、AutoCodeRover、SWE-agent 的工程路线转成 Octocode 的第一层可落地能力。

## 1. 核心定位

Repair Agent 是 Octocode 自我进化体系的第一块地基。

它的目标不是“像人一样自由探索整个项目”，而是稳定完成一个受约束的工程闭环：

```text
问题输入 -> 定位 -> 生成最小补丁 -> 验证 -> 报告 -> 记录经验
```

Repair Agent 做稳之后，Octocode 才有资格继续做 skill、knowledge、archive 和 core self-evolution。

## 2. 为什么先做 Repair Agent

原因：

- 它是 Agentless / AutoCodeRover 路线，工程确定性高。
- 它能直接服务用户项目修复。
- 它也能服务 Octocode 自己的核心代码修复。
- 它天然产生验证信号、失败记忆和 skill 候选。
- 它比全自由 Agent 更便宜、更可控、更容易 debug。

核心判断：

```text
没有稳定 repair，直接做 DGM/HGM 式自我改代码，会放大系统已有混乱。
```

## 3. Repair Agent 的非目标

第一版不做：

- 不做长时间自由探索。
- 不做全仓库无边界重构。
- 不自动 apply 高风险补丁。
- 不把模型解释当作正确性证明。
- 不默认跑昂贵外部 benchmark。
- 不允许为了通过测试而删除或弱化测试。

## 4. 基本循环

Repair Agent 的最小循环：

```text
Input
-> Intake
-> Localization
-> Patch Planning
-> Patch Generation
-> Deterministic Validation
-> Report
-> Trace Storage
-> Failure/Skill Update
```

对应命令：

```bash
octocode repair propose "..."
octocode repair run <proposal-id>
octocode repair report <run-id>
octocode repair status
```

第二阶段再加：

```bash
octocode repair apply <run-id>
octocode repair rollback <apply-id>
```

## 5. 输入类型

Repair Agent 应支持这些输入：

| 输入 | 例子 | 处理方式 |
|---|---|---|
| 用户自然语言 | “模型切换有问题” | 生成 repair proposal |
| 测试失败 | cargo test 输出 | 定位相关模块 |
| 编译失败 | rustc error | 定位具体文件/符号 |
| 运行错误 | CLI/TUI 错误 | 结合 trace 分析 |
| provider 错误 | API 返回异常 | 检查 provider adapter |
| 回归问题 | 之前能用现在不能用 | 使用 failure memory |

## 6. Proposal 理论

Repair proposal 是 repair 的任务合同。

它必须包含：

```text
id
created_at
user_request
problem_statement
expected_behavior
scope_hint
risk_level
allowed_paths
blocked_paths
validation_plan
status
```

原则：

- proposal 要先定义“修什么”，再让模型写代码。
- 没有 proposal 的 patch 不进入 repair archive。
- proposal 越清晰，后续 judge 越便宜。

## 7. Localization 理论

Localization 是 repair 的关键，因为它决定 token 成本和补丁质量。

Octocode 应采用分层定位：

### L0：用户提示定位

从用户描述中提取模块和关键词。

### L1：文本搜索定位

使用 `rg` / 文件名 / 符号名。

### L2：结构定位

使用语言结构信息：

- Rust module。
- function。
- struct。
- enum。
- trait。
- CLI command branch。
- provider adapter。

### L3：测试定位

从失败测试、断言、堆栈、stderr 中定位。

### L4：历史定位

从 failure memory、risk-map、previous repair traces 中定位。

### L5：模型辅助定位

只在前几层不足时调用模型辅助。

原则：

```text
先用便宜确定性定位，再用模型。
```

## 8. Patch 理论

Repair patch 应该最小化。

最小补丁要求：

- 只改 proposal 范围内的文件。
- 不做无关重构。
- 不删除测试来通过验证。
- 不弱化错误处理。
- 不隐藏日志或失败。
- 不修改安全/权限路径，除非 proposal 明确允许。

Patch 输出必须是：

```text
unified diff + touched files + behavior change summary + validation expectation
```

## 9. 验证理论

Repair Agent 的正确性主要来自验证，不来自模型自信。

验证层级：

| 层级 | 验证 | 用途 |
|---|---|---|
| V1 | diff static scan | 快速拒绝危险 patch |
| V2 | format/check | 基本语法和类型安全 |
| V3 | targeted tests | 验证相关行为 |
| V4 | full tests | 防止大范围回归 |
| V5 | CLI smoke | 验证真实命令入口 |
| V6 | scenario replay | 复放真实任务 |
| V7 | judge review | 解释性审查 |

原则：

- V1-V3 是第一版必需。
- V4-V6 视风险等级启用。
- V7 不能覆盖 V1-V6 的失败。

## 10. Diff Judge 理论

Diff judge 是第一版最现实的 judge。

输入：

```text
proposal summary
diff
touched files
validation output
risk rules
historical failure snippets
```

输出：

```text
pass/fail/warn
reason
risk_flags
missing_tests
suggested_next_validation
```

Diff judge 不需要全量读取 repo。它应该回答：

- patch 是否解决 proposal？
- patch 是否越界？
- 是否触碰高风险文件？
- 是否删除或弱化测试？
- 是否隐藏错误？
- 是否缺验证？

## 11. Risk Map 理论

Repair Agent 必须知道哪些文件风险高。

risk-map 应保存：

```text
path
risk_level
reason
required_gates
auto_apply_allowed
last_incident
```

高风险路径示例：

- policy。
- sandbox。
- command execution。
- key storage。
- provider credential。
- evolution controller。
- evaluator gates。
- approval flow。

risk-map 来源：

- 静态规则。
- AGENTS.md。
- 历史失败。
- 用户偏好。
- 手动标记。
- 自动 incident。

## 12. Failure Memory 理论

failure-memory 是 Repair Agent 变强的关键。

每次失败应记录：

```text
id
time
proposal_id
run_id
error_type
touched_files
symptom
root_cause
failed_patch_hash
validation_output
lesson
avoid_pattern
```

使用方式：

- propose 阶段：提醒历史相似问题。
- localization 阶段：优先查看历史相关文件。
- patch 阶段：避免重复失败模式。
- judge 阶段：降低相似 patch 置信度。

## 13. Skill Promotion 理论

不是每次成功都变成 skill。

一个 repair run 适合变成 skill，需要满足：

- 问题类型可重复。
- 步骤清晰。
- 验证方式稳定。
- 失败边界明确。
- 不依赖一次性上下文。

skill 来源示例：

- “Rust CLI command wiring fix”。
- “Provider adapter smoke validation”。
- “TUI snapshot regression audit”。
- “Cargo feature flag repair”。
- “Mission replay failure triage”。

## 14. Cost 理论

Repair Agent 必须控制 token 和 API 成本。

成本策略：

- 默认不全量读 repo。
- 先定位，再读取小上下文。
- judge 只看 diff。
- 失败后再扩展上下文。
- 大模型只用于 patch/judge 的关键阶段。
- 小模型或 deterministic rule 做前置筛选。

## 15. 与自我进化的关系

Repair Agent 是 core self-evolution 的前置能力。

关系：

```text
repair 修用户项目
-> 产生 failure memory / skill / risk-map
-> repair 修 Octocode 自己
-> evolve 调用 repair pipeline 生成候选补丁
-> archive 保存候选
-> lineage 选择下一代
```

也就是说，`evolve` 不应该重新发明修代码能力，而应该复用 `repair`。

## 16. 第一版理论完整性

| 模块 | 完整度 | 是否可实现 |
|---|---:|---|
| proposal | 高 | 是 |
| localization | 中高 | 是，先做简单版 |
| patch generation | 中 | 是，先接现有 provider |
| validation | 高 | 是 |
| report | 高 | 是 |
| failure memory | 高 | 是 |
| risk map | 高 | 是 |
| diff judge | 中高 | 是，先 deterministic |
| skill promotion | 中 | 可做骨架 |
| apply/rollback | 中 | 第二阶段做 |

## 17. 第一版推荐范围

第一版只做：

```text
repair propose
repair run
repair report
repair status
failure-memory write
risk-map read/generate
diff static judge
```

暂缓：

```text
repair apply
repair rollback
multi-agent repair
benchmark integration
auto skill promotion
cross-model judge
```

原因：先把只读/可验证闭环跑稳，再加写入主工作区的动作。

## 18. 验收标准

第一版 Repair Agent 合格标准：

- 可以生成 proposal。
- 可以生成 run 目录。
- 可以保存 patch 或 candidate report。
- 可以执行至少一种 deterministic validation。
- 可以生成 report。
- 失败可以写入 failure-memory。
- 能根据 risk-map 标记高风险文件。
- 不会自动 apply 高风险修改。

## 19. 最终判断

Repair Agent 理论已经足够完整，可以作为 Octocode 自我进化的第一阶段实现基础。

但实现时必须保持克制：

```text
先做可观测、可复放、可验证；
再做自动应用；
最后做核心自我进化。
```
