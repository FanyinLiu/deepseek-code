# DS vs 6 CLI 竞品对齐：最终定稿（迭代版）

更新时间：2026-05-15
目标：在“可见性、可达性、可控性、可恢复性”四大体验目标上，把 DS 与 6 个竞品做按模块收敛，且输出可执行的落地清单。

## 一、基准与边界（先说清楚）

- 竞品清单：Codex CLI（开源/官方） / Gemini CLI（开源） / Claude Code（闭源） / Kimi CLI（闭源） / Droid CLI（闭源） / Qwen CLI（开源）
- DS 现状优先：Rust TUI、Subagent/Swarm、Mission/Events、policy/approvals、MCP stdio、输入历史与会话持久化基础能力。
- 核对原则：
  - 主模块对齐到“用户立刻感知到的交互能力”
  - 子模块对齐到“CLI 命令可达 + TUI 首屏可操作 + 状态可解释”
  - 未对齐不报“已完成”，只做“P0/P1/P2”排序

## 二、模块对齐总表（主模块）

| 主模块 | Codex CLI | Gemini CLI | Claude Code | Kimi CLI | Droid CLI | Qwen CLI | DS 当前 | 目标收敛 |
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
- DS 当前实现：
  - `src/tui/input.rs`：空输入占位提示增强，新增 `/help /agents /@path /!cmd`；`pending_options` 真实候选预览（前3 + +N）已实现。
  - `src/tui/layout.rs`：小终端时自适应隐藏 `status spacer` 与 `model hint`，避免压缩主内容。
  - `src/tui/welcome.rs`：快速入口区固定出现在首屏，减少“找不到命令面”。
- 仍缺：
  - `/` 菜单分组展示、快速过滤、可预览命令参数与默认别名。
  - Ctrl-R 命令历史检索（跨会话优先）。

### 2) 状态可见性（statusline）
- 对标命题：状态要先说“我现在在哪、我能做什么、风险是什么”。
- DS 当前实现：
  - `src/tui/statusline.rs` 改造为核心优先顺序（模型/模式/状态/权限/上下文）+ 宽度折叠。
  - `context_limit` 接入真实上下文上限。
- 仍缺：
  - `status` 文本与 policy 来源（谁允许/禁止了本次操作）未完全链到同一来源。
  - 小宽度下上下文/成本/工具字段应保留“最小可执行语义”，并支持 `~` 版本化摘要。

### 3) 欢迎页与首次可达性
- 对标命题：首次见到的不应该是“背景信息”，而是“第一步输入路径”。
- DS 当前实现：
  - `src/tui/welcome.rs` 增加 quick access，指向 `/help /agents /model @path !command`。
  - 多尺寸布局分支下仍保留命令入口预览。
- 仍缺：
  - 无 API key 与有 API key 时的可操作差异仍依赖文案，建议添加 `one-tap action label`（例如 `/features`/`/agent list` 的可见触发条）。

### 4) 命令系统与 CLI 可达性
- 对标命题：CLI 与 TUI 命令必须映射一致。
- 竞品参考：
  - Gemini 与 Claude 均在 TUI 和非交互命令形成同源树。
  - Codex 对 `/status` `/diff` `/resume` 等命令语义清晰可回归。
- DS 当前实现：
  - `src/cli/features.rs`、`src/cli/agent.rs`、`src/cli/mission.rs`、`src/cli/resume.rs` 已有模块分层。
  - 命令面到欢迎/输入提示没有完全收敛。
- 仍缺：
  - `ds mcp add/list/remove/status` 命令族、`ds config explain`、`ds settings`、`ds session list/replay` 仍建议明确化。

### 5) Agents / Mission / Subagent 编排
- 对标命题：可见性优先于隐藏能力。
- DS 优势：已有 supervisor/supervisor/bus/queue 体系。
- 仍缺：
  - Agent 生命周期、工具授权、错误原因未在 TUI 充分可视化。
  - `.deepseek-code/agents/*.md` 可在 validate 后自动映射到状态页。

### 6) 工具链与审批
- 对标命题：用户需要知道每次工具调用前后的原因、风险、代价。
- DS 优势：policy 与 approval 已具备基础闭环。
- 仍缺：
  - Hook runner 与 pre/post tool 事件还未全链。
  - apply_patch 及危险命令风险门槛应做“可配置白名单 + 超时 + 片段大小”.

### 7) 配置、可恢复性与机器可读输出
- DS 现状：
  - config/session/events 已在多处打通。
- 仍缺：
  - machine output schema 一致化（stream-json 字段名固定）
  - session replay 与 mission replay 的统一入口
  - 统一 config precedence 的 `doctor` 风格提示

## 四、最终行动序列（按 DS 交付价值排序）

### P0（本轮）
1. `src/tui/input.rs` 已完成：pending options 真实展示。
2. 完成 `statusline` 字段折叠与优先级收口（已在进行中，需回归确认）。
3. 欢迎页第一屏增加可达命令入口（已完成）。
4. 统一 `pending_options` 提示和 `/help` 入口路径测试（建议补交互测试）。

### P1（下一轮）
1. `/` 命令面板（可过滤/可分组/可参数预览），与 `ds features` 命令对齐。
2. `ds mcp list/add/remove/status` 与 `tools` 可读状态。
3. `Ctrl-R` 历史搜索（支持会话级 + 命令级）。
4. `@` 文件提示与命令占位补全（路径优先 + 最近文件 + 项目根）。

### P2（后续）
1. `ds config explain/doctor` 与 policy 来源解释。
2. hooks runner：PreToolUse/PostToolUse/SessionStart/SessionEnd。
3. stream-json 标准 schema（machine-readable）。
4. mission/chat 统一 replay 视图 + 导出。

## 五、结论（可直接执行）

- 本轮核心成功：UI 的“第一视觉可达性”已经从“静态欢迎页”升级为“命令入口可见 + 输入提示可用 + 状态行优先级化”。
- 下轮关键转折：不能停留在界面文本优化，必须把 CLI 命令与 TUI 行为做同源（尤其 `/` 命令面、session/mcp/agents 可视化）。
- 风险控制：所有新增命令发现能力必须先与现有审批/权限、队列、历史记录绑定，避免“看到可点但执行无反馈”。

## 六、产物清单（已变更/新增）
- `[src/tui/input.rs](/Users/klein/ds/deepseek-code/src/tui/input.rs)`
- `[src/tui/layout.rs](/Users/klein/ds/deepseek-code/src/tui/layout.rs)`
- `[src/tui/statusline.rs](/Users/klein/ds/deepseek-code/src/tui/statusline.rs)`
- `[src/tui/welcome.rs](/Users/klein/ds/deepseek-code/src/tui/welcome.rs)`
- `[docs/competitor_parity_matrix_2026-05-15.md](/Users/klein/ds/deepseek-code/docs/competitor_parity_matrix_2026-05-15.md)`
- `docs/competitor_parity_final_draft_2026-05-15.md`（本次新增）
