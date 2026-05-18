# 深度汇总报告：Octocode UI 与竞品对齐

**生成时间**：2026-05-15
**仓库路径**：`/Users/klein/octocode/octocode`

本文件整合了本次你要求的完整调研与本地改造结果，覆盖：开源/闭源竞品对照、当前 Octocode 模块实现差距、已完成 UI 改造、待办清单。

---

## 一、汇总结论

1. Octocode 已具备的基础能力较强：Rust TUI、Subagent/Swarm、任务/事件持久化、policy 与 approvals、MCP stdio。
2. 最大短板集中在：
   - 命令发现与可达性（尤其 `/` 面板、命令分组和过滤）
   - 首屏可操作入口（第一次看到命令时要知道“现在该打什么”）
   - TUI 交互细节的可解释性（权限来源、工具来源、状态来源）
3. 已完成价值改进：
   - 输入行可提示（`/help` 等）和候选项展示
   - 状态栏字段按优先级重排并支持宽度自适应
   - 欢迎页增加快速入口与指令引导
   - 小终端布局自适应，优先保留可操作区域

---

## 二、已交付文件清单

### 修改文件

- `src/tui/input.rs`
  - 空输入显示上下文提示
  - 在 `pending_options` 存在时显示候选项预览（最多 3 项 + 剩余数量）
- `src/tui/layout.rs`
  - 高度不足场景下自适应隐藏次要区域（status spacer、model hint），减少拥挤
- `src/tui/statusline.rs`
  - `context_limit` 使用真实上限
  - 状态行字段优先级重排（compact/narrow 分支）
- `src/tui/welcome.rs`
  - 新增 quick access 指令入口（`/help /agents /model @path !command`）

### 新增文档

- `docs/competitor_parity_matrix_2026-05-15.md`
- `docs/competitor_parity_final_draft_2026-05-15.md`

### 变更代码行数（聚焦本次关键文件）

```text
`git diff --stat -- src/tui/input.rs src/tui/layout.rs src/tui/statusline.rs src/tui/welcome.rs docs/competitor_parity_matrix_2026-05-15.md`

after:
```

```text
src/tui/input.rs      |  55 +++++++++++++-
src/tui/layout.rs     |  70 ++++++++++++++---
src/tui/statusline.rs | 182 ++++++++++++++++++++++++++++++---------------
src/tui/welcome.rs    | 202 ++++++++++++++++++++++++++++++++++++++++----------
4 files changed, 396 insertions(+), 113 deletions(-)
```

---

## 三、最终对齐定稿（主模块 + 子模块）

（见附录文档内容）

# Octocode vs 6 CLI 竞品对齐：最终定稿（迭代版）

更新时间：2026-05-15
目标：在“可见性、可达性、可控性、可恢复性”四大体验目标上，把 Octocode 与 6 个竞品做按模块收敛，且输出可执行的落地清单。

## 一、基准与边界（先说清楚）

- 竞品清单：Codex CLI（开源/官方） / Gemini CLI（开源） / Claude Code（闭源） / Kimi CLI（闭源） / Droid CLI（闭源） / Qwen CLI（开源）
- Octocode 现状优先：Rust TUI、Subagent/Swarm、Mission/Events、policy/approvals、MCP stdio、输入历史与会话持久化基础能力。
- 核对原则：
  - 主模块对齐到“用户立刻感知到的交互能力”
  - 子模块对齐到“CLI 命令可达 + TUI 首屏可操作 + 状态可解释”
  - 未对齐不报“已完成”，只做“P0/P1/P2”排序

## 二、模块对齐总表（主模块）

| 主模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen CLI | Octocode 当前 | 目标收敛 |
|---|---|---|---|---|---|---|---|---|
| 入口与会话主干 | 命令齐全+resume | exec/chat/retrieve+resume | continue/retry/continue + 命令面 | chat/resume 命令 | exec + 会话 id | qwen -p + chat/reply | run/agent/mission/resume/chat | 统一入口语义到 `run/resume` 与 TUI 一致，并补命令可见列表 |
| TUI/输入层 | bottom composer+status+footer 清晰 | 状态提示+头显命令 | 多入口命令面 | 最小交互 | 简约执行流 | 首屏指引较清晰 | 输入/状态/欢迎页三态存在 | 主动保持输入可见、优先显示模型/模式/权限/上下文 |
| 命令发现与帮助 | `/help` + 内置命令索引 | `/` 命令面强 | `/help` + command docs | commands 列表 | 固定命令风格 | `/commands` | `/help` 与 `features/agents/mcp` 有散落入口 | 落地统一命令提示/预置模板；欢迎页+输入框都要可达 |
| 权限与审批 | 分模式+审批状态机 | policy hook + trust | permission mode + hooks | plan/default | 企业级 prompt | 安全模式与沙箱 | approval/Policy 框架已在 | 增强权限状态可读性（模式+来源+风险） |
| Agents/任务编排 | agent profiles + multi-agent | agents/skills | agents + context | agent 概念逐步 | 有流程后台化 | agents/skills 框架 | subagent/mission/事件已到位 | “可见”切换和状态追踪为 P0，配置字段齐全为 P1 |
| 工具 & MCP & 扩展 | MCP/stdin 多种后端 | MCP HTTP/SSE + extension | tools + hooks 丰富 | 命令 hook 与工具 | run tool | tools + command packs | MCP stdio 主干、extensions未闭环 | 先完成 MCP 命令面与工具来源可解释 |
| 历史与回放 | 历史/会话/续接 | chat history/replay | tasks/review/context history | session resume | session id | chat logs | mission events + session/transcript 起步 | unify chat/missions 历史导航与导出 |
| 可观测与输出 | stream-json/events | stream-json | 结果流 + hooks | 命令 trace | minimal logs | JSON 输出 | 二进制模式基础可用 | 标准化 machine-readable output schema |

## 三、子模块与对比细节（按实现责任人路径）

### 1) TUI 输入与显示
- 对标命题：输入永远是主控面板，不得被空状态/提示吞没。
- 竞品信号：
  - Codex：输入区始终在可预测位置，支持命令提示、历史、执行状态回显。
  - Gemini/Claude：slash commands 与 command palette 是主要发现入口。
  - Kimi/Qwen：命令入口直接但较精简。
- Octocode 当前实现：
  - `src/tui/input.rs`：空输入占位提示增强，新增 `/help /agents /@path /!cmd`；`pending_options` 真实候选预览（前3 + +N）已实现。
  - `src/tui/layout.rs`：小终端时自适应隐藏 `status spacer` 与 `model hint`，避免压缩主内容。
  - `src/tui/welcome.rs`：快速入口区固定出现在首屏，减少“找不到命令面”。
- 仍缺：
  - `/` 菜单分组展示、快速过滤、可预览命令参数与默认别名。
  - Ctrl-R 命令历史检索（跨会话优先）。

### 2) 状态可见性（statusline）
- 对标命题：状态要先说“我现在在哪、我能做什么、风险是什么”。
- Octocode 当前实现：
  - `src/tui/statusline.rs` 改造为核心优先顺序（模型/模式/状态/权限/上下文）+ 宽度折叠。
  - `context_limit` 接入真实上下文上限。
- 仍缺：
  - `status` 文本与 policy 来源（谁允许/禁止了本次操作）未完全链到同一来源。
  - 小宽度下上下文/成本/工具字段应保留“最小可执行语义”，并支持 `~` 版本化摘要。

### 3) 欢迎页与首次可达性
- 对标命题：首次见到的不应该是“背景信息”，而是“第一步输入路径”。
- Octocode 当前实现：
  - `src/tui/welcome.rs` 增加 quick access，指向 `/help /agents /model @path !command`。
  - 多尺寸布局分支下仍保留命令入口预览。
- 仍缺：
  - 无 API key 与有 API key 时的可操作差异仍依赖文案，建议添加 `one-tap action label`（例如 `/features`/`/agent list` 的可见触发条）。

### 4) 命令系统与 CLI 可达性
- 对标命题：CLI 与 TUI 命令必须映射一致。
- 竞品参考：
  - Gemini 与 Claude 均在 TUI 和非交互命令形成同源树。
  - Codex 对 `/status` `/diff` `/resume` 等命令语义清晰可回归。
- Octocode 当前实现：
  - `src/cli/features.rs`、`src/cli/agent.rs`、`src/cli/mission.rs`、`src/cli/resume.rs` 已有模块分层。
  - 命令面到欢迎/输入提示没有完全收敛。
- 仍缺：
  - `octocode mcp add/list/remove/status` 命令族、`octocode config explain`、`octocode settings`、`octocode session list/replay` 仍建议明确化。

### 5) Agents / Mission / Subagent 编排
- 对标命题：可见性优先于隐藏能力。
- Octocode 优势：已有 supervisor/supervisor/bus/queue 体系。
- 仍缺：
  - Agent 生命周期、工具授权、错误原因未在 TUI 充分可视化。
  - `.octocode/agents/*.md` 可在 validate 后自动映射到状态页。

### 6) 工具链与审批
- 对标命题：用户需要知道每次工具调用前后的原因、风险、代价。
- Octocode 优势：policy 与 approval 已具备基础闭环。
- 仍缺：
  - Hook runner 与 pre/post tool 事件还未全链。
  - apply_patch 及危险命令风险门槛应做“可配置白名单 + 超时 + 片段大小”.

### 7) 配置、可恢复性与机器可读输出
- Octocode 现状：
  - config/session/events 已在多处打通。
- 仍缺：
  - machine output schema 一致化（stream-json 字段名固定）
  - session replay 与 mission replay 的统一入口
  - 统一 config precedence 的 `doctor` 风格提示

## 四、最终行动序列（按 Octocode 交付价值排序）

### P0（本轮）
1. `src/tui/input.rs` 已完成：pending options 真实展示。
2. 完成 `statusline` 字段折叠与优先级收口（已在进行中，需回归确认）。
3. 欢迎页第一屏增加可达命令入口（已完成）。
4. 统一 `pending_options` 提示和 `/help` 入口路径测试（建议补交互测试）。

### P1（下一轮）
1. `/` 命令面板（可过滤/可分组/可参数预览），与 `octocode features` 命令对齐。
2. `octocode mcp list/add/remove/status` 与 `tools` 可读状态。
3. `Ctrl-R` 历史搜索（支持会话级 + 命令级）。
4. `@` 文件提示与命令占位补全（路径优先 + 最近文件 + 项目根）。

### P2（后续）
1. `octocode config explain/doctor` 与 policy 来源解释。
2. hooks runner：PreToolUse/PostToolUse/SessionStart/SessionEnd。
3. stream-json 标准 schema（machine-readable）。
4. mission/chat 统一 replay 视图 + 导出。

## 五、结论（可直接执行）

- 本轮核心成功：UI 的“第一视觉可达性”已经从“静态欢迎页”升级为“命令入口可见 + 输入提示可用 + 状态行优先级化”。
- 下轮关键转折：不能停留在界面文本优化，必须把 CLI 命令与 TUI 行为做同源（尤其 `/` 命令面、session/mcp/agents 可视化）。
- 风险控制：所有新增命令发现能力必须先与现有审批/权限、队列、历史记录绑定，避免“看到可点但执行无反馈”。

## 六、产物清单（已变更/新增）
- `[src/tui/input.rs](/Users/klein/octocode/octocode/src/tui/input.rs)`
- `[src/tui/layout.rs](/Users/klein/octocode/octocode/src/tui/layout.rs)`
- `[src/tui/statusline.rs](/Users/klein/octocode/octocode/src/tui/statusline.rs)`
- `[src/tui/welcome.rs](/Users/klein/octocode/octocode/src/tui/welcome.rs)`
- `[docs/competitor_parity_matrix_2026-05-15.md](/Users/klein/octocode/octocode/docs/competitor_parity_matrix_2026-05-15.md)`
- `docs/competitor_parity_final_draft_2026-05-15.md`（本次新增）



---

# Octocode vs 6 CLI 对照：逐模块完整矩阵（更新版）

更新时间：2026-05-15

来源：官方文档 / 公网仓库 / 官方 help（本地快照见仓库历史与子任务产物）。

## 0) 快速结论

1. Octocode 已具备：Rust 核心能力、TUI、会话、策略与批准框架、MCP stdio 基础、Subagent/Swarm、任务与事件持久化的雏形。
2. Octocode 最大短板：`命令发现与可达性` 和 `第一屏可操作入口`。
3. 从 UI 可用性角度，首屏应优先解决：
   - “输入区总是可见 + 可提示上下文动作”
   - “状态行字段按优先级折叠”
   - “欢迎页把命令面板入口显式化”
4. 从功能可用性角度，首阶段优先保留已有架构、补齐：
   - `/` 命令面板与命令发现
   - `Ctrl-R` / 历史回放可搜索
   - `@` 补全的文件路径入口（含路径提示）
   - `octocode features/agents/mcp/help/model` 可发现的命令面。

## 1) 模块对照（主模块 vs 子模块）

### 1.1 CLI 命令与会话主干

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 主任务 |
|---|---|---|---|---|---|---|---|---|
| 主命令入口 | `codex`（exec/start/resume） | `gemini` + `-p`/`--output-format` | `claude` + `-p`/`--continue` | `kimi` + `--plan`/`--print` 等 | `droid` + `exec` + session flags | `qwen` + `qwen -p` | `octocode` + `run/agent/mission/resume/chat` | 保持并发/会话语义一致，补齐标准入口一致性
| 无交互/机器可读 | JSON/stream-json、resume logs | JSON/headless 与 tool events | headless 与 JSON-ish 记录 | print/trace + resume | 非交互 exec 会话 | JSON/脚本化能力 | CLI 已有，但需要统一事件字段 | 标准化 machine output schema
| 会话恢复 | `--resume` | `/chat resume` 与历史 | `--continue`/`--resume` | `/chat resume` | `exec -s`/会话 id | `/chat resume` | `octocode resume` 与 mission 复用 | 对齐恢复体验并明确命令层语义 |

### 1.2 TUI + 输入层

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 输入前缀系统 | `/` slash、`!` shell、文件/注释上下文 | `/` 命令、`!` shell | `/` commands，`@` + 文件路径语义 | `/` + shell 与恢复 | `exec` 风格命令交互 | slash 与 tool 命令 | 输入可用，但命令发现/补全较弱 | 上线命令建议面和 `Ctrl-R` 历史检索 |
| 欢迎页 | 初始化引导简洁 | 入口指引清晰 | 多上下文提示 | minimal / shell 起步 | 简化会话启动 | assistant 首屏指令清晰 | 欢迎页信息重排较新，但命令入口仍弱 | 在首屏放置可执行命令入口卡 |
| 上下文状态行 | 状态+模式+工具提示 | top/bottom status hints | status/思考提示 | compact 提示 | 轻量状态显示 | prompt/命令提示 | 已有状态line，但信息过密 | 折叠策略重排（core优先） |

### 1.3 命令发现与帮助

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 全量命令列表 | `/help` + 内置 docs | `--help` + `/help`/指令引用 | `/help` + 文档索引 | commands list（含扩展） | CLI reference（固定命令） | `/commands` + extensions | `octocode features` 新增过，但未完全打通 TUI | 命令可达性低，需统一 command palette |
| 自定义命令加载 | command pack | custom commands/toml | `/agents` 与 skill/memory 文件 | agent / command hooks | 有限公开 | `/commands` + 自定义 | `.octocode/commands` 还未作为一等入口 | 统一 command 面 + loader |
| 命令上下文开销 | 统一 help 与当前上下文注入 | 命令上下文提示 | 分层 /agent /tasks /model | 模式化命令入口 | simple 执行命令 | 命令+参数约定 | 命令与输入提示未统一 | 命令提示与执行反馈结构化化 |

### 1.4 权限 / 安全 / 审批

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 核心模式 | default/auto/full-auto/plan | plan/shell/approvals | default/plan/auto/bypass | plan/default/auto-edit/yolo | 公开细节有限 | 安全模式与沙箱约束 | octocode 已有 policy+审批 + mode 但命名与展示需统一 | 标准化权限状态机可见性 |
| 工具策略 | allow/deny + sandbox | tool-level + plan profile | permission rules + tools filter | command/沙箱开关 | 企业化 review 路径 | 规则与 sandbox | policy.rs 已覆盖大部分 | UI 展示审批来源与可复用上下文 |
| Hooks | hooks 分层 | hook + project trust | 多 hook 类型 | 关键步骤 hooks | 少量流程钩子 | hooks/事件 | schema 存在，runtime 不完整 | 逐步接入 `PreToolUse/PostToolUse/UserPromptSubmit` |

### 1.5 Agents / Team / 任务编排

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| Agent 定义 | 内建 + 自定义 profile | agents/skills/extensions | agents 文件 + tool/mode | agents 概念逐步公开 | 工作流子任务 | agents/skills 配置 | `.octocode/agents/*.md` + executor | 显式 agent 管控面薄 |
| 并发执行 | 多任务/队列 | 自动化 agent plan | 并行任务 | loop + retry | session/后台流程 | subagent-like flows | Supervisor/Swarm/MessageBus 已有 | UI 需显示状态与切换 |
| 任务队列/复用 | run history | resume/chat-history | tasks list | run session | bg process | session/retry | mission/event 已开始 | 统一任务目录与回放面 |

### 1.6 工具、MCP 与扩展

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| MCP/工具总线 | stdio + http + 配置 | stdio/http/sse + auth | tools + hooks + memory | MCP docs 与集成 | tools/集成 | MCP 命令与 auth | stdio 客户端已在 | 补 HTTP/SSE、工具可视化来源 |
| 技能/扩展 | 技能+插件+主题 | skills/extensions | SKILL.md + tool packs | 命令包/扩展入口 | enterprise workflow | command packs | 实现仍分散 | 统一 extensions 打包模型 |
| 日志与可追溯 | tool call 日志 + tool events | tool events + command events | 审计型记录 | 日志 trace | session log | session logs | Octocode 有 event/mission，UI 关联待增强 | UI 直接显示来源和失败路径 |

### 1.7 配置 / 可观察性 / 可用性

| 模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen Code | Octocode 现状 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| 配置层级 | system/user/project | env + config + 受信项目 | 多层 + 本地覆盖 | project + user + trust | 官方文档可配置项少 | `.qwen` + settings | storage::Config 已有优先级 | 增加 `config explain/doctor` |
| 可见性/状态 | status, approval summary | settings + 权限状态 | status/permission + context | 命令视图 | process status | status 命令 | statusline/info 已有但密度高 | 折叠信息、保留关键字段 |
| 历史与日志 | session export | resume/share | plan/task traces | chat history | run history | chat logs | transcript/session 存在 | 统一命令和截图式回放 |

## 2) Octocode 侧立即执行建议（按优先级）

### P0（本轮先做）
1. 输入可达性：在空输入显示 `/help /agents @path !cmd` 提示（已落地）。
2. 布局自适应：超小终端去掉 model_hint/footer 冗余（已落地）。
3. 欢迎页：新增快速入口（`/help`, `/agents`, `/model`, `@path`, `!command`）（已落地）。
4. 状态行：将 status/permission/context 拉到固定优先级、隐藏不必要字段（部分落地，需联调）。
5. 状态线 `context_limit` 使用真实上下文上限（参数化）。

### P1（下一轮）
1. `/` 命令面板统一到 plan_tracker：支持分页、过滤、分组、命令说明。
2. `octocode features` 与 `octocode agent/mcp` 命令文案映射到欢迎页与 statusline。
3. `@` 与 `!` 的补全入口加入 suggestion panel（当前 suggestion 面已有数据）。
4. `Ctrl-R` 历史搜索与近似匹配。

### P2（后续）
1. `octocode mcp add/list/remove` 命令族与 trust 状态。
2. Hooks runner（PreToolUse/PostToolUse/UserPromptSubmit）。
3. Stream JSON output 标准 schema。
4. Mission/replay 可视化页。

## 3) 开放源代码 vs 闭源对照标签

- 开源可直接复用：Codex CLI（命令架构/patch 流）、Gemini CLI（命令/plan/mcp）、Qwen CLI（命令风格与接口组织）、Kimi CLI（plan/shell/restore 风格）。
- 闭源可借鉴交互策略：Claude Code（权限模型/上下文管理/agent治理思路）。
- Droid CLI 公开文档较少，主要复用 `exec session + 执行流程` 的简洁风格。



---

## 四、实施建议（可直接执行）

### P0（下一步优先级）
1. `/` 命令面板（分组 + 过滤 + 参数说明）
2. `octocode mcp add/list/remove/status` 与 tools 可视化
3. 统一命令入口映射：`/help`、`/agents`、`/model`、`@path`、`!cmd`
4. 将 `@` 与 `!` 补全增强为可选项面板（目前为文本提示）

### P1
1. `octocode config explain/doctor`：输出 config 来源链（system/user/project/local）
2. session/missions replay 统一入口与展示（含导出）
3. Hooks runner（至少 PreToolUse/PostToolUse）
4. stream-json 输出 schema 固化

### P2
1. 执行风险与安全层：危险命令白名单、超时、补丁体积保护
2. 任务状态与 agent 状态页的可视化（并行任务、失败原因、重试路径）

### 风险提示
- 本轮改造主要聚焦“可见性/可达性”；未做的能力（MCP HTTP/SSE、完整 hooks、`octocode config explain`）仍需下一轮实现，避免在 UI 暂未可达时出现“可见按钮但实际不可执行”情况。
