# Anthropic Claude Code 深度调研报告（对标 deepseek-code）

> 调研时间：2026-05-13
> 调研对象：Anthropic Claude Code CLI（最新发布 v2.1.128–v2.1.136 一线，docs 当前版本对应 v2.1.139+ 的 agent view 等特性）
> 信源置信度图例：🟢 官方文档直接确认 / 🟡 GitHub/issues/blog 间接 / 🟠 截图视频可见 / 🔴 推断
> 对应 deepseek-code 现状档案：`docs/audit/00-current-state.md`（21 项痛点）

---

## 0. 调研概览 & 信源

**主要信源（已抓取并核实）**：
- 🟢 官方文档站根：`https://docs.claude.com/en/docs/claude-code/overview`（SPA，备用 markdown 出口在 `https://code.claude.com/docs/en/<page>.md`）
- 🟢 完整文档索引：`https://code.claude.com/docs/llms.txt`（137 页 md，本次抓取 60+ 关键页全文）
- 🟢 每周发布纪要 `whats-new/2026-w13.md` ~ `2026-w19.md`（覆盖最近 7 周特性，涵盖 v2.1.83 → v2.1.136 的所有变化）
- 🟢 完整 changelog `https://code.claude.com/docs/en/changelog.md`（325 KB）
- 🟢 Agent SDK 文档（hooks、subagents、permissions、mcp、skills 等 17 页）
- 🟡 npm 包 `@anthropic-ai/claude-code`：仅暴露 `cli.js`（混淆 JS）+ 平台原生二进制（v2.1.113 之后默认通过 optional dependency 拉取 `@anthropic-ai/claude-code-darwin-arm64` 等，npm install 不再依赖 Node）。
- 🟠 截图/视频：官方文档内嵌的 mp4 演示（agent-view、ultrareview、auto-mode、push-notifications、routines、fullscreen 等），统一视觉特征在下面各节描述。
- 🔴 部分内部协议（Task tool 的 system prompt 模板、minified cli.js 的具体调度逻辑）只能推断。

**33 维度信源置信度统计（粗略，详见每节）**：🟢 24 个 · 🟡 5 个 · 🟠 3 个 · 🔴 1 个。

---

## 1. 架构与语言栈

### 1.1 现状描述
Claude Code 是用 **TypeScript/JavaScript 写的 CLI**，分发为两种形态：(a) npm 包 `@anthropic-ai/claude-code` 提供 `claude` 可执行入口（v2.1.113 起 npm 包内置的 `claude` 不再调用 Node，而是通过 optional dependency 拉取平台原生二进制 `@anthropic-ai/claude-code-darwin-arm64 / -linux-x64 / -win32-x64` 等），(b) 独立 `curl https://claude.ai/install.sh | bash` 安装的原生二进制（macOS/Linux/WSL），(c) Homebrew cask 两个 channel：`claude-code`（稳定，约落后一周）和 `claude-code@latest`（最新即时跟进），(d) Windows 通过 PowerShell `irm https://claude.ai/install.ps1 | iex` 或 cmd 脚本，(e) JetBrains 插件，(f) VS Code 扩展（自带 CLI 二进制）。

TUI 渲染层：从 cli.js 中可见 React 风格的 JSX 注释残片 + ANSI escape 序列 + alternate screen buffer 切换逻辑，**几乎可以确认底层用了 Ink（React for terminals）**。v2.1.89 起新加 "fullscreen rendering" 模式（虚拟化的 alt-screen 渲染器，flicker-free + 鼠标支持 + 长会话内存稳定），v2.1.105 起 `/tui` 命令可以在 `default` / `fullscreen` 间无缝切换且保留 conversation，证明渲染层是两套独立路径（classic line-based scrollback vs. virtualized alt-screen）。原生二进制是 Bun 或自定义打包结果。

### 1.2 关键引用
- 🟢 [Setup guide](https://code.claude.com/docs/en/setup.md)
- 🟢 [v2.1.113 native binaries 公告](https://code.claude.com/docs/en/whats-new/2026-w16.md)：「The `claude` CLI now spawns a native per-platform binary instead of bundled JavaScript」
- 🟢 [Fullscreen guide](https://code.claude.com/docs/en/fullscreen.md)
- 🔴 Ink/React 推断：来自混淆代码中残留的 JSX-style 标记 + 文档对 `useEffect`-like 文案的描述。

### 1.3 亮点
- **两条渲染路径并存**（classic + fullscreen），用户按需切换，不强制断裂。
- **平台原生二进制 + npm 兼容**，启动速度对比纯 JS 显著提升，分发不变。
- **Homebrew 双 channel** 给企业一个稳定锚点。

### 1.4 缺点
- 核心闭源 + 混淆，社区无法 fork 修小 bug。
- 体积大（~50 MB 二进制 + 数十 MB Node-style 资源）。
- TUI 不是真原生（Ink/React 的运行时开销）—— 与 Rust 写的 Codex / deepseek-code 拉不开延迟。

### 1.5 对 deepseek-code 的启示
deepseek-code 选 Rust + ratatui 0.30 + crossterm 0.29 已经在**渲染性能上天然胜出**，这是不该放弃的底气。但 Claude Code 的"两条渲染路径"模式值得学：deepseek-code 现状档案 §2.2 指出 "没有真正的分屏/sidebar"，应该考虑在 ratatui 之上做 "classic（line scroll）vs. workbench（alt-screen + 固定输入框）" 的双模式切换。

---

## 2. 二进制 / 子命令面

### 2.1 现状描述
单一 `claude` 二进制 + 大量子命令。完整 CLI 命令列表（出自 [`cli-reference.md`](https://code.claude.com/docs/en/cli-reference.md)）：

```
claude                                启动交互 session
claude "query"                        交互 session 带初始 prompt
claude -p "query"                     headless 模式（print）
claude -c                             continue 最近 session
claude -r "<session>" "query"         resume by ID/name
claude update                         更新到最新
claude install [version]              安装/重装（stable/latest/版本号）
claude auth login [--email] [--sso] [--console]
claude auth logout
claude auth status [--text]
claude agents                         打开 agent view（背景 session 监控面板）
claude attach <id>                    附加到背景 session
claude auto-mode defaults             打印 auto mode 分类器规则（JSON）
claude logs <id>                      打印背景 session 最近输出
claude mcp                            配置 MCP servers（子命令一大堆，见 §21）
claude plugin / plugins               管理插件
claude project purge [path]           清除项目所有本地状态（transcripts/tasks/history/...）
claude remote-control                 启动 Remote Control 服务（被 claude.ai 控制）
claude respawn <id>                   重启停止的背景 session（--all）
claude rm <id>                        移除背景 session
claude setup-token                    生成 CI 用的长效 OAuth token
claude stop <id> / claude kill        停止背景 session
claude ultrareview [target]           非交互运行 ultrareview
```

CLI flags 极多（cli-reference 单页 55 KB），关键的：`--add-dir`、`--agent`、`--agents '<JSON>'`（动态注入 subagent 定义）、`--allow-dangerously-skip-permissions`、`--allowedTools`、`--append-system-prompt[-file]`、`--bare`、`--betas`、`--bg`（背景 dispatch）、`--channels`、`--chrome`、`--continue/-c`、`--dangerously-load-development-channels`、`--dangerously-skip-permissions`、`--debug`、`--debug-file`、`--disable-slash-commands`、`--disallowedTools`、`--effort`、`--enable-auto-mode`（已废，2.1.111 之后）、`--exclude-dynamic-system-prompt-sections`、`--fallback-model`、`--fork-session`、`--from-pr`、`--ide`、`--init`、`--init-only`、`--include-hook-events`、`--include-partial-messages`、`--input-format`、`--json-schema`、`--maintenance`、`--max-budget-usd`、`--max-turns`、`--mcp-config`、`--model`、`--name/-n`、`--no-chrome`、`--no-session-persistence`、`--output-format`、`--permission-mode`、`--permission-prompt-tool`、`--plugin-dir`、`--plugin-url`、`--print/-p`、`--remote`、`--remote-control/--rc`、`--replay-user-messages`、`--resume/-r`、`--session-id`、`--setting-sources`、`--settings`、`--strict-mcp-config`、`--system-prompt[-file]`、`--teleport`、`--teammate-mode`、`--tmux`、`--tools`、`--verbose`、`--worktree/-w`。

子命令的拼写纠错：「If you mistype a subcommand, Claude Code suggests the closest match and exits without starting a session. For example, `claude udpate` prints `Did you mean claude update?`.」

### 2.2 关键引用
- 🟢 [CLI reference](https://code.claude.com/docs/en/cli-reference.md)（全表）
- 🟢 [Agent view docs](https://code.claude.com/docs/en/agent-view.md)（`claude agents` / attach / logs / stop / rm / respawn 全套）

### 2.3 亮点
- **CLI 表面成熟到「类 git」**：子命令 + 别名（`claude kill`/`stop`、`claude plugin`/`plugins`）+ 拼写纠错 + 上下文相关帮助。
- **shell-friendly**：`auth status` 退出码 0/1 可被脚本检测；`auto-mode defaults` 输出 JSON 可被 grep。
- **`--bare` 模式**专为 SDK/CI 用，跳过自动发现的 hooks/skills/plugins/MCP/auto-memory/CLAUDE.md —— 这是性能 + 行为可预期的工程精品设计。

### 2.4 缺点
- 子命令分类不够好（agent view 相关的 `agents/attach/logs/stop/rm/respawn` 五个动词分散在顶层）。
- `--dangerously-load-development-channels` / `--allow-dangerously-skip-permissions` 这类「危险但有用」的 flag 命名长到讨厌。
- 部分 flag 没列入 `--help`（doc 明确："`claude --help` does not list every flag"）—— 探索性不强。

### 2.5 对 deepseek-code 的启示
deepseek-code 现状档案 §1.3 列了 17 个子命令 + 48 个 slash 命令，**广度上已不输 Claude Code**。但 Claude Code 的几个细节值得抄：(a) `auth login/logout/status` 三件套独立成顶层子命令（deepseek-code 目前只有 `login`）；(b) 命令拼写纠错（"Did you mean X?"）；(c) `--bare` 模式 —— 直接对应 deepseek-code 现状档案 §4 的痛点「Settings 占位」和「scripting 友好度未知」；(d) `project purge` 这种全量清理命令值得加（对应 §3.10 的 sessions / events.jsonl 持久化清理）。

---

## 3. 欢迎屏 / 启动横幅

### 3.1 现状描述
启动 `claude` 显示一个简洁的欢迎屏，**没有大幅 ASCII wordmark**（与 Codex/Gemini 不同）。从最近 changelog 描述和官方截图可见，欢迎屏只显示一行 "Welcome to Claude Code"（或类似的极简标题）+ 当前模型/项目目录/快捷键提示行 + 一条灰色 placeholder prompt 提示（取自项目 git 历史中你最近 touch 过的文件，"prompt suggestions" 功能）。

**Tips of the day** 是 inline 模式：每次启动显示一条小提示。`/powerup` 命令（v2.1.90 引入）显式打开交互式课程演示，弥补"feature discovery 难"问题。**brand color** 在文档站和插件市场是 Anthropic 标志性的 **暖橙 (#D97757 一类)**，CLI 内的 accent 跟随用户主题。

`claude doctor` 命令打开诊断面板（带状态图标 / 按 `f` 让 Claude 修），相当于 deepseek-code 的 `doctor` 子命令的高度产品化版本。

### 3.2 关键引用
- 🟢 [Quickstart](https://code.claude.com/docs/en/quickstart.md)
- 🟢 [Interactive mode > Prompt suggestions](https://code.claude.com/docs/en/interactive-mode.md)：「When you first open a session, a grayed-out example command appears in the prompt input to help you get started. Claude Code picks this from your project's git history」
- 🟢 [Powerup command](https://code.claude.com/docs/en/whats-new/2026-w14.md)

### 3.3 亮点
- **不渲染大 wordmark**，留出垂直空间给真正的对话区。
- **prompt suggestions 智能化**：从 git 历史里挑文件名/最近 touch 的路径，作为灰色占位 prompt —— 按 Tab/→ 接受。
- **`/powerup` 教程**：内嵌动画 demo（不是外链文档）。

### 3.4 缺点
- Tips of the day 没有"再来一条"/"隐藏"按钮（要靠 `/help` 间接探索）。
- 不显示「当前 git 仓库 / 是否 dirty / branch 名」—— 这是 Codex 显式做的安全信号。

### 3.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.3 的痛点 1-3（ASCII wordmark 不像 "DeepSeek"、starter 硬编码、首屏不显示 git 状态）正好被 Claude Code 这一节命中：**完全没有 wordmark 也是合法选项**，把腾出来的垂直空间还给对话区。**最关键**：把 deepseek-code 的 3 个硬编码 starter prompt 替换成 Claude Code 风格的 "从 git history 挑最近文件" 算法 —— 这是 `welcome.rs` 应该做的智能化升级方向。

---

## 4. 主题系统

### 4.1 现状描述
**v2.1.118（Week 17）大幅升级**：`/theme` 命令打开 picker，支持 **`auto`**（匹配终端明暗）/ **`light`** / **`dark`** / **`light-daltonized`** / **`dark-daltonized`**（色盲友好变体）/ **`light-low-contrast`** / **`dark-low-contrast`** 等多个预设。**自定义主题**通过 `~/.claude/themes/<name>.json` 文件定义，每个主题选一个 base preset，然后只 override 你关心的 token。**Plugins 可以发布主题**（在 plugin 的 `themes/` 目录）。

`/theme` picker 还有一个 `Ctrl+T` 切换：开关代码块的 syntax highlighting（用户可以彻底关闭高亮，比如想要复制 + 粘贴时干净）。

### 4.2 关键引用
- 🟢 [Custom themes (Week 17)](https://code.claude.com/docs/en/whats-new/2026-w17.md)
- 🟢 [Auto (match terminal)](https://code.claude.com/docs/en/whats-new/2026-w16.md)
- 🟢 [Settings > theme](https://code.claude.com/docs/en/settings.md)

### 4.3 亮点
- **`auto` 模式跟随终端明暗**（POSIX 上 `COLORFGBG` + macOS `defaults read AppleInterfaceStyle`），同时尊重 dark-daltonized 等 a11y 变体。
- **JSON token override**：用户只需写差异，不要重新定义整个色板。
- **plugin 可贡献主题** —— 生态化。
- `/theme` 在 picker 内带 `Ctrl+T` 关闭代码块高亮 —— 复制友好。

### 4.4 缺点
- 主题 token 文档不全（只能从 plugins 示例反推）。
- 没有 "import / export 单个主题为可分享 gist" 的内置流程（要手动复制 JSON）。

### 4.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.1 已经有三套 palette（Light/Dark/HighContrast）+ 自动检测，**完成度甚至和 Claude Code 接近**。但缺两件事：(a) **不支持用户自定义主题文件**（建议加 `~/.deepseek-code/themes/*.toml`，每个主题指定 base preset + override），(b) **没有 dark-daltonized 等 a11y 变体**（直接抄 Claude Code 的命名空间和色板）。**痛点 5、6**（statusline / diff_viewer / file_tree 不走 palette）是真正的硬伤，必须先把所有组件统一到 palette 才能谈自定义主题。

---

## 5. 整体布局

### 5.1 现状描述
两套布局并存：

**Classic 模式（默认到 v2.1.89 前）**：传统的 line-based 滚动 —— 对话内容追加到终端 scrollback，输入框跟随光标向下移动，scrollback 直接受终端控制（`Cmd+F` 搜索可用）。

**Fullscreen 模式（v2.1.89 引入，v2.1.105 起通过 `/tui fullscreen` 切换）**：alt-screen buffer + 虚拟化渲染。布局变成：
- 顶部：固定区域（统计 / mode 指示）—— 灰色，最低密度
- 中部：转录区（可虚拟滚动，长会话不爆内存）
- 底部：**固定的输入框 + statusline**（不跟随光标移动）
- 不画 sidebar / panel —— 所有"弹窗"都是临时遮挡转录区的 dialog（permission prompt / `/diff` viewer / `/agents` picker / `/skills` picker / transcript viewer 等）

`/focus` 模式（v2.1.96+）：转录区**只显示**你的最后一个 prompt + 一行 tool-call 摘要（带 diffstats）+ Claude 的最终回复。其余全折叠 —— 这是一个"读着轻松"的简化视图，setting 可持久化。

`/diff` 打开"interactive diff viewer"，可左右切换 git diff / 单个 Claude turn 的 diff，上下浏览文件。**不是分屏**——它是 takeover 视图。

**没有真正的 sidebar / file explorer**。这是关键观察：Claude Code 的 IDE 集成（VS Code 插件）补上了这块（VS Code 的 file explorer 就是 Claude Code 的 sidebar），CLI 自己不做。

### 5.2 关键引用
- 🟢 [Fullscreen rendering](https://code.claude.com/docs/en/fullscreen.md)
- 🟢 [/focus command (Week 16)](https://code.claude.com/docs/en/whats-new/2026-w16.md)
- 🟢 [/tui command](https://code.claude.com/docs/en/commands.md)

### 5.3 亮点
- **两条路径共存** —— 用户从老 terminal 迁过来体验不断裂。
- **alt-screen 模式下输入框固定底部** —— 视觉上是"应用"而非"流式 log"，但同时保留可切回 classic 的逃生门。
- **虚拟滚动长会话内存恒定** —— Codex 在这上面有 PR 抱怨。
- **`/focus` 视图** —— 一键看清主线，是噪音过滤器。

### 5.4 缺点
- Alt-screen 模式下 `Cmd+F` 不能用（必须先 `Ctrl+O` 进 transcript viewer，再 `/` 搜索）—— 路径变长。
- 鼠标 capture 与终端 native selection 互斥，over SSH / tmux 体验有摩擦。
- 没有真正的 sidebar / file tree —— 跨文件多任务时需要切换上下文。

### 5.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.2 的痛点「没有 sidebar / 输入区上限 5 行偏紧 / 没有 modal 层」可以靠"双布局"思路解决：
- **保留 classic 模式**（当前的 6 行栈）—— 老用户体验稳定。
- **新增 workbench 模式**（alt-screen + 固定底部输入）—— 给重度用户长会话稳定渲染。
- 输入区上限从 5 行放到 30 行（甚至无限滚动）—— Claude Code 的 paste 也是任意行。
- `/focus` 视图直接抄 —— 中文用户的「精简模式」需求很强。

---

## 6. 对话区渲染

### 6.1 现状描述
**完整 markdown 支持**：headings、bold、italic、strikethrough、inline code、code blocks（带 syntect 风格 syntax highlight，按 theme 跟随）、blockquotes、lists（unordered / ordered / nested）、tables、links（OSC 8 hyperlink in supporting terminals）、horizontal rules、HTML 块级注释（剥离）。CodeBlock 显示 language 标签（左上角小 chip）+ 行号（如果开启）+ 复制按钮（fullscreen 模式可点击）。

**Tool use 卡片** —— 这是视觉最有特征的部分。Claude Code 的 tool call 显示模式：
- 标题行：`● Read(path/to/file)` 或 `● Bash(command)` —— `●` 是 accent 色的实心圆点，工具名首字母大写，参数用括号包起来。
- 子行：缩进 + `⎿` 字符 + 一句话进度（如 `⎿ Read 42 lines (ctrl+o to expand)`）—— 默认折叠。
- 「Reading 1 file...」「Searching..."「Running 1 shell command...」**这类进行时（"-ing"）标题是 Claude Code 标志性的设计**。`/focus` 模式下进一步压缩到 "Edit src/foo.rs (+5 -2)" 单行 + diffstats。

**reasoning / extended thinking 折叠卡片**：默认折叠（v2.1.86 起 "Thinking summaries off by default"），点击或 `Ctrl+O` 展开。展开后用左边一条灰色 `│` 边线 + dim 文字。Opus 4.6/4.7 的 `xhigh` / `max` effort 下 thinking 内容可能数千行，必须折叠。

**MCP tool 调用** 默认折叠为 "Called slack 3 times" 这种一行 —— `Ctrl+O` 进 transcript 模式后才完整展开。

### 6.2 关键引用
- 🟢 [Interactive mode > Transcript viewer](https://code.claude.com/docs/en/interactive-mode.md)：「Ctrl+O ... expands MCP calls, which collapse to a single line like "Called slack 3 times" by default」
- 🟢 [/focus](https://code.claude.com/docs/en/whats-new/2026-w16.md)
- 🟠 视觉来自 [agent-view 截图](https://code.claude.com/docs/en/agent-view.md) 中的 row icon `✻ ∙ ✢ ✽` 系列。

### 6.3 亮点
- **"-ing" 进行时卡片**模式 —— "Reading..." / "Searching..." / "Running..." —— 这是真正抓眼球的细节，把"对话"包装成"在做事"。
- **进行中 vs. 完成**用动画 spinner 和 `●` / `✓` 切换。
- **MCP tool 默认折叠为 "Called X 3 times"** —— 避免 N 个 MCP 工具调用刷屏。
- 代码块默认带 language chip，长代码可一键复制。

### 6.4 缺点
- Spinner 动画依赖终端正常重绘，over SSH / 慢终端会有撕裂感。
- thinking 折叠卡片在 default 模式（非 fullscreen）下不可点击，只能键盘 `Ctrl+O` 切换 —— 鼠标用户摸不着北。

### 6.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.9 已经在 transcript_view.rs 中模仿了 Claude Code 的 `render_claude_tool_lines`（"Reading 1 file..." / "Searching workspace..." 系列），但有几个该补的细节：(a) **MCP tool 折叠成 "Called X N times"** —— 当前没有这个逻辑；(b) **代码块加 language chip**（现状档案 §2.9 明确说"不显示 language 标签"）；(c) **inline subagent + diff + plan 进 transcript 后的渲染顺序**，参考 Claude Code 的折叠策略：默认折叠，`Ctrl+O` 展开 —— 这能解决现状档案痛点 12「markdown parser 太简化」之外的「信息密度过高」问题。

---

## 7. 语法高亮

### 7.1 现状描述
跟随主题。Claude Code 的 syntax highlight 是 native（不依赖 syntect），从混淆代码看大概率用了某种 prismjs / shiki 风格的语法树，但**没有官方文档说明用了什么库**。每个主题都包含 token 颜色（不止 12 个 syntax token：keyword / type / string / number / comment / etc.）。

**v2.1.114 起原生 macOS / Linux 二进制**直接把 `Glob` 和 `Grep` 工具替换成 embedded `bfs` 和 `ugrep` 通过 Bash 调用 —— 这不是 syntax highlight 但说明 Anthropic 在打包原生组件这条路上很激进。

代码块前后空一行；`language` 标签显示在左上角；行号默认隐藏（`/config` 可开）。

### 7.2 关键引用
- 🟢 [whatsnew w17](https://code.claude.com/docs/en/whats-new/2026-w17.md) 提到 native 主题
- 🔴 高亮引擎具体实现未公开

### 7.3 亮点
- 多主题覆盖（dark / light / a11y）。
- 代码块带语言标签。

### 7.4 缺点
- 引擎不透明，用户不能自定义 token 颜色范围（只能整体换主题）。
- 不支持非常见语言（Rust 的 macro / Zig / Nim 等）的高亮深度，主流是 JS/TS/Py/Go/Rust。

### 7.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.15 用 syntect 5，**硬编码 `base16-ocean.dark`**，Light 主题代码块违和。修复方向已经清楚：(a) 把 syntect theme 字符串接到 palette（`palette.code_theme` 字段），Light 主题用 `base16-ocean.light` 或 `Solarized (light)`；(b) 给用户开 `~/.deepseek-code/syntect_themes/` 自定义目录。

---

## 8. Diff 渲染

### 8.1 现状描述
两种渲染场景：

**Inline diff（在 transcript 中显示 Edit 工具的结果）**：折叠为 "Edit src/foo.rs (+5 -2)" 单行，展开后显示统一 diff 格式，背景色按 +/- 染色（绿/红），文件头 `--- a/foo.rs` `+++ b/foo.rs` 用 dim 色，hunk header `@@ -10,5 +10,8 @@` 用 accent 色加粗。**背景色跟随主题**（light 模式下用淡绿/淡红，dark 模式深绿/深红）。

**Interactive diff viewer（`/diff` 命令）**：takeover 视图，显示当前 git diff 和 per-turn diffs。左右箭头切换 git diff / 单个 Claude turn 的 diff，上下浏览文件。可滚动；带文件 list（如果多文件改动）。**不是 side-by-side**，是 unified diff。

**VS Code 扩展中的 diff** 是 side-by-side（IDE native），但 CLI 是 unified。

### 8.2 关键引用
- 🟢 [Commands > /diff](https://code.claude.com/docs/en/commands.md)：「Open an interactive diff viewer showing uncommitted changes and per-turn diffs. Use left/right arrows to switch between the current git diff and individual Claude turns, and up/down to browse files」
- 🟠 [VS Code diff 截图](https://code.claude.com/docs/en/vs-code.md)

### 8.3 亮点
- **per-turn diff** —— 每一回合的 Claude 改动都可独立查看（关键的可审计性）。
- 跟主题。
- VS Code 扩展用 IDE native diff（最好的体验）。

### 8.4 缺点
- CLI 内是 unified-only，没有 side-by-side 模式 —— 长 hunk 可读性差。
- Inline diff 展开/折叠靠 `Ctrl+O`，鼠标点不动（除 fullscreen 模式）。
- 没有 "accept hunk by hunk" —— 是全文件 accept/reject。

### 8.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.14 的 diff_viewer 比 Claude Code CLI 还多一些细节（文件 list 的 ○/✓/✗ 状态、syntect 高亮），核心问题就是**强制 `BG_DEEP` 底色不跟主题**。修复路线：(a) `palette.diff_added_bg` / `palette.diff_removed_bg` 字段加入 palette，light 主题用浅色版本（PaleGreen / Pink），dark 主题保留深色；(b) per-turn diff 已有数据（diffs Vec<FileDiffItem>）—— 加入"按 turn 过滤"的快捷键；(c) 长 hunk 折叠按钮（默认显示前 20 行）。**不需要做 side-by-side**（Claude Code 自己也没做）。

---

## 9. 输入框

### 9.1 现状描述
- **多行**：`Shift+Enter`（部分终端 native，VS Code/Alacritty/Zed 需要跑 `/terminal-setup` 注册）、`\+Enter`（任何终端 fallback）、`Ctrl+J`（readline-native，任何终端）、`Option+Enter`（macOS Meta 配置后）、直接粘贴多行。
- **`!` shell mode**：`!` 在空 prompt 开头时进入 shell 模式，命令直接 bash 执行（不经 Claude），输出加入 conversation context。`Esc`/`Backspace`/`Ctrl+U` on empty 退出。"Pasting text that starts with `!` into an empty prompt enters shell mode automatically」。Tab 补全：从该项目历史 `!` 命令补全。
- **`#` ... 不存在了**（Claude Code **没有 `#` 追加 memory 的语法**！这是与 ChatGPT/Cursor 等不同的设计——memory 通过 `/memory` 命令或对话 "remember that..." 触发 auto-memory）。
- **`/` slash command**：起点。
- **`@` file mention**：典中典。
- **Esc 中断**：单按 Esc 中断当前 turn，保留已完成的工作。**Esc + Esc** 打开 rewind / summarize 菜单（checkpointing！）。
- **Shift+Tab 切 mode**：cycle `default → acceptEdits → plan → (optional: auto / bypassPermissions)`。
- **IDE-mode prompt 指示器**：VS Code / JetBrains 中显示「Connected to VS Code」一类。
- **Voice dictation**：`/voice` 启用，按住 Space 录音（hold mode）或单击 Space 开始 / 再单击发送（tap mode，v2.1.116+）。文档明确 transcription 「不消耗 Claude messages or tokens」。
- **Paste**：图片支持（`Ctrl+V` / `Cmd+V` / `Alt+V` 按平台不同），插入 `[Image #N]` chip，可位置引用。
- **Vim editor mode**：`/config → Editor mode` 切换，支持 normal/insert/visual + 经典 motions（hjkl/w/e/b/0/$/^/gg/G/f{char}/...）+ text objects (`iw/aw/i"/a"/i(`...）。Visual mode (`v`/`V`) v2.1.114 加入。

### 9.2 关键引用
- 🟢 [Interactive mode](https://code.claude.com/docs/en/interactive-mode.md)（所有快捷键 + 多行 + shell mode + voice）
- 🟢 [/memory](https://code.claude.com/docs/en/memory.md) —— `#` 不是输入语法，是 conversation 中"记住 X"被 auto-memory 捕获
- 🟢 [Voice dictation](https://code.claude.com/docs/en/voice-dictation.md)

### 9.3 亮点
- **`Esc + Esc` 一键 rewind** —— 这个细节真正抓人。
- **Shift+Tab 循环 mode** —— mode 切换不离开输入框。
- **Voice dictation**（hold/tap 两模式）+ 编程词汇优化 + 项目名/分支名作为识别 hint —— 远超其他 CLI。
- **Paste 图片插入 `[Image #N]` chip**，可重命名 / 位置引用。
- **Vim mode** 完整（motions + text objects + visual）。
- **`!` shell mode 自动检测粘贴**（粘贴以 `!` 开头自动进入）。

### 9.4 缺点
- 没有 `#` 追加 memory 的快捷语法（要么走 `/memory`，要么靠 auto-memory 捕获 "remember that..."）—— 对刚从 Cursor 迁来的用户不直观。
- Shift+Enter 在某些终端要 `/terminal-setup` 配置 —— 上手摩擦。

### 9.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.5 / 痛点 4-7 几乎是**直接对应 Claude Code 这一节**。修复路线：
1. `input.rs` 触发 `slash_command_panel`（代码就在隔壁，现状档案 §2.10 明确指出 panel 已实现但不联动）—— **P0 第一刀**。
2. `@` mention 补全 UI（现状档案 §3.11 的 `mention_prefix_at_cursor` 已实现，input.rs 没渲染）—— **P0 第二刀**。
3. `Ctrl+R` reverse history search —— 直接抄 Claude Code 的 reverse-search 设计：默认所有项目，`Ctrl+S` 切换范围（all/project/session），`Tab/Esc` 接受继续编辑，`Enter` 接受并执行。
4. `Esc + Esc` 打开 rewind 菜单 —— 这条直接和现状档案痛点 21（rollback 只在内存）联动：先做 UI，再做持久化。
5. `Shift+Tab` 循环 mode —— deepseek-code 已经有 4 个 mode，但切换走的是「永久 mode 改变」，可学 Claude Code 改成「cycle through enabled modes」语义。
6. **Paste 图片 `[Image #N]` chip** —— 现状档案 §1.3 已有 `image_input` skill，但 input.rs 没渲染 chip。

---

## 10. 斜杠命令系统

### 10.1 现状描述
**系统级 slash 命令（出自 [`commands.md`](https://code.claude.com/docs/en/commands.md)）**，按用途分类，**70+ 条**（不是完整列表，部分仅特定 plan 可见）：

| 类别 | 命令 |
|---|---|
| **会话/上下文** | `/clear /reset /new /continue /resume /branch /fork /export /copy /recap /compact /context /focus /tui /color /rename` |
| **诊断** | `/doctor /debug /heapdump /feedback /bug /status /usage /cost /stats /release-notes /privacy-settings` |
| **工具配置** | `/agents /mcp /memory /init /hooks /skills /commands /plugins /reload-plugins /permissions /sandbox /statusline /theme /keybindings /terminal-setup /config /settings /scroll-speed` |
| **模式/模型** | `/model /effort /fast /thinking /plan /mode` |
| **工作流（含 Skill）** | `/batch /simplify /debug /loop /claude-api /review /security-review /ultrareview /ultraplan /diff /goal /fewer-permission-prompts /team-onboarding /powerup /autofix-pr /schedule /background /bg /tasks /bashes /btw /add-dir` |
| **企业/账户** | `/login /logout /upgrade /extra-usage /mobile /ios /android /stickers /passes /chrome /desktop /app /remote-control /rc /teleport /vim（已废）/voice /radio /insights /install-github-app /install-slack-app /web-setup /remote-env /pr-comments（已废）/setup-bedrock /setup-vertex` |
| **检查点** | `/rewind /checkpoint /restore /undo` |
| **其他** | `/help /exit /quit /resume /image` |

**自定义 commands** 统一收敛到 **skills**：`.claude/commands/<name>.md`（旧路径）和 `.claude/skills/<name>/SKILL.md`（新路径，**官方推荐**）都创建 `/<name>` 命令。Plugins 提供的命令使用 `/<plugin-name>:<skill-name>` namespace。

斜杠命令补全：**输入 `/` 触发自动 popover**，显示带 fuzzy filter 的列表（`/skills` v2.1.122 加了 `type-to-filter` 搜索框；`/` menu 全程类似）。

「A command is only recognized at the start of your message」—— 严格行首匹配，避免误触发。

参数：`/clear [name]` 给可选 label；`/copy [N]` 给数字；`/batch <instruction>` 给 prompt。参数 hint 通过 frontmatter `argument-hint: "[issue-number]"` 在补全 popup 中显示。

### 10.2 关键引用
- 🟢 [Commands reference](https://code.claude.com/docs/en/commands.md)（完整 70+ 行表）
- 🟢 [Skills (custom commands merged into skills)](https://code.claude.com/docs/en/skills.md)
- 🟢 [Interactive mode > Commands](https://code.claude.com/docs/en/interactive-mode.md)

### 10.3 亮点
- **数量、分类、覆盖面**远超任何其他 CLI（Codex、Gemini、Aider 加起来都不到 30 个 slash）。
- **commands 合并到 skills** —— 一个 mechanism 同时支持「用户调用」「Claude 自动调用」「Plugin 分发」。
- **`/btw`（side question）**：临时问 Claude 一个问题，看完即焚，不进 conversation history —— **极聪明的去噪设计**。
- **`/copy` 智能化**：识别代码块 → 弹出 picker 让你选哪个 block 复制；按 `w` 写文件而不是剪贴板（SSH 友好）。
- **`/recap`** 自动 session 摘要（如果你离开 3 分钟以上）。
- **`/insights`** 分析你的 Claude Code 使用模式，给 friction 报告。

### 10.4 缺点
- 数量太多导致初次接触者迷茫 —— 靠 `/powerup` 教程缓解。
- 部分命令命名重叠或混乱（`/cost` / `/stats` / `/usage` 是别名；`/clear` / `/reset` / `/new` 也是；`/undo` / `/rewind` 也是）。

### 10.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.10 已有 48 个 slash 命令，**数量上已经追平 Claude Code 三分之二**！但实现深度未必到位。优先级建议：
1. **`/` 触发 popup**（痛点 4）—— 第一刀。
2. **`/btw` side question** —— deepseek-code 还没有；做法是开一个**临时 conversation context** 跑一个回合再回到主线，不持久化。
3. **`/recap` 自动摘要** —— 当 session 闲置 N 分钟后回来时，自动生成「过去 N 步做了什么」一行摘要。
4. **`/copy` 选 code block** —— 简单的功能，立刻提升体验。
5. **`/insights`** —— 长期价值，分析最近 100 个 session 找出 friction（哪些命令被频繁取消、哪些 approval 总被 reject）。
6. **commands → skills 统一化** —— deepseek-code 已经有 `/skills` 命令但不清楚是否合并 commands；建议直接抄 Claude Code 的迁移路径（保留兼容老 .md 文件 + 推 skills 目录）。

---

## 11. @-mentions

### 11.1 现状描述
**`@` 文件提及** + **`@` agent 提及** + **`@` 图片 chip 占位**（间接）。

**文件提及**：输入 `@` 触发 fuzzy matching popup。可以：
- `@file.ts` —— 单个文件
- `@auth` —— fuzzy match 任何文件名含 "auth"（"matches auth.js, AuthService.ts, etc."）
- `@src/components/` —— 末尾斜杠 = 文件夹
- `@app.ts#5-10` —— 行号范围（VS Code 扩展按 `Option+K` / `Alt+K` 自动注入选中文本的范围）
- `@AGENTS.md` —— 在 CLAUDE.md 中通过 `@path/to/import` 导入其他文件（递归最多 5 跳，第一次会弹审批对话框）

**agent 提及**：`@"code-reviewer (agent)"` —— typeahead 选完显示 quoted 形式；plugin agent 显示为 `@<plugin>:<agent>`。@-mention 保证选中的 agent 被使用（不是让 Claude 自己决定）。

**MCP resource**：通过 `@mcp:<server>:<resource-uri>` 注入（推断 + 部分文档暗示，未直接确认 syntax）。

**Image chip**：粘贴图片 → 自动转 `[Image #N]` 占位 → 你可以在 prompt 后面写 "describe the second image" 之类按编号引用。**这不是 `@` 但等价的位置占位语义**。

PDF 文件：大 PDF 可以让 Claude 读特定页（"single page, a range like pages 1-10, or an open-ended range」）—— 通过 prompt 内的 `pages: "3-10"` 参数语义。

### 11.2 关键引用
- 🟢 [VS Code @-mention](https://code.claude.com/docs/en/vs-code.md)：「fuzzy matching」描述
- 🟢 [Subagents @-mention](https://code.claude.com/docs/en/sub-agents.md)
- 🟢 [Memory imports](https://code.claude.com/docs/en/memory.md)：「`@path/to/import` syntax」

### 11.3 亮点
- **fuzzy match** —— `@auth` → AuthService.ts —— 不需要记完整路径。
- **行号范围 `#5-10`** —— 给 Claude 精确上下文。
- **agent / MCP / file 三种实体统一在 `@` 语义下**。
- **CLAUDE.md import 也用 `@`** —— 一致性。

### 11.4 缺点
- MCP resource 的 `@` 语法在文档里不显眼，要靠 `/mcp` 反查每个 server 的 resource list。
- 图片靠 paste，**不能 `@image.png`**（这个不是 @ 语义而是 paste 语义，可能会困惑用户）。

### 11.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.11 的 `mention_prefix_at_cursor` 已经实现 file mention 后端，但 input.rs 不触发 popup。建议升级路线：
1. **`@` 触发 popup**（P0）。
2. **fuzzy match** —— 抄 `nucleo-matcher` Rust crate（VSCode 用的 fzf 算法的 Rust 实现）。
3. **行号范围 `#5-10` 注入** —— deepseek-code 当前没有"读文件特定行"的 tool 调用，需要先在 backend 加 read_file_range tool，再在 mention 解析中拼装参数。
4. **agent / MCP resource 用同一个 `@` 语义** —— 不要分散成 `@agent:name` `@mcp:...` 不同前缀。

---

## 12. 补全

### 12.1 现状描述
五处独立补全：
- **命令补全**：`/` 起 fuzzy filter + 描述行。
- **文件补全**：`@` 起 fuzzy + 行号范围。
- **agent 补全**：`@<name>` 选 agent，typeahead 自动加 `(agent)` 标记。
- **`!` shell 补全**：在 `!` 模式下，**Tab 从该项目之前的 `!` 命令历史中补全**（不是 fish/zsh 风格 PATH 补全，是 history-based）。
- **prompt suggestion**：灰色 placeholder，按 Tab / → 接受。

`Ctrl+R` reverse history search（v2.1.129 起默认全项目，`Ctrl+S` 切 session/project/all）+ `Tab` 接受 + 编辑 + `Enter` 立即执行。

模型补全：`/model` 命令下，部分 gateway 模式（`CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`）会列 `/v1/models` 返回的列表。

### 12.2 关键引用
- 🟢 [Interactive mode > Reverse search with Ctrl+R](https://code.claude.com/docs/en/interactive-mode.md)
- 🟢 [Shell mode > history-based autocomplete](https://code.claude.com/docs/en/interactive-mode.md)

### 12.3 亮点
- **history-based Tab 补全 in shell mode** —— `!` 模式下不抢 fish/zsh 的角色。
- **`Ctrl+R` 三档 scope cycling** —— 一个键管所有项目历史。
- **prompt suggestion 复用 prompt cache** —— 后台请求几乎免费。

### 12.4 缺点
- 没有命令参数 hint 行内显示（agent_hint 只在 popup 中显示，输入到一半时不再提示）。

### 12.5 对 deepseek-code 的启示
deepseek-code 现状档案痛点 4-7 直接对应。优先级：（已在 §9 / §10 / §11 中给出）。补充：**prompt suggestion 的灰色文字**是非常便宜但 effect 大的设计 —— 在 input.rs 中加一个 fetch_recent_files 的 placeholder（从 git log --name-only 取最近 5 个文件）。

---

## 13. 状态栏 / statusline

### 13.1 现状描述
**Claude Code 的 statusline 是用户脚本**（不是内置的）。架构：
- 配置：`settings.json` 的 `statusLine` 字段：`{"type": "command", "command": "~/.claude/statusline.sh", "padding": 2}`
- 工作模型：Claude Code 在每个 assistant turn / `/compact` 完成 / permission mode 改变 / vim mode toggle 时，**把一份完整 JSON session data pipe 到该脚本的 stdin**；脚本把任意文本（含 ANSI 颜色、OSC 8 hyperlink）打到 stdout，Claude Code 显示。
- 更新去抖动 300ms。
- 可选 `refreshInterval`（秒）：用于时间 / 外部数据驱动 —— event-driven 之外的 fallback。
- 可选 `hideVimModeIndicator: true` —— 如果你自己渲染 vim mode。

**JSON schema**（部分关键字段）：
```json
{
  "cwd": "/current/working/directory",
  "session_id": "abc123...",
  "session_name": "my-session",
  "transcript_path": "/path/to/transcript.jsonl",
  "model": {"id": "claude-opus-4-7", "display_name": "Opus"},
  "workspace": {
    "current_dir": "...",
    "project_dir": "...",
    "added_dirs": [],
    "git_worktree": "feature-xyz"
  },
  "version": "2.1.140",
  "output_style": {"name": "default"},
  "cost": {
    "total_cost_usd": 0.01234,
    "total_duration_ms": 45000,
    "total_api_duration_ms": 2300,
    "total_lines_added": 156,
    "total_lines_removed": 23
  },
  "context_window": {
    "total_input_tokens": 15500,
    "total_output_tokens": 1200,
    "context_window_size": 200000,
    "used_percentage": 8,
    "remaining_percentage": 92,
    "current_usage": {
      "input_tokens": 8500,
      "output_tokens": 1200,
      "cache_creation_input_tokens": 5000,
      "cache_read_input_tokens": 2000
    }
  },
  "exceeds_200k_tokens": false,
  "effort": {"level": "high"},
  "thinking": {"enabled": true},
  "rate_limits": {
    "five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
    "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}
  },
  "vim": {"mode": "NORMAL"},
  "agent": {"name": "security-reviewer"},
  "worktree": {"name": "...", "path": "...", "branch": "...", "original_cwd": "...", "original_branch": "..."}
}
```

**`/statusline` 命令**：自然语言指令，让 Claude **自己生成 statusline 脚本**写到 `~/.claude/statusline.sh` 并更新 settings.json。

**默认 statusline**：单行 / 两行，显示 `[Opus] dirname 8% context` 或类似 —— **不彩色 chip 风**，更接近一行紧凑文字 + 偶尔的 git branch + cost。

**ANSI 颜色 + OSC 8 hyperlink** 都支持。

### 13.2 关键引用
- 🟢 [Statusline customization](https://code.claude.com/docs/en/statusline.md)（完整 schema + 7 个示例脚本：上下文 progress bar、git status、cost、clickable links 等）

### 13.3 亮点
- **完全脚本化 / 用户拥有 statusline**：自由度最大，不约束信息密度或视觉风格。
- **每次 turn pipe 完整 JSON** —— 脚本能用 jq 提取任何字段。
- **`refreshInterval`**：解决 idle 时不更新的问题。
- **plugin 可发布 statusline 模板**。
- **OSC 8 hyperlink** —— PR 链接可点击。
- **`/statusline` 自然语言生成** —— 「show model name and context percentage with a progress bar」一键生成脚本。

### 13.4 缺点
- 用户脚本性能可能拖慢 statusline 更新（debounce 300ms 缓解但极端场景仍卡）。
- 默认 statusline 信息密度低 —— 对照 Codex / deepseek-code 的 chip 风格"看一眼啥都知道"差距明显。

### 13.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.7（11 个彩色 chip + ¥ CNY + 1M context 硬编码）这一块的修复路线必须双管齐下：
1. **Chip 接入 palette**（痛点 3、5）—— 移除硬编码 RGB。
2. **加用户自定义 statusline 脚本**：抄 Claude Code 的 settings 字段 `[statusline.command]`，pipe JSON 到 stdin，stdout 显示。**保留 default chip 模式**作为开箱即用，但允许 power user 完全替换。
3. **JSON schema 直接抄 Claude Code**：`cost.total_cost_usd`（不是写死 ¥）、`context_window.context_window_size`（让脚本知道当前模型 ctx 上限）、`effort.level`、`thinking.enabled` —— 这些字段对脚本作者非常友好。
4. **OSC 8 hyperlink** for PR / 文件路径。

---

## 14. Spinner / 流式 / thinking 指示器

### 14.1 现状描述
**Tool call spinner**：tool 调用进行中显示动画（Braille 字符序列）+ "Running 1 shell command..." 标题。完成后切换到 `●`（done）或 `✗`（failed）+ duration。

**Thinking 动画**：extended thinking 启用时，对话区会出现一个折叠的 thinking 卡片，标题如 "✻ Thinking" + 计时器；展开看具体思考内容。Opus 4.7 / xhigh effort 下可能 10s-2min。**v2.1.86 之后 thinking summaries 默认关**（`showThinkingSummaries: true` 恢复）。

**"Brewing for N seconds"** —— 这是 **GitHub Copilot 的 UI 信号**，**不是 Claude Code 的设计**。Claude Code 用 "Thinking..." / "Pondering..." 之类简单文案。

**Agent view 中的 spinner**：背景 session 状态用 `✻`（process alive，replies immediately）/ `∙`（process exited，still attachable）/ `✢`（loop session sleeping）/ 动画 `✽`（working）—— 4 种形状 + 颜色组合（动画/黄/灰/绿/红）表达 6 种状态。

**Streaming**：流式输出按 token 显示，输出区不重排（在 fullscreen 模式下是虚拟化追加）。partial-message events 可以通过 `--include-partial-messages` 暴露给 SDK。

`/recap` 自动生成一行 session summary（用 Haiku 模型，开销极小） —— 你离开 3 分钟后回来时显示。

### 14.2 关键引用
- 🟢 [Agent view > Read session state](https://code.claude.com/docs/en/agent-view.md)
- 🟢 [Interactive mode > Session recap](https://code.claude.com/docs/en/interactive-mode.md)
- 🟢 [Statusline > effort.level](https://code.claude.com/docs/en/statusline.md)
- 🔴 "Brewing for N" **不是** Claude Code 设计（推断：可能 deepseek-code 自己加的；现状档案 §3.11 也猜测来自 Copilot 借鉴）

### 14.3 亮点
- **多形状（`✻ ∙ ✢ ✽`）+ 颜色双维度** —— 用紧凑视觉传递多维状态信息。
- **session recap** —— 离开 / 回来这个场景的产品化最贴心。
- **thinking 默认关** —— 解决了"显示 5000 行思考刷屏"的问题，需要时再展开。

### 14.4 缺点
- "Thinking..." 文案不分级（没有 "still thinking" / "almost done" 这种渐进文案 —— 这点 **deepseek-code 反而做得好**）。
- Spinner 在非 fullscreen 模式有重绘开销。

### 14.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.6 的 status_bar 渐进文案（"thinking → still thinking → almost done"）**比 Claude Code 还细**，保留。但要补的：
1. **多形状 spinner** —— `✻ ∙ ✢ ✽` + 6 态颜色。
2. **session recap**：实现 `cli recap` / `/recap` 命令，自动在 idle 3min 回来时生成（用 DeepSeek Flash 跑摘要，开销小）。
3. **thinking 默认折叠**：deepseek-code 现状档案 §2.9 显示 `show_reasoning` 是个 boolean flag，但默认值在 app.rs 没看到 —— 应该默认 false，留 hotkey 展开。

---

## 15. Plan mode / TodoWrite UI

### 15.1 现状描述
**Plan mode 是 permission mode 之一**（Shift+Tab cycle 到达）。进入后 Claude 只用 read-only tools 探索代码 + 写 plan，**不改文件**。Plan 完成时会弹出 **approval dialog**，提供 5 个选项：
1. Approve and start in auto mode
2. Approve and accept edits
3. Approve and review each edit manually
4. Keep planning with feedback
5. Refine with Ultraplan for browser-based review

按 `Ctrl+G` 可在默认编辑器打开 plan 直接改。

**Plan 不是 todo list**。它是一段 markdown 文本（结构化但不是 schema）。

**Todo list / Task tool**（互相替代关系）：
- **Old**：`TodoWrite` tool（仍是默认在 SDK / `claude -p` 中；交互式 session 已默认用 Task tools）
- **New**：`TaskCreate / TaskGet / TaskList / TaskUpdate / TaskStop / TaskOutput`（deprecated 在 task-output 上） + 配套 cron 三件套 `CronCreate/CronDelete/CronList`
- 状态：`pending / in_progress / completed` 三态
- UI 显示：终端状态区一行 row，**默认显示最多 5 条**任务，按 `Ctrl+T` toggle 显示/隐藏。「show me all tasks」/「clear all tasks」是自然语言操作。
- **持久化**：「Tasks persist across context compactions」+ `CLAUDE_CODE_TASK_LIST_ID=my-project` 可命名共享于 `~/.claude/tasks/`
- 不支持嵌套（一层 flat list）。
- **完成态checkbox**：`◇ pending / ✻ in progress / ✓ completed` 类似（部分推断）。

`/plan` 命令进入 plan mode 后立即给提示（一步走完 cycle）。`Ctrl+T` toggle 任务列表显示。

### 15.2 关键引用
- 🟢 [Plan mode](https://code.claude.com/docs/en/permission-modes.md)
- 🟢 [Task list](https://code.claude.com/docs/en/interactive-mode.md#task-list)
- 🟢 [Tools reference > TaskCreate/TaskList/TaskUpdate](https://code.claude.com/docs/en/tools-reference.md)
- 🟢 [Ultraplan](https://code.claude.com/docs/en/ultraplan.md)

### 15.3 亮点
- **5 选 1 approval dialog** —— 让用户在 plan 完成时统一选定后续 permission 模式（不是先 plan 完再单独切 mode）。
- **`Ctrl+G` 编辑 plan** —— Claude 的输出不是定死的，可以人工 review 再实施。
- **Task tools + Cron tools 一起设计** —— 一个 session 内调度多次执行（`/loop` 就是基于这套）。
- **Task list 持久化**跨 compaction、跨命名共享（`CLAUDE_CODE_TASK_LIST_ID`）。

### 15.4 缺点
- Task list 不支持嵌套 —— 长 plan 只能 flat 表达，逻辑层级丢失。
- TodoWrite vs. Task tool 的过渡期（`CLAUDE_CODE_ENABLE_TASKS=1` 提前切到新版）—— 文档充满 "deprecated in favor of..." 噪音。
- `/plan` 入口和 Shift+Tab 入口的关系略乱（一个是 single-prompt 入口，一个是 session-wide）。

### 15.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.10 的 plan_tracker（plan 2/5、进度条、step kind 自动识别、动态 6 行窗口）**视觉细节比 Claude Code 还细**。但应该补的：
1. **Plan approval 5 选 1 dialog**：抄 Claude Code 的 "Approve & auto / accept edits / manual review / keep planning / refine elsewhere"，对应 §2.10 的 PlanAction DecisionKind。
2. **`Ctrl+G` 编辑 plan**：调用系统 `$EDITOR` 打开 plan 文本。
3. **Task list 持久化跨 compaction**（痛点 21 的延伸）：现状档案 §3.4 提到 BackgroundQueue / TaskStatus，但持久化机制未读完 —— 建议确认是否跨 compact 保留。
4. **`CLAUDE_CODE_TASK_LIST_ID` 等价物**：deepseek-code 的 task list 是 session-scoped 还是 project-scoped？应该支持 `--task-list-id` 来共享于多 session。

---

## 16. Sub-agents / /agents

### 16.1 现状描述
**Sub-agent 定义** = Markdown file + YAML frontmatter，在 `.claude/agents/<name>.md`（项目）或 `~/.claude/agents/<name>.md`（用户）。

**完整 frontmatter schema**（关键字段）：
```yaml
---
name: code-reviewer                          # 必填，lowercase-hyphen, max 64 chars
description: Reviews code for quality        # 必填，告诉 Claude 何时 delegate
tools: Read, Glob, Grep                      # 可选（默认 inherit 所有）
disallowedTools: Write, Edit                 # 可选
model: sonnet | opus | haiku | inherit | <full-id>
permissionMode: default|acceptEdits|auto|dontAsk|bypassPermissions|plan
mcpServers: { ... }                          # 可选（inline MCP server 定义）
hooks: { PreToolUse: [...], PostToolUse: [...] }
maxTurns: 50
skills: [api-conventions, error-handling]    # preload skills 到 context
initialPrompt: "..."                         # 自动 submit 第一回合
memory: user | project | local               # 启用 persistent memory dir
effort: low | medium | high | xhigh | max
background: true                             # 后台运行
isolation: worktree                          # 在独立 worktree 运行
color: red | blue | green | ...              # UI 卡片颜色
---

Markdown 系统 prompt 正文...
```

**优先级**：Managed > `--agents` CLI flag > project (`.claude/agents/`) > user (`~/.claude/agents/`) > plugin。同名取高优先级。

**built-in subagents**：Explore（Haiku, 只读）/ Plan（plan mode 内研究用，只读）/ general-purpose（all tools）/ statusline-setup / claude-code-guide。

**调用方式**：
1. **Auto-delegation**：Claude 根据 description 自己决定派 subagent。
2. **Natural language**：在 prompt 中点名（"Use the test-runner subagent..."）。
3. **`@-mention`**：`@"code-reviewer (agent)"` —— 保证用这个 agent。
4. **session-wide**：`claude --agent <name>` —— 整个 session 把主线程当 subagent 跑（替换 system prompt）。

**Task tool** (`Agent`)：Claude 调用此 tool 时 spawn subagent。Subagent **不能 spawn 其它 subagent**（避免 infinite nesting）。

**Parallel scheduling**：「Run agents in parallel」明确有 4 种并行：subagents（同 session 内）/ agent view（独立背景 sessions）/ agent teams（实验，共享 task list）/ worktrees（git 工作树隔离）。**没有明确 max_parallel 上限**（受 quota 和模型 rate limit 限制）。

**UI**：`/agents` 命令打开 tabbed 界面 —— **Running tab**（live subagents，可 open/stop）+ **Library tab**（所有定义，create / edit / delete）。`/agents` 写完后 immediate take effect，**不需重启**。Background subagent 用 `Ctrl+X Ctrl+K` kill 全部（按两次确认）。

**Agent view（`claude agents`）**：v2.1.139 引入的 monitor 面板，所有 background sessions 一张表格 row，按状态分组（Working / Needs input / Completed / Pinned）。peek (Space) / attach (Enter or →) / dispatch (typing prompt + Enter)。每行配置：动画图标 + 名字 + 当前 activity + 时间戳 + PR status dot（链接到 PR）。

**Subagent memory**：`memory: project|user|local` 字段启用 `agent-memory/<name>/MEMORY.md`（前 200 行 / 25 KB 在每次启动注入 system prompt），子 agent 自己读写。

### 16.2 关键引用
- 🟢 [Subagents](https://code.claude.com/docs/en/sub-agents.md)
- 🟢 [Agent view](https://code.claude.com/docs/en/agent-view.md)
- 🟢 [Agent teams](https://code.claude.com/docs/en/agent-teams.md)
- 🟢 [Agents overview](https://code.claude.com/docs/en/agents.md) (4 种 parallel 对比)

### 16.3 亮点
- **schema 简单但表达力强**：`name + description + body + tools + model` 五要素就能跑。
- **`isolation: worktree`** —— 直接 git worktree 隔离，避免文件冲突。
- **`memory:` 字段** —— subagent 累积跨 session 的领域知识。
- **`skills:` preload** —— 给 subagent 注入 N 个 SKILL.md 内容到 system prompt。
- **`--agents '<JSON>'` CLI flag** —— 临时注入 subagent，不写文件，CI/automation 友好。
- **Agent view 把所有 background sessions 一张表展示** —— 这是「想 dispatch 5 个并行任务」场景的最佳 UI。

### 16.4 缺点
- 不能嵌套 subagent —— 大任务无法继续分解。
- frontmatter 字段很多（~16 个），学习曲线陡。
- Agent view 各 session 独立计费 —— 「Each session uses your subscription quota independently」—— 容易超 quota。

### 16.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.1 列了 5 套并行抽象（subagent / team / swarm / lanes / task_tool），**严重过度工程**。建议收敛路线：
1. **直接采用 Claude Code 的 frontmatter schema** —— deepseek-code 现状档案 §2.12 已经有 SubagentCard 数据结构，把 backend 的 `SubagentConfig` 改成同样的 YAML schema，删除 `team.rs`，把 `swarm.rs` 降级为 internal scheduler。
2. **`/agents` tabbed UI** —— Running tab + Library tab，现状档案 §2.12 的 subagent_cards 直接是 Running tab 的实现。
3. **`isolation: worktree`** —— deepseek-code 没有 worktree 支持，这是 P1 改造大头，对应痛点 18 / 19。
4. **`memory:` 字段** —— deepseek-code 目前没有 subagent persistent memory；建议加 `.deepseek/agent-memory/<name>/MEMORY.md`。
5. **`--agents '<JSON>'`** —— 临时注入 subagent 定义（CI/headless 场景）。
6. **Background agent view** —— deepseek-code 现状档案 §3.4 有 BackgroundQueue / TaskStatus，缺一个 "dispatch + monitor + attach" 的 takeover UI。

---

## 17. Permission 系统

### 17.1 现状描述
**6 个 permission modes**：
| Mode | What runs without asking | Best for |
|---|---|---|
| `default` | Reads only | Getting started, sensitive work |
| `acceptEdits` | Reads + file edits + filesystem bash (mkdir/touch/rm/mv/cp/sed) | Iterating |
| `plan` | Reads only | Exploring before changing |
| `auto` | **Everything**, with background safety classifier | Long tasks |
| `dontAsk` | Only pre-approved tools | Locked-down CI |
| `bypassPermissions` | **Everything** without checks | Containers/VMs only |

**Cycle 顺序**：`default → acceptEdits → plan → (auto / bypassPermissions)` —— 后两个是 optional 加入 cycle 的（auto 需要账户满足条件 + opt-in，bypassPermissions 需要启动时带 `--allow-dangerously-skip-permissions`）。

**Auto mode**（**重要差异化设计**）：
- 一个独立的 **classifier model**（不是你 `/model` 选的那个）评估每个 action。
- **默认 block**：`curl|bash`、外发敏感数据、生产部署/迁移、IAM 授权、shared infra 修改、不可逆删除会话前文件、force push、push to main。
- **默认 allow**：本地文件操作、安装 lock 文件中的依赖、读 `.env` 并发到匹配 API、只读 HTTP、push to 自己 branch。
- **Boundary statements**：你在 conversation 里说 "don't push" —— classifier 把这当 block signal，直到你 lift。
- **Fallback**：3 次连续 block / 20 次总 block → auto mode 暂停，回到 prompt。
- 要求：Max/Team/Enterprise/API plan + 特定模型（Sonnet 4.6/Opus 4.6/4.7）+ 非 Bedrock/Vertex/Foundry。

**Permission rules 评估顺序**：
1. Hooks（PreToolUse hook 可以 deny / allow / pass）
2. Deny rules（settings.json `permissions.deny[]` + `--disallowedTools`）
3. Permission mode（acceptEdits/bypassPermissions 直接通过；其他 fall through）
4. Allow rules（`permissions.allow[]` + `--allowedTools`）
5. `canUseTool` callback / 交互 prompt（dontAsk 模式跳过此步直接 deny）

**Rule syntax**（`permissions.allow / ask / deny`）：
```
"Bash(npm run lint)"               # 精确匹配
"Bash(npm run test *)"             # 通配
"Bash(git diff *)"                 # 命令 + 参数模式
"Read(~/.zshrc)"                   # 路径
"Read(./src/**/*.ts)"              # 路径 glob
"Edit(//path)"                     # 绝对路径（//）
"Edit(/path)"                      # 项目相对路径（单 /）
"WebFetch(https://api.example.com/*)"  # URL
"Agent(Explore)"                   # subagent 名字
"mcp__github__*"                   # MCP tool 名 wildcard
```

**Protected paths**：永不 auto-approve（除 `bypassPermissions`）：
- 目录：`.git`、`.vscode`、`.idea`、`.husky`、`.claude`（除 commands/agents/skills/worktrees 子目录）
- 文件：`.gitconfig`、`.gitmodules`、`.bashrc/.bash_profile/.zshrc/.zprofile/.profile`、`.ripgreprc`、`.mcp.json`、`.claude.json`

**Auto mode 的 dropped rules**：进入 auto mode 时，broad allow rules（`Bash(*)`、`Bash(python*)`、package manager run、`Agent` allow）被丢弃，narrow rules（`Bash(npm test)`）保留。

**`/fewer-permission-prompts`** skill：扫历史 transcript，找出常用的 read-only Bash/MCP 调用，生成 prioritized allowlist 加到 `.claude/settings.json`。

**UI**：交互式 permission prompt 显示工具名 + 参数 + 多选（Allow once / Allow for session / Allow always / Deny / Edit and approve / Don't ask again for this prefix）。`/permissions` 命令打开 picker，可按 scope（user/project/local/managed）查看、添加、删除 rule，**`Recently denied` tab 可按 `r` 重试**（auto mode 配套）。

### 17.2 关键引用
- 🟢 [Permission modes](https://code.claude.com/docs/en/permission-modes.md)
- 🟢 [Permissions](https://code.claude.com/docs/en/permissions.md)
- 🟢 [SDK Permissions](https://code.claude.com/docs/en/agent-sdk/permissions.md)
- 🟢 [Auto mode classifier](https://www.anthropic.com/engineering/claude-code-auto-mode)（blog post 链接）

### 17.3 亮点
- **5 + 1 mode 矩阵**，每档都有清晰的"什么会自动通过"约束。
- **Auto mode classifier** —— 罕见的「不让你做安全决策但替你做安全决策」工程实现，结合 boundary statements 把对话中的 "don't push" 当 block signal。
- **`Agent()` permission rules** —— subagent 可以被禁用（`deny: ["Agent(Explore)"]`），细到 subagent name。
- **`/fewer-permission-prompts`** —— 转录扫描出常用 read-only 调用一键加白名单。
- **Recently denied tab + 重试**：auto mode 被错 deny 时一键 retry。
- **dropped broad rules in auto mode** —— 自动收紧 `Bash(*)` 这种宽规则。

### 17.4 缺点
- mode 矩阵 + rule syntax 学习曲线陡，文档密集。
- auto mode 仅限 Anthropic API（不能 Bedrock/Vertex），企业部署可能受限。
- Protected paths 列表是写死的（虽然管理员可以管 managed settings）—— 想加 `.envrc` 一类自定义难。

### 17.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.5 的 policy/approvals.rs（4 actions × 7 risks + autonomy 三档）+ `defense/perimeter.rs` 静态扫描 + `defense/identity.rs` prompt-injection 防御，**安全模型实际上比 Claude Code 还细**，但缺：
1. **`permissions.allow / ask / deny` 白名单 schema**（痛点 19、20）—— 直接抄 Claude Code 的 rule syntax，特别是 `Bash(npm run *)` 这种命令通配 + `Read(./src/**/*.ts)` 这种路径 glob。
2. **Auto mode classifier**：deepseek-code 已有 ComplexityRouter（差异化设计），可改造为「risk classifier」：用一个轻量模型（DeepSeek Flash？）对每个 action 评估，类似 Claude Code 的两层（broad classifier + boundary statements）。
3. **Protected paths**：当前现状档案 §3.9 PathsConfig 已有 protected list（用户可配），保留但加 default list（`.git`、`.deepseek`、`.gitconfig`、shell rc）。
4. **`Recently denied` 回顾 + retry**：对应现状档案 §2.11 的审批面板，加一个 history view。
5. **`/fewer-permission-prompts` 转录扫描**：deepseek-code 的 events.jsonl 已经持久化，可以写一个 skill 扫历史。
6. **dropped broad rules**：当进入"较宽 mode"时自动收紧之前用户加的过宽 allow rule（防止全局 `run_command(*)` 被默默放行）。

---

## 18. 文件树 / 工作区

### 18.1 现状描述
**CLI 本身没有 file tree sidebar**。VS Code / JetBrains 扩展用 IDE native file explorer。

**Workspace 多目录**：`claude --add-dir ../apps ../lib` 或 `/add-dir <path>` 添加运行时目录。注意：「Most `.claude/` configuration is **not** discovered from these directories」—— 仅 grant file access，**不读 settings/agents/commands/output-styles**（**例外**：`.claude/skills/` 是会从 `--add-dir` 加载的）。

**`settings.json` `permissions.additionalDirectories`**：持久化多目录。

**Worktree（git）**：`claude --worktree feature-auth` 创建独立 git worktree（在 `.claude/worktrees/feature-auth/`），自动切到该目录的 Claude session。Worktree 退出时按是否有改动决定是否清理。`.worktreeinclude` 文件控制 gitignored 文件（如 `.env`）自动拷贝到新 worktree。

**`workspace` JSON 字段 in statusline**：`current_dir`、`project_dir`、`added_dirs`、`git_worktree`。

### 18.2 关键引用
- 🟢 [Add additional directories](https://code.claude.com/docs/en/cli-reference.md)
- 🟢 [Worktrees](https://code.claude.com/docs/en/worktrees.md)
- 🟢 [Permission additional directories caveat](https://code.claude.com/docs/en/permissions.md#additional-directories-grant-file-access-not-configuration)

### 18.3 亮点
- **`--add-dir` 多目录** + 明确的 "配置不发现" 边界 —— 安全模型清晰。
- **`--worktree`** —— git worktree 创建 + 退出清理一条龙，包括 `.worktreeinclude` 拷贝 gitignored 文件。
- **`.claude/skills/` 跨 add-dir 加载**（唯一例外）—— skills 是真正的"可分享配置"。

### 18.4 缺点
- 没有 CLI 内 file tree —— 大型项目导航靠 `@` mention + `Glob` 工具。
- worktree 仅 git，其它 VCS 要写 `WorktreeCreate` hook。

### 18.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.16 的 file_tree.rs 已经实现（懒加载 + git_ignore + 排序），但 sidebar 没启用，**痛点 5 「不走 palette」 + 「在 layout 中未启用」**。建议：
1. **保持 sidebar 实现但不默认启用**（与 Claude Code 一致：CLI 不要 sidebar）。
2. **加 `/add-dir` 命令** + `--add-dir` flag：现状档案 §1.3 没有这个能力。
3. **加 `--worktree` 支持**：这是一个完整的新子系统（git 命令封装 + WorktreeCreate hook），对应 §16 sub-agents 的 `isolation: worktree`。
4. **`.deepseekignore` / `.worktreeinclude` 这一类用户控制文件**。

---

## 19. 设置

### 19.1 现状描述
**4 个 scope（优先级从高到低）**：
| Scope | Location | Affects | Shared? |
|---|---|---|---|
| Managed | `/Library/Application Support/ClaudeCode/managed-settings.json`（macOS）/ `/etc/claude-code/`（Linux）/ `C:\Program Files\ClaudeCode\`（Win），或 MDM plist / registry / 服务器管理 | All users on machine | Yes (IT) |
| User | `~/.claude/settings.json` | You, all projects | No |
| Project | `.claude/settings.json` | Team (committed) | Yes |
| Local | `.claude/settings.local.json` | You, this project | No (gitignored) |

**优先级**：Managed > CLI args > Local > Project > User。**Permission rules 是 merge（不 override）**。

**Settings schema**（关键字段，settings.json 文档单页 124 KB —— 几十个字段，最重要的 30 个：）
- `agent`、`alwaysThinkingEnabled`、`apiKeyHelper`、`attribution.commit/pr`、`autoMemoryEnabled`、`autoMemoryDirectory`、`autoMode.environment/allow/soft_deny/hard_deny`、`claudeMdExcludes`、`color`、`companyAnnouncements`、`defaultMode`、`disableSkillShellExecution`、`editorMode`、`effortLevel`、`enabledPlugins`、`env`、`fastMode`、`fastModePerSessionOptIn`、`hideVimModeIndicator`、`hooks`、`includeCoAuthoredBy`、`language`、`mcpServers`、`model`、`outputStyle`、`paddingHorizontal`、`permissions.allow/ask/deny/defaultMode/additionalDirectories/disableAutoMode/disableBypassPermissionsMode`、`refreshInterval`、`sandbox.enabled/filesystem.allowWrite/denyWrite/allowRead/denyRead/network.allowedDomains/deniedDomains/failIfUnavailable/excludedCommands`、`statusLine.type/command/padding/refreshInterval/hideVimModeIndicator`、`teammateMode`、`theme`、`tui`、`useNonInteractiveSpinner`、`viewMode`、`voice`、`worktree.baseRef`。

**`$schema` 引用** + JSON Schema validation in editor。

**Managed-only settings**（admin lock）：`allowedMcpServers`、`allowManagedHooksOnly`、`allowManagedMcpServersOnly`、`allowManagedPermissionRulesOnly`、`fastModePerSessionOptIn`。

**`managed-settings.d/`** drop-in directory：分团队部署独立 policy fragments（按字母排序 deep-merge）。

**MDM 支持**：macOS `com.anthropic.claudecode` plist + Windows `HKLM\SOFTWARE\Policies\ClaudeCode` registry + Group Policy。

**自动备份**：「retains the five most recent backups」。

### 19.2 关键引用
- 🟢 [Settings](https://code.claude.com/docs/en/settings.md)（completest schema）
- 🟢 [Managed settings examples (MDM)](https://github.com/anthropics/claude-code/tree/main/examples/mdm)

### 19.3 亮点
- **4 scope + 优先级清晰**（Managed > CLI > Local > Project > User）+ permission rules merge（不 override）—— 这个细节非常关键。
- **MDM 部署**（plist / registry / managed-settings.d/）—— 企业级。
- **`$schema` JSON Schema 验证** —— VS Code / Cursor 内 autocomplete + inline lint。
- **5 个自动 backup** —— 改坏配置可回滚。
- **`autoMemoryDirectory` 不接受 project/local scope**（防 cloned repo 通过设置重定向 memory 到敏感目录）—— **路径注入防御的精细处理**。

### 19.4 缺点
- 字段太多（settings.json 单页 124 KB）—— 没有按"任务"分类的 cheatsheet，新用户难快速上手。
- TOML vs JSON 之争：Claude Code 选 JSON（schema 友好），但 JSON 写注释不便（要靠 `_comment` 字段 hack）。

### 19.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.9 storage/config.rs（12+ 配置节）**层级已经成熟**，但缺：
1. **明确的 4 scope 层级**：现状档案 §3.9 直接说「缺少 user-level vs project-level 优先级机制」—— 抄 Claude Code 的 4 scope，TOML 文件加一份 `.deepseek/config.local.toml` (gitignored)。
2. **`$schema` JSON Schema / TOML Schema**：deepseek-code 用 TOML，可以发布 `taplo` 的 schema 让 editor 验证。
3. **MDM 部署支持** —— 优先级低，企业付费客户多了再做。
4. **`autoMemoryDirectory` 路径注入防御** —— 直接抄：deepseek-code 的 `[paths]` 节里如果加 memory 目录，**必须只接受 user/managed scope，不接受 project/local**。
5. **5 个自动 backup**：现状档案没看到 backup 机制 —— 加一个简单的 `~/.deepseek/backups/config-YYYYMMDD-HHmmss.toml.bak`，保留 5 个。

---

## 20. 工具系统

### 20.1 现状描述
**32 个内置工具**（[tools-reference.md](https://code.claude.com/docs/en/tools-reference.md)，按字母）：

| Tool | Permission Required | 关键作用 |
|---|---|---|
| `Agent` | No | spawn subagent |
| `AskUserQuestion` | No | 多选问题（结构化 clarification） |
| `Bash` | Yes | shell |
| `CronCreate/CronDelete/CronList` | No | session-scoped 调度 |
| `Edit` | Yes | 增量编辑（old_string/new_string） |
| `EnterPlanMode / ExitPlanMode` | No / Yes | plan mode 切换 |
| `EnterWorktree / ExitWorktree` | No | git worktree 切换 |
| `Glob` | No | 文件匹配（native 二进制中已替换为 `bfs`） |
| `Grep` | No | 内容搜索（native 中已换 `ugrep`） |
| `ListMcpResourcesTool / ReadMcpResourceTool` | No | MCP resources |
| `LSP` | No | language server intelligence |
| `Monitor` | Yes | 后台运行命令，按行 stream 回 conversation |
| `NotebookEdit` | Yes | Jupyter cells |
| `PowerShell` | Yes | Windows native shell（opt-in） |
| `PushNotification` | No | desktop + 手机推送（Remote Control 配套） |
| `Read` | No | 读文件 |
| `RemoteTrigger` | No | 管理 Routines（cloud 调度） |
| `SendMessage` | No | agent teams 内 teammate 消息 / resume subagent |
| `ShareOnboardingGuide` | Yes | 上传 ONBOARDING.md 共享链接 |
| `Skill` | Yes | 调用 skill |
| `TaskCreate/TaskGet/TaskList/TaskUpdate/TaskStop` | No | 新版 task list |
| `TaskOutput` | No | deprecated |
| `TeamCreate/TeamDelete` | No | agent teams（实验） |
| `TodoWrite` | No | 老 task list |
| `ToolSearch` | No | MCP tool search（deferred tools 按需加载） |
| `WebFetch` | Yes | 抓 URL |
| `WebSearch` | Yes | 搜索 |
| `Write` | Yes | 创建/覆盖文件 |

**特殊设计**：
- **`Glob` / `Grep` 在 native binaries 中被替换**：v2.1.114 起 macOS/Linux native 二进制把 Glob/Grep 工具替换成 embedded `bfs` 和 `ugrep`，通过 Bash 调用 —— **少一个 tool round-trip**，性能更高。
- **`Bash` background 模式**：`Ctrl+B` 把 in-flight Bash 移到后台（tmux 用户按两次）。后台任务有唯一 ID，输出写文件，Claude 用 Read 取。5GB stdout 自动终止。`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` 关闭。
- **`Monitor`**：独特设计 —— 后台进程，每行输出作为新 transcript message 流入 conversation，Claude 即时反应（不阻塞 turn）。
- **`AskUserQuestion`**：结构化多选 clarification（不是自由 prompt）—— 强制结构化的澄清。
- **`Edit` 的 read-less 优化**：v2.1.90 起 Edit 可以在 `cat` / `sed -n` 看过的文件上直接 edit，不再要求先 Read。

**工具结果限额**：MCP tool 输出超 10K tokens 警告；`MAX_MCP_OUTPUT_TOKENS` 可调；Hook output 超 50K 写文件 + 路径注入。MCP server author 可以加 `_meta.anthropic/maxResultSizeChars` per-tool 提到 500K char。

### 20.2 关键引用
- 🟢 [Tools reference](https://code.claude.com/docs/en/tools-reference.md)
- 🟢 [Monitor tool (Week 15)](https://code.claude.com/docs/en/whats-new/2026-w15.md)
- 🟢 [Background bash](https://code.claude.com/docs/en/interactive-mode.md#background-bash-commands)

### 20.3 亮点
- **32 个工具** + **每个有 frontmatter 说明 permission required** + **细分 sub-category**（task / cron / team / lsp / mcp / monitor / push / notebook / powershell / shellcheck / web）。
- **Monitor tool** —— 不阻塞 turn 的事件驱动消费，搭配 `/loop` 自适应间隔。
- **`Glob`/`Grep` 替换为原生 `bfs/ugrep`** —— 削减 tool round-trip。
- **5GB stdout 自动终止** —— 防内存炸。
- **MCP tool 输出 truncation + per-tool override** —— 大 schema/file tree 类工具单独豁免。

### 20.4 缺点
- 32 个工具 + 各种 alias / deprecated（TodoWrite vs TaskCreate）—— 学习曲线。
- `PowerShell` / `Monitor` / `TeamCreate` 等都是 opt-in，文档分散。

### 20.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.7 的 tools/dispatch.rs（17 个内置工具 + self-heal）**远不够**。建议补：
1. **`Monitor` 等价物**：deepseek-code 现状档案 §2.6 的 status_bar / app.rs 已有 background 概念，加一个真正的 `monitor` tool（spawn process + 按行回流到 conversation）—— 这是 `/loop` 自适应间隔的基础。
2. **`AskUserQuestion`** 结构化 clarification —— 现状档案 §2.10 已有 `render_options_panel` Clarification kind，加一个 tool 调用入口。
3. **`Glob` / `Grep` 替换原生 binaries**：deepseek-code 已用 `walkdir / ignore / glob` crate，性能应该不输，但可以考虑发布时 embed `bfs/ugrep` 二进制做 fallback。
4. **`NotebookEdit` / `LSP` / `PowerShell`** —— optional，做长尾。
5. **统一的 background bash**：现状档案 §3.4 BackgroundQueue 存在，要做成 `Ctrl+B` 一键背景 + 5GB cutoff。
6. **`github_pr` mega-tool 拆分**：现状档案 §3.7 已经标记为问题，按 `action` 字段拆成 `gh_pr_list / gh_pr_get / gh_pr_diff / gh_pr_comment`。

---

## 21. MCP 支持

### 21.1 现状描述
**完整 transport 支持**：
- **stdio**：本地进程，stdin/stdout 通信。
- **HTTP**：streamable HTTP（`type: "http"` 或 alias `"streamable-http"`）。**推荐**。
- **SSE**：Server-Sent Events（`type: "sse"`，已 deprecated 但仍可用）。
- **In-process MCP**（SDK only）：直接在 SDK 应用里定义工具。

**Authentication**：
- 环境变量（`env: {GITHUB_TOKEN: "${GITHUB_TOKEN}"}`）—— 仅 stdio。
- HTTP headers（`headers: {Authorization: "Bearer ${API_TOKEN}"}`）。
- **OAuth 2.1 自动流程**：标记 server 需要 auth 时（HTTP 401 + `WWW-Authenticate` header），`/mcp` 命令开浏览器登录，token 存 keychain。支持 Dynamic Client Registration / 预配置 client_id + secret / Client ID Metadata Document (CIMD)。`--callback-port` 固定回调端口。
- `oauth.scopes`：pinned scopes（"channels:read chat:write search:read"）。
- `authServerMetadataUrl`：覆盖 OAuth metadata discovery。
- **`headersHelper`**：动态 header 生成脚本（10s 超时，每次连接新跑，环境变量 `CLAUDE_CODE_MCP_SERVER_NAME / _URL` 注入）—— 解决 Kerberos / 短期 token / 内部 SSO 等。

**Scope（4 级 + claude.ai connectors）**：
| Scope | Loads in | Shared | Stored in |
|---|---|---|---|
| Local | Current project only | No | `~/.claude.json` per-project |
| Project | Current project only | Yes (`.mcp.json` committed) | `.mcp.json` |
| User | All your projects | No | `~/.claude.json` user-level |
| Plugin-provided | Where plugin enabled | Via plugin | plugin's `.mcp.json` or `plugin.json` |
| claude.ai connectors | Inherited | — | — |

**优先级**：Local > Project > User > Plugin > claude.ai connectors。Plugin / connector match by endpoint (URL/command), 其他 match by name.

**Project scope 的安全审批**：第一次加载 `.mcp.json` 弹审批 dialog，`claude mcp reset-project-choices` 重置。

**`.mcp.json` schema**：
```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"],
      "env": {"FOO": "${FOO:-default}"}
    },
    "github": {
      "type": "http",
      "url": "https://mcp.example.com/mcp",
      "headers": {"Authorization": "Bearer ${API_TOKEN}"},
      "oauth": {
        "clientId": "your-client-id",
        "callbackPort": 8080,
        "scopes": "...",
        "authServerMetadataUrl": "..."
      },
      "headersHelper": "/opt/bin/get-headers.sh"
    }
  }
}
```

**`claude mcp` 子命令**：
```
claude mcp add --transport http <name> <url> [--header "K: V"] [--env K=V] [--scope local|project|user] [--client-id X] [--client-secret] [--callback-port N]
claude mcp add --transport sse <name> <url> [...]
claude mcp add --transport stdio <name> -- <command> <args...>
claude mcp add-json <name> '<json>' [--client-secret]
claude mcp list
claude mcp get <name>
claude mcp remove <name>
claude mcp reset-project-choices
```

**`/mcp` panel**：在 session 内显示每个 server 状态 + tool count + 标记 0-tool 的 server。**自动重连**：HTTP/SSE 断线指数退避（5 次，1s 起步翻倍）。初始连接 transient error 重试 3 次（v2.1.121+）。

**Dynamic tool updates**：MCP server 发 `list_changed` 通知，Claude Code 自动刷新可用 tools。

**Tool search**：默认启用。tool definitions 不全注入 context window，只在 Claude 需要时加载（按需）。**MCP server 可声明 `alwaysLoad: true`**（v2.1.122+） 让该 server 的所有 tools 始终可用，**或 per-tool `anthropic/maxResultSizeChars` `_meta` 字段** 提高 truncation cap 到 500K（v2.1.91+）。

**Channels (research preview)**：MCP server 可声明 `claude/channel` capability，主动 push 消息进 session（用 `--channels plugin:<name>@<marketplace>` 启用），实现 「monitor / webhook → Claude 反应」。

**Plugin 提供 MCP**：plugin root 放 `.mcp.json` 或 `plugin.json` inline 的 `mcpServers` 字段。`${CLAUDE_PLUGIN_ROOT}` / `${CLAUDE_PLUGIN_DATA}` / `${CLAUDE_PROJECT_DIR}` 变量。

**Managed MCP**：admin 在 managed settings 中限制 `allowedMcpServers` / `deniedMcpServers`。

### 21.2 关键引用
- 🟢 [MCP CLI](https://code.claude.com/docs/en/mcp.md)
- 🟢 [SDK MCP](https://code.claude.com/docs/en/agent-sdk/mcp.md)
- 🟢 [Channels](https://code.claude.com/docs/en/channels.md)

### 21.3 亮点
- **transport 全覆盖**（stdio + HTTP + SSE + in-process SDK）。
- **OAuth 2.1 完整流程**自动化 + 固定 callback port + scope pin + metadata URL override + CIMD 支持 —— **企业级**。
- **`headersHelper` 动态 headers** —— 解决非 OAuth 的 Kerberos/SSO 场景。
- **4 scope + 优先级 + project scope 审批** —— 安全 + 协作平衡。
- **Tool search**：deferred tools 按需加载 —— 大量 MCP server 不爆 context。
- **`list_changed` dynamic refresh** —— server 端 tool 列表变更不需要重连。
- **指数退避自动重连** + transient 初始连接重试。
- **Channels** —— 真正的 push 通信，不只是 polling。

### 21.4 缺点
- 配置复杂：4 scope + OAuth 三种模式 + `headersHelper` —— 文档密集。
- `headersHelper` 安全模型脆弱（project scope 用时需要 workspace trust）。

### 21.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.8 直接说「**MCP 只支持 stdio**」—— **这是最大单点差距**。修复路线（应该是 P0 中的 P0）：
1. **加 HTTP transport**：使用 `reqwest`（已依赖）实现 streamable HTTP + SSE 客户端。SSE 用 `tokio` + `reqwest::Response::bytes_stream()`。
2. **加 OAuth 2.1**：用 `oauth2` Rust crate。完整的：自动发现 metadata、Dynamic Client Registration、固定 callback port、scope pin。token 存到 keyring（已有依赖）。
3. **加 4 scope 体系**：local/project/user/plugin —— 现状档案 §3.9 的 McpConfig 只支持 local-ish，要拆。
4. **`.mcp.json` schema 完全兼容 Claude Code** —— 这样用户可以共享 MCP server 定义。
5. **`headersHelper`** —— 优先级低，但企业客户会问。
6. **`list_changed` dynamic refresh** —— protocol 层补。
7. **Tool search**：现状档案 §3.7 的 17 工具都 always-on，加几个 MCP server 后 context 会爆 —— 必须做 deferred tool search。
8. **Plugin 提供 MCP** —— 等 plugin 系统先做（见 §27）。

---

## 22. Auth / Login

### 22.1 现状描述
**3 种 auth backend**：
- **Claude.ai 账户**（Pro / Max / Team / Enterprise）：`claude` 第一次启动开浏览器登录；token 存 macOS Keychain / Linux `~/.claude/.credentials.json` (0600) / Windows `%USERPROFILE%\.claude\.credentials.json`。
- **Claude Console API key**：通过 Console 配置；admin 邀请用户后用户 `claude auth login --console` 登录；同样的 credentials 文件。
- **Cloud provider**：Bedrock / Vertex / Foundry —— 设环境变量 `CLAUDE_CODE_USE_BEDROCK=1` 等，不需要 browser login。

**`claude auth` 子命令**：`login` (`--email`, `--sso`, `--console`) / `logout` / `status` (`--text`)（exit code 0/1）。`setup-token` 生成长效 OAuth token（CI 用）。

**Auth precedence**：cloud provider → `ANTHROPIC_AUTH_TOKEN` → `ANTHROPIC_API_KEY` → `apiKeyHelper` → keychain token。

**`apiKeyHelper`** 设置：shell script 输出 API key，默认 5min 或 401 触发 refresh；`CLAUDE_CODE_API_KEY_HELPER_TTL_MS` 调；>10s 显示 warning。

**Browser-less login**（v2.1.126+）：登录后浏览器无法回调时（WSL2 / SSH / 容器），可以把 OAuth code 直接粘贴到终端。

**Cross-machine login**：通过 Claude.ai 账户跨设备 syncs；Console API key 是设备无关。

### 22.2 关键引用
- 🟢 [Authentication](https://code.claude.com/docs/en/authentication.md)

### 22.3 亮点
- **3 backend + 4 plan + 3 OS 存储位置** —— 完整。
- **OAuth Browser-less paste-code fallback** —— SSH/WSL2/容器友好。
- **`apiKeyHelper`** —— 公司内 rotation 友好（短期 token + refresh）。
- **Console 团队管理 + SSO** —— 企业级。
- **`claude auth status` exit code** —— 脚本可检测。

### 22.4 缺点
- 4 plan + 3 provider + 4 admin layers，文档矩阵复杂。
- token rotation 错配场景多（OAuth refresh race，但 v2.1.128 修了）。

### 22.5 对 deepseek-code 的启示
deepseek-code 现状档案痛点「OAuth 登录」明确缺。建议：
1. **`auth login/logout/status` 三件套** + `--api-key` (现有) + `--web`（OAuth）。
2. **DeepSeek 账户 OAuth**：如果 DeepSeek 没有 OAuth endpoint 就只能 API key + Claude.ai 风格的 long-lived token。
3. **`apiKeyHelper`** —— 企业短期 token rotation，直接抄 Claude Code 的 shell script schema。
4. **Browser-less paste-code fallback** —— WSL/SSH/容器用户体验直接抄。

---

## 23. Session 持久化

### 23.1 现状描述
**Session 模型**：每次启动 `claude` 创建 session（UUID）。可命名（`--name "my-feature"` / `/rename`）。**`--continue`**：当前目录最近 session。**`--resume <id|name>`**：按 ID 或 name 选；不带参打开 picker。**`--from-pr <number|url>`**：按 PR 链接找到创建该 PR 的 session（v2.1.122+，支持 github.com / GitHub Enterprise / GitLab / Bitbucket）。

**`--fork-session`**：resume 时创建新 session ID（不复用原 ID）。`/branch [name]` (aka `/fork`)：在当前 session 当前点 fork 出独立 branch session。

**`/teleport`**：Claude Code on the web 的 session 一键拉到本地 terminal。`--remote "task"`：在 claude.ai 上开 cloud session。

**Storage**：`~/.claude/projects/<project-hash>/` 下：
- transcripts（jsonl）
- task lists
- file edit history（**checkpointing**！见 §15）
- debug logs
- prompt history lines
- `~/.claude.json` 的 per-project state（trust / allowed-tools cache）

`claude project purge [path]`：删除项目所有本地状态（`--dry-run`、`-i` 交互、`--all` 全部、`-y` skip 确认）。

**Auto cleanup**：30 天（`cleanupPeriodDays` 可配）。

**Auto memory** （§25 详述）的 `~/.claude/projects/<project>/memory/` 是 session 之外的 per-repo memory，跨 worktree 共享。

### 23.2 关键引用
- 🟢 [Sessions](https://code.claude.com/docs/en/agent-sdk/sessions.md)
- 🟢 [project purge](https://code.claude.com/docs/en/cli-reference.md)
- 🟢 [/resume / --from-pr](https://code.claude.com/docs/en/whats-new/2026-w18.md)

### 23.3 亮点
- **`--from-pr`** —— 通过 PR URL 回到创建它的 session，极强的"工作 → 上下文"对齐。
- **`--fork-session`** —— resume 时不重用原 ID，避免覆盖。
- **`/branch` (fork) vs. `/fork` (subagent)** —— 通过 env var `CLAUDE_CODE_FORK_SUBAGENT` 切换语义，老 alias 保留。
- **30 天 auto cleanup** + `project purge` —— 卫生。

### 23.4 缺点
- session 文件格式（jsonl）专有，没有 round-trip 编辑工具。
- 大 session resume 慢（v2.1.114 起优化 67%，但仍可能秒级）。

### 23.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.4 / §3.9 已经有 sessions / transcripts / events.jsonl 持久化。补的：
1. **`--from-pr`** —— 抄。需要 link session ↔ PR 的机制：commit/PR 创建时把 session_id 写到 commit footer 或 PR description。
2. **`--fork-session`** —— `--resume` 时加 `--fork-session` 不复用 ID。
3. **`/branch`** —— 在 transcript 当前位置分叉。
4. **`project purge`** —— 现状档案 §3.9 没有显式的清理命令。
5. **`/teleport` 等价物**：DeepSeek 没有 cloud session，这条略。

---

## 24. Cost / token 显示

### 24.1 现状描述
**`/usage`**（=`/cost`=`/stats`，三个 alias）：分 tab：
- **Cost**：session $ + per-model breakdown + cache hit %
- **Limits**：5h window + 7d window 百分比 + reset_at
- **Stats**：lines added/removed、token totals、duration

**v2.1.105+** 新增 "/usage breakdown"：显示什么在驱动你的 limit（并行 sessions / subagents / cache misses / 长 context），各占 24h 百分比 + 优化 tip。`d/w` 切日/周视图。

**`exceeds_200k_tokens`** statusline 字段：超 200k 提示（cache + input + output 总和）。

**Context window 可视化**：`/context [all]`：彩色 grid 显示当前 context 用法 + 优化建议（context-heavy tools、memory bloat、capacity warnings）。

**Effort level**：`/effort` + slider（`low/medium/high/xhigh/max`，**`xhigh` v2.1.105 新加**为 Opus 4.7 的甜点档）。`/effort` 无参数打开 interactive arrow-key slider。

**Pricing** （opus / sonnet 标准 + fast mode $30/$150）公开。

**`--max-budget-usd 5.00`**（print mode only）—— 强制预算上限。

### 24.2 关键引用
- 🟢 [Costs](https://code.claude.com/docs/en/costs.md)
- 🟢 [Statusline > context_window fields](https://code.claude.com/docs/en/statusline.md)
- 🟢 [Effort level (Week 16)](https://code.claude.com/docs/en/whats-new/2026-w16.md)

### 24.3 亮点
- **`/usage breakdown`** —— 不只显示 "你花了多少"，**显示 "什么让你花的"**（subagents / cache miss / parallel）。
- **5h + 7d 双时间窗**显示 rate limit。
- **`/context all`** —— context window 可视化为 grid，找 memory bloat / 大文件 / 累积工具结果。
- **`xhigh` effort 档** + interactive slider —— effort 不只 3 档了。
- **`--max-budget-usd`** —— scripted 场景安全网。

### 24.4 缺点
- `/usage` 数据局部（不能精确预测 5h 用完）。
- effort slider 在 non-fullscreen 模式 UX 一般。

### 24.5 对 deepseek-code 的启示
deepseek-code 现状档案 §2.7 statusline 已有 11 chip 显示 cost / context %，但**没有 `/usage breakdown`**。建议：
1. **`/usage` 命令**：tab 化（Cost / Limits / Stats），breakdown 显示什么在烧 token。
2. **`/context all` grid 可视化**：把 transcript 内每条 message 的 token 占用画成 colored grid，找 bloat。
3. **`--max-budget-cny`** 等价（用 CNY）—— 现状档案 §2.7 已 ¥ 硬编码，这条延续。
4. **Effort slider**：deepseek-code 当前用 thinking on/off 二值；可改成 4 档 + slider。

---

## 25. Memory 系统

### 25.1 现状描述
**两个独立系统并存**：

**1. CLAUDE.md（用户写）**：4 层 scope，加载顺序（broadest → most specific）：
| Scope | Location | Purpose |
|---|---|---|
| Managed policy | `/Library/Application Support/ClaudeCode/CLAUDE.md`（macOS）/ `/etc/claude-code/CLAUDE.md`（Linux）/ `C:\Program Files\ClaudeCode\CLAUDE.md` | Org-wide |
| User | `~/.claude/CLAUDE.md` | Personal preferences |
| Project | `./CLAUDE.md` or `./.claude/CLAUDE.md` | Team-shared |
| Local | `./CLAUDE.local.md` | Personal project-specific (gitignored) |

**Walk-up resolution**：从 cwd 向上找每层目录的 `CLAUDE.md` + `CLAUDE.local.md`，concat 后注入。子目录 `CLAUDE.md` 在 Claude 读子目录文件时按需加载。

**`@path/to/import`** 语法：CLAUDE.md 中递归 import（最多 5 跳），第一次外部 import 弹审批 dialog。

**`AGENTS.md` 不读**，但官方推荐 `@AGENTS.md` import 进 CLAUDE.md 共存。

**`.claude/rules/*.md`**：组织规则 modular 化，`paths:` frontmatter 限制规则只在匹配的文件类型 active：
```yaml
---
paths:
  - "src/api/**/*.ts"
---
```

`.claude/rules/` 支持 symlink + 共享。`~/.claude/rules/` 用户全局。

**`claudeMdExcludes`** setting：glob 排除某些 CLAUDE.md（monorepo 跨团队场景）。

**HTML 块级注释** `<!-- maintainer notes -->` 在加载时剥离（节省 token）。

**2. Auto memory（Claude 自己写）**：
- 启用：`autoMemoryEnabled: true`（默认 on，v2.1.59+），`/memory` 切换。
- 存储：`~/.claude/projects/<project>/memory/`，`<project>` 由 git repo derive，跨 worktree / 子目录共享。`autoMemoryDirectory` setting 可改（仅 user/managed scope 接受，防注入）。
- 结构：`MEMORY.md`（索引，前 200 行 / 25 KB 注入每次 session）+ topic files（debugging.md / api-conventions.md / etc.，按需读）。
- Claude 写："Writing memory" / "Recalled memory" 视觉信号在 UI 显示。
- 用户操作："always use pnpm, not npm" → Claude 写到 auto memory；"add this to CLAUDE.md" → Claude 改文件。

**`/memory` 命令**：列出当前 session 加载的所有 CLAUDE.md / CLAUDE.local.md / rules，toggle auto memory，打开 auto memory 文件夹链接。

**Compaction 影响**：项目根 `CLAUDE.md` 跨 compaction 重读注入；nested CLAUDE.md 不会自动；任务列表持久化。

**`InstructionsLoaded` hook**：log 哪些文件加载了 + 为什么（debug 工具）。

### 25.2 关键引用
- 🟢 [Memory](https://code.claude.com/docs/en/memory.md)（最长最详细一页）
- 🟢 [.claude/rules/](https://code.claude.com/docs/en/memory.md#organize-rules-with-claude/rules/)

### 25.3 亮点
- **CLAUDE.md（手写）+ auto memory（Claude 写）二元** —— 显式 vs 学习。
- **Walk-up resolution** + import + `paths:` scoped rules —— 精细。
- **`autoMemoryDirectory` 仅 user/managed scope**（不接受 project/local）—— 路径注入防御。
- **`@AGENTS.md` import** —— 共存其他工具的 instructions 文件。
- **HTML 注释剥离** —— 内部 maintainer notes 不烧 token。
- **`/memory` 列出加载内容**——透明可审计。
- **跨 compaction 重新注入 root CLAUDE.md** —— context 被压缩后规则不丢。

### 25.4 缺点
- 4 scope + auto memory + rules 三层叠加，新用户难理解什么在生效。
- auto memory 写错时（学了错的 build 命令）需要手动清理。

### 25.5 对 deepseek-code 的启示
deepseek-code 现状档案 §1.3 已有 AGENTS.md 概念（welcome page 显示状态）；但**没有 auto memory** —— **核心差距**。建议路线：
1. **CLAUDE.md / AGENTS.md / `.deepseek/rules/*.md` 三件套**（用 AGENTS.md 做项目主入口，CLAUDE.md 兼容 import）。
2. **Auto memory**：在 `~/.deepseek/projects/<hash>/memory/MEMORY.md` 累积；写入触发条件：用户说 "remember that..." / "always..." / 在 conversation 中明确教学。
3. **`.deepseek/rules/*.md` + `paths:` frontmatter** —— modular。
4. **`InstructionsLoaded` hook** —— debug 用。
5. **`autoMemoryDirectory` 路径注入防御** —— 抄。
6. **`/memory` 命令**：deepseek-code 已经有 `/memory` 命令（现状档案 §3.10）—— 确保实现深度到位（不只是 placeholder）。

---

## 26. Hooks

### 26.1 现状描述
**19 个 hook events**（[hooks.md](https://code.claude.com/docs/en/hooks.md)）：
| Event | Python | TS | When |
|---|---|---|---|
| `PreToolUse` | Y | Y | 工具调用前（可 deny / allow / 修改 input） |
| `PostToolUse` | Y | Y | 工具调用后 |
| `PostToolUseFailure` | Y | Y | 工具失败 |
| `PostToolBatch` | N | Y | 整批工具调用完成 |
| `UserPromptSubmit` | Y | Y | 用户提交 prompt |
| `Stop` | Y | Y | Agent 停止（含 SubagentStop 在 frontmatter 中自动转换） |
| `SubagentStart` | Y | Y | Subagent 启动 |
| `SubagentStop` | Y | Y | Subagent 结束 |
| `PreCompact` | Y | Y | 压缩前（可 block） |
| `PermissionRequest` | Y | Y | Permission dialog 会显示前（自定义 permission 逻辑） |
| `SessionStart` | N | Y | Session 启动（matcher: `compact` / 普通启动） |
| `SessionEnd` | N | Y | Session 结束 |
| `Notification` | Y | Y | UI 状态消息（matchers: `permission_prompt / idle_prompt / auth_success / elicitation_*`） |
| `Setup` | N | Y | Session setup/maintenance（init/maintenance matcher） |
| `TeammateIdle` | N | Y | teammate 空闲 |
| `TaskCompleted` | N | Y | 后台任务完成 |
| `ConfigChange` | N | Y | 配置文件变更 |
| `WorktreeCreate / WorktreeRemove` | N | Y | git worktree 创建/移除 |
| `PermissionDenied` (auto mode) | Y | Y | Classifier denial（return `retry: true`） |
| `CwdChanged / FileChanged` | Y | Y | direnv-style |
| `InstructionsLoaded` | Y | Y | CLAUDE.md / rules 加载（debug） |

**Hook types**：`command`（shell command）/ `mcp_tool`（直接调用 connected MCP tool，v2.1.115+，无需 spawn process）。

**Matcher**：regex 对 event 的 filter field 匹配（tool 名 / notification 类型 / 等）。**`if` 字段**（v2.1.85+）：permission rule syntax 限定（如 `if: "Bash(git commit *)"`）。

**Hook callback inputs**（stdin JSON）：tool 名、tool_input、session_id、cwd、`$CLAUDE_PROJECT_DIR`、`$CLAUDE_EFFORT`（v2.1.135+）、`$CLAUDE_CODE_SESSION_ID`（v2.1.135+）。

**Hook outputs**（stdout JSON / exit code）：
- exit code 0：成功，stdout 是 informational
- exit code 2：deny（per-event 语义不同：PreToolUse block tool；UserPromptSubmit block prompt；PreCompact block compaction）
- 其他非零：失败，stderr 显示
- JSON output：`{"hookSpecificOutput": {...}}`，per-event schema（PreToolUse 可返回 `permissionDecision: allow/deny/ask/defer + permissionDecisionReason + updatedToolInput`；PostToolUse 可 `updatedToolOutput`；UserPromptSubmit 可 `additionalContext + sessionTitle`；SessionStart 输出加入 context；PreCompact `decision: block`）

**Async hooks**：长任务可后台运行，结果通过下一个 turn 注入。

**`prompt-based hooks` / `agent-based hooks`**：不只 shell —— 可以让 Claude 模型自己做条件评估的 hook（少用，对应高级场景）。

**Hooks in skills/agents frontmatter**：scoped 生命周期。

**Plugin hooks**：plugin 的 `hooks/hooks.json`。Managed settings 可 lock `allowManagedHooksOnly: true`。

### 26.2 关键引用
- 🟢 [Hooks (full reference, 179 KB)](https://code.claude.com/docs/en/hooks.md)
- 🟢 [Hooks guide (54 KB)](https://code.claude.com/docs/en/hooks-guide.md)
- 🟢 [SDK Hooks](https://code.claude.com/docs/en/agent-sdk/hooks.md)

### 26.3 亮点
- **19 个 events** —— 覆盖整个生命周期（pre/post tool、batch、prompt submit、stop、compact、session start/end、permission request、notification、setup、teammate、task、worktree、cwd/file changed、auto-mode denied、instructions loaded）。
- **`mcp_tool` hook type** —— 不 spawn process 直接调 connected MCP tool。
- **`if` field**（permission rule syntax）—— 不需要每次 spawn process，更便宜。
- **`defer` permissionDecision** —— SDK 在 `-p` 模式可拉出 `deferred_tool_use` payload，自定义 UI 处理后 resume。
- **`mcp_tool` + `command` 类型 + 异步 hook + prompt-based hook** —— 灵活到任何场景。
- **`updatedToolInput` / `updatedToolOutput`** —— PreToolUse 可修改即将执行的参数，PostToolUse 可替换输出（v2.1.122+ 对所有 tool 生效，之前只 MCP）。
- **5 KB stdout limit**：超 50 KB 自动写文件 + 路径注入（避免 context bloat）。

### 26.4 缺点
- 19 events 学习成本高，每个 event 的 input/output schema 不同。
- shell hooks 的安全模型靠 `$CLAUDE_PROJECT_DIR` 等约定，project-scope hook 第一次跑前要 workspace trust。

### 26.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.10 已有 `/hooks` 命令但实现深度未知。建议**整套抄 Claude Code 的 hook schema**：
1. **19 events 全集**：是 deepseek-code 的差异化升级机会 —— 比现有 hooks 少肯定有。
2. **`if` field**：permission rule syntax 限定 matcher。
3. **`mcp_tool` type**：现状档案 §3.8 MCP 只 stdio，但 hook 调 MCP tool 是个差异化好设计。
4. **`updatedToolInput` / `updatedToolOutput`**：PreToolUse 可改参数，PostToolUse 可改输出 —— 直接对接现状档案 §3.6 defense/output.rs。
5. **`InstructionsLoaded` hook** —— debug。
6. **Hook input JSON 通过 stdin、output 通过 stdout/exit code**：deepseek-code 用 Rust，可以直接 spawn shell + pipe，跟 Claude Code 模式一致。

---

## 27. Plugins / 生态

### 27.1 现状描述
**Plugin = 目录 + `.claude-plugin/plugin.json` manifest**。Plugin 可包含：
- `.claude-plugin/plugin.json`（manifest：name / description / version / author / homepage / repository / license / dependencies）
- `skills/<name>/SKILL.md` —— 命名空间 `<plugin-name>:<skill-name>`
- `commands/*.md`（老路径，兼容）
- `agents/<name>.md`
- `hooks/hooks.json`
- `.mcp.json` 或 inline `mcpServers` in plugin.json
- `.lsp.json`（LSP servers，v2.1.91+）
- `monitors/monitors.json`（background monitors，v2.1.105+）
- `bin/` —— 可执行加入 Bash tool PATH（v2.1.91+）
- `settings.json` —— plugin 启用时默认 settings
- `themes/*.json`（v2.1.118+）
- `output-styles/*.md`

**安装方式**：
- `claude plugin install <name>@<marketplace>`
- `--plugin-dir <path>` / `--plugin-url <https://...>.zip`（v2.1.128+ 加了 URL）
- Marketplaces：用户配置可信 marketplace 列表（`enabledPlugins` setting）

**`claude plugin` 子命令**：`install / uninstall / list / update / prune / tag` 等。`/plugin install <name>@<marketplace>` 在 session 内。`/reload-plugins` 应用 enable/disable 改动。

**Channels**：plugin 可声明 `claude/channel` MCP capability，通过 `--channels plugin:<name>@<marketplace>` 启用，server push 消息进 session。

**Marketplaces**：第三方 marketplace。官方有 `claude-plugins-official`（一些工具）和 `claude-plugins-community`（社区）。

**Managed**：`enabledPlugins` 在 managed settings 可强制启用；`strictKnownMarketplaces` 限定可信 marketplace。

### 27.2 关键引用
- 🟢 [Plugins](https://code.claude.com/docs/en/plugins.md)
- 🟢 [Plugins reference](https://code.claude.com/docs/en/plugins-reference.md)
- 🟢 [Plugin marketplaces](https://code.claude.com/docs/en/plugin-marketplaces.md)
- 🟢 [Channels](https://code.claude.com/docs/en/channels.md)

### 27.3 亮点
- **一个 plugin 打包 skills/agents/hooks/MCP/LSP/themes/output-styles/monitors/bin** —— 真正的生态架构。
- **`bin/` 加入 PATH** —— plugin 可分发 CLI 工具。
- **`monitors/`** —— plugin 可定义 session 启动时自动 arm 的后台监听。
- **Channels** —— plugin 可 push 进 session。
- **Marketplaces** + managed lockdown —— 企业可信生态。
- **Plugin themes** —— 主题也可分享。
- **`/reload-plugins`** —— 不需要重启 session。

### 27.4 缺点
- Plugin schema 大（10+ component 类型）—— 学习曲线。
- Plugin security model：所有可执行（bin/、hooks shell command、MCP servers）跑用户权限 —— 信任 marketplace 关键。

### 27.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.10 已有 `/plugins` 命令但实现深度未知。建议路线（这是大工程，P1 后期）：
1. **Plugin schema 完全照抄 Claude Code**：`.deepseek-plugin/plugin.json` + skills/agents/hooks/.mcp.json/etc.
2. **`--plugin-dir / --plugin-url` flag**。
3. **Channels** —— 等 MCP push 通信先做。
4. **Plugin marketplace** —— 长期工程，可先做 GitHub-as-marketplace（plugin 仓库 = marketplace entry）。

---

## 28. IDE 集成

### 28.1 现状描述
**VS Code 扩展**（推荐，最完整）：
- VS Code 1.98+
- 自带 CLI 二进制（不需要单独安装）
- 编辑器工具栏右上角 Spark icon 唤起 panel
- Activity Bar / Command Palette / Status Bar 三个入口
- **`Option+K` / `Alt+K`**：把选中文本作为 @-mention（含文件名 + 行号）插入 prompt
- 自动看到选中文本（footer 显示行数）
- 拖拽文件入 prompt 加 attachment（Shift+drag）
- **Plan mode 在 IDE 中是 markdown 文档**：可加 inline comments 给反馈
- **Side-by-side diff**：IDE native，比 CLI 强
- 多 tab / 多窗口 conversation
- 历史 sessions sidebar

**`Cursor` 扩展**：和 VS Code 同一 extension。**Windsurf / Kiro** 等其他 VS Code fork 也支持。

**JetBrains 插件**（IntelliJ / WebStorm / PyCharm / RustRover / etc.）：
- 在 IDE 内置 terminal 运行 Claude Code CLI
- `Shift+Tab` 切 mode（terminal native）
- `--permission-mode` 启动 flag

**Cursor 兼容性**：原生支持（同一 VS Code extension）。

**`/ide`**：管理 IDE 集成状态。

**Inline edit**：VS Code 扩展中 diff 是 side-by-side（IDE native），可在 diff 中直接编辑 proposed content 再 accept —— Claude 收到通知 "you modified it"。

**Auto-context**：VS Code 中 Claude 自动看到当前选中的代码。

### 28.2 关键引用
- 🟢 [VS Code](https://code.claude.com/docs/en/vs-code.md)
- 🟢 [JetBrains](https://code.claude.com/docs/en/jetbrains.md)
- 🟢 [Cursor compat](https://code.claude.com/docs/en/vs-code.md)（mentioned）

### 28.3 亮点
- **`Option+K` 一键 @-mention 选中区** —— 鼠标用户最爱。
- **Plan mode in IDE = markdown doc with inline comments** —— review 体验远超 CLI。
- **Side-by-side diff with edit** —— IDE native 优势充分用。
- **VS Code fork 全覆盖**（Cursor / Windsurf / Kiro）。
- **JetBrains 集成轻量**：直接复用 CLI in IDE terminal。

### 28.4 缺点
- VS Code 扩展是闭源，不能改。
- JetBrains 比 VS Code 体验差（只是 terminal embed）。

### 28.5 对 deepseek-code 的启示
deepseek-code 现状档案痛点中明确缺 IDE 扩展。路线（长期）：
1. **VS Code 扩展**（同时跑在 Cursor / Windsurf）—— TypeScript + Spawn deepseek-code CLI。
2. **JetBrains 插件**：直接 embed CLI in terminal（简单）。
3. **`/ide` 命令**：CLI 自查 IDE 连接状态。
4. **`Option+K` 选中即 @-mention** —— 设计简单，重要。

---

## 29. Sandbox / Bypass / 危险 flag

### 29.1 现状描述
**OS-level sandbox**（v2.1.84+）：
- macOS：Seatbelt
- Linux：bubblewrap（依赖 `socat`）
- WSL2：bubblewrap
- WSL1：不支持

**Sandbox modes**（`/sandbox`）：
1. **Auto-allow**：sandboxed bash 自动通过；不可 sandbox 的（外网到非 allowlist host）fall back 到 prompt。`rm`/`rmdir` 触及 `/` 或 home 仍 prompt。
2. **Regular permissions**：sandbox enforce + 每个命令仍要 approval。

**Filesystem**：
- 默认 write：仅 cwd 及其子目录
- 默认 read：整机（除特定 deny）
- `sandbox.filesystem.allowWrite / denyWrite / allowRead / denyRead` 配置
- Path prefix 语义：`/abs`、`~/home-relative`、`./project-relative`（与 Read/Edit permission rule 的 `//path` 不一样，注意区别！）
- 跨 scope merge（不 override）
- `allowManagedReadPathsOnly`：managed settings 锁

**Network**：
- 默认 deny
- `sandbox.network.allowedDomains` 白名单（host-based，TLS 不解密）
- 新 domain 弹 prompt（除非 `allowManagedDomainsOnly`）
- `sandbox.network.deniedDomains` 从宽 allow 中扣除
- Custom proxy：高级用户自实现 outgoing 规则

**Bypass flags**：
- `--dangerously-skip-permissions`：等同 `--permission-mode bypassPermissions`。Hook 仍执行。Linux/macOS root/sudo 拒启动（除非 recognized sandbox）。
- `--allow-dangerously-skip-permissions`：把 bypassPermissions 加入 Shift+Tab cycle，但不立即激活（允许从 plan mode 切过去）。
- `--dangerously-load-development-channels`：解锁未在 allowlist 的 channels（local dev）。

**`bypassPermissions` v2.1.126+ 也 bypass 写 protected paths**（`.git/`、`.claude/`、`.vscode/`、shell rc）—— 之前仍 prompt；现在为 catastrophic 命令 (`rm -rf /` / `rm -rf ~`) 保留 safety net。

**`sandbox.failIfUnavailable`**（managed）：sandbox 起不来时硬失败（不 fall back 到 prompt）。

**Devcontainer**：官方 dev container 配置 + non-root user，是推荐的 bypassPermissions 容器。

### 29.2 关键引用
- 🟢 [Sandboxing](https://code.claude.com/docs/en/sandboxing.md)
- 🟢 [Permission modes > bypass](https://code.claude.com/docs/en/permission-modes.md#skip-all-checks-with-bypasspermissions-mode)

### 29.3 亮点
- **OS-level enforcement**（Seatbelt / bubblewrap）—— 不是 in-process scan，绕过难度高。
- **Network proxy** 接管所有 outgoing traffic（不只 Claude 自己的 WebFetch）。
- **`failIfUnavailable`** —— 企业部署的 hard gate。
- **`--allow-dangerously-skip-permissions`** —— 不激活但加入 cycle，权限渐进升级。
- **Devcontainer 推荐** —— 安全容器化的 happy path。

### 29.4 缺点
- 配置复杂（OS-specific 安装步骤 + AppArmor 配置 + scope merge 规则）。
- TLS 不解密 —— 防泄漏不彻底（hostname-based only）。
- WSL1 不支持 —— 老 Windows 用户被排除。

### 29.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.5 / §3.6 的 policy + defense 5 层是**静态扫描** —— 绕过容易（恶意 prompt 让 Claude 把 dangerous command 包装成"看起来安全"的形式就能过）。**这是真正的安全模型差距**。
1. **加 OS-level sandbox**：macOS Seatbelt + Linux bubblewrap，等价 Claude Code 实现 —— P1 大头。
2. **`--allow-dangerously-skip-permissions`** 语义：cycle 加入但不激活。
3. **`failIfUnavailable`**：企业部署 hard gate。
4. **Devcontainer 模板**：发布一个官方推荐的 docker dev container。

---

## 30. 错误处理 / context window 满 / `/compact`

### 30.1 现状描述
**`/compact`**：summarize conversation so far。可选 focus instructions。**保留**：root CLAUDE.md（重读注入）/ task list / auto memory。**丢失**：conversation-only context (没写进 CLAUDE.md 的)。Nested CLAUDE.md 在 Claude 重读子目录文件时自动 reload。

**`PreCompact` hook**：可 block compaction（exit code 2 或 `{"decision":"block"}`）；可 re-inject context via `SessionStart` hook with `compact` matcher (echo to stdout)。

**Auto-compact**：close to limit 时自动触发。`/context [all]` 可视化使用 grid。

**`Esc + Esc` rewind / summarize**：选择 message → restore code/conversation/both / summarize from here（与 `/compact` 区别：targeted，只压缩 selected 之后）/ never mind。

**Rate limit handling**：5h + 7d 双窗口。Fast mode 有独立 pool。命中限 fall back（fast → standard speed；session quota → `/extra-usage`）。auto-retry on transient 5xx / connection refused / timeout，最多 3 次（initial）+ 5 次（mid-session HTTP/SSE reconnect 指数退避）。

**Fallback model**：`--fallback-model sonnet`（仅 print 模式）。

**`api_retry` event**（stream-json 输出）：可被 SDK 监听。

**Out-of-context error / context window warning UI**：通过 statusline `exceeds_200k_tokens` + `context_window.used_percentage` 自定义；`/context` 命令显示 grid + 优化 tip + capacity warning。

### 30.2 关键引用
- 🟢 [Context window](https://code.claude.com/docs/en/context-window.md)
- 🟢 [Checkpointing (Esc Esc)](https://code.claude.com/docs/en/checkpointing.md)
- 🟢 [PreCompact hook](https://code.claude.com/docs/en/hooks.md)
- 🟢 [api_retry events](https://code.claude.com/docs/en/headless.md)

### 30.3 亮点
- **`Esc + Esc` "summarize from here"** —— targeted compact，不全压。
- **PreCompact hook 可 block** —— 让 hook 决定何时该压。
- **`/context all` grid 可视化** —— 找谁在烧 token。
- **Fallback model** + **5xx auto-retry** + **rate-limit fall back to standard** —— 三层容错。
- **`api_retry` stream event** —— SDK 可显示 retry 状态。

### 30.4 缺点
- Auto-compact 阈值不可调（仅靠用户主动 `/compact`）。
- Compact 后某些 conversation-only instruction 丢失，要写到 CLAUDE.md 才稳。

### 30.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.7 有 `apply_patch` / `edit_file` 自愈，但 compact / context 满处理不明。建议：
1. **`/compact` + targeted summarize from rewind menu**（结合 §15 的 Esc+Esc）。
2. **`PreCompact` hook** —— 用户可拦截。
3. **`/context` grid 可视化** —— 找谁在烧。
4. **fallback model + retry** —— DeepSeek Pro overloaded → fall back to Flash。

---

## 31. 长任务 / 后台 / background bash

### 31.1 现状描述
**Background bash**（`Ctrl+B` 或 prompt 要求 background）：
- async run，立即返回 task ID
- 输出写文件，Claude 用 Read 取
- 5GB stdout 自动 terminate（stderr 写原因）
- 退出 session 时清理

**Background tools**：上面 Bash + **Monitor tool**（事件流回 conversation）+ **CronCreate/CronList/CronDelete**（session-scoped 调度）。

**`/tasks` / `/bashes`**：列出当前 background tasks。

**Background subagent**：`background: true` frontmatter 或 `--bg` flag → subagent 后台跑 → `claude agents` 监控。

**Background session**（`claude --bg "task"` 或 `/background` / `/bg`）：detach session 跑 background，返回 session ID。`claude agents` 监控，`claude attach <id>` 重连，`claude logs <id>` 查最近输出，`claude stop <id>` / `kill` 停，`claude respawn` 重启已停止的，`claude rm` 移除。

**Supervisor process**：独立进程托管所有 background sessions。session 状态持久化到磁盘，跨 auto-update / supervisor restart 保留。机器睡眠/关机：sessions 停止，`claude respawn --all` 重启。

**`/loop`** + Monitor：自适应轮询（Claude 自己决定间隔）/ 事件驱动（Monitor 替代轮询）。

### 31.2 关键引用
- 🟢 [Background bash](https://code.claude.com/docs/en/interactive-mode.md#background-bash-commands)
- 🟢 [Agent view](https://code.claude.com/docs/en/agent-view.md)
- 🟢 [Monitor tool](https://code.claude.com/docs/en/tools-reference.md#monitor-tool)
- 🟢 [Supervisor process](https://code.claude.com/docs/en/agent-view.md#how-background-sessions-are-hosted)

### 31.3 亮点
- **三层 background**：Bash background / Monitor event-stream / 整个 session 后台。
- **Supervisor 独立进程**：跨 auto-update / shell close / session detach 都保留 background sessions。
- **`claude agents` 表格 UI** —— 6 状态 × 颜色 × 形状，PR status dot 链接。
- **Monitor tool 替代 polling** —— token 效率高。

### 31.4 缺点
- supervisor 模型复杂（涉及 IPC / PID 管理 / 日志路径）。
- 每个 background session 独立计费 quota —— 容易超限。

### 31.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.4 有 BackgroundQueue 但**没有 supervisor 进程**。建议：
1. **Supervisor 模式**：独立进程托管 background sessions / tasks。
2. **`claude agents` 等价表格 UI**：deepseek-code 现状档案 §2.12 subagent_cards 是基础，要扩展到 session-level。
3. **Monitor tool**：见 §20。
4. **PR status dot** in agent view rows —— 链接到 PR。

---

## 32. Headless 模式

### 32.1 现状描述
**`claude -p "query"`** 入口：
- 默认输出 text。`--output-format json` 结构化（含 result / session_id / metadata / total_cost_usd / per-model breakdown）。`--output-format stream-json` newline-delimited JSON events。
- **`--bare`**：skip auto-discovery（CLAUDE.md / hooks / skills / plugins / MCP / auto memory）—— 快 + 可复现。CI 推荐。
- **`--input-format text|stream-json`**：piped input 模式。
- **`--include-partial-messages`**：token-level stream。
- **`--include-hook-events`**：hook lifecycle 事件出流。
- **`--json-schema '{...}'`**：JSON Schema validated output。
- **`--max-turns N`** + **`--max-budget-usd N`** + **`--init-only`**（只跑 SessionStart hook 退出）/ **`--init`**（跑 init matcher hook）/ **`--maintenance`**（maintenance matcher）。
- **`--no-session-persistence`**：不持久化（CI 风格 disposable）。
- **`--replay-user-messages`**：stream-json 模式下回显输入。
- **`--exclude-dynamic-system-prompt-sections`**：把 per-machine 部分移到 first user message，提高 prompt-cache reuse。

**Agent SDK**：Python + TypeScript 两个 package（`claude-agent-sdk`）。完整功能：`query()` / `ClaudeSDKClient` / `ClaudeAgentOptions` / 流式接收 / 自定义 in-process MCP / canUseTool callback / hooks / sub-agents inline。

**CI/CD 示例**（npm package.json）：
```json
{ "scripts": { "lint:claude": "git diff main | claude --bare -p 'find typos' --allowedTools Read" } }
```

**GitHub Actions** / **GitLab CI/CD** / 自定义 webhook：完整文档支持。

### 32.2 关键引用
- 🟢 [Headless](https://code.claude.com/docs/en/headless.md)
- 🟢 [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview.md)
- 🟢 [GitHub Actions](https://code.claude.com/docs/en/github-actions.md)

### 32.3 亮点
- **`--bare` 模式** —— CI 可复现 + 启动快。
- **`--init-only` / `--init` / `--maintenance`** —— session lifecycle 不同阶段 cherry-pick。
- **`--json-schema` validated output**。
- **Python + TypeScript SDK 双语**。
- **stream-json events 全暴露**（plugin_install / api_retry / partial messages / hook events）。

### 32.4 缺点
- 命令行 flag 多到记不住。
- Python/TS SDK API 升级时 migration cost（v1 → v2 迁移 guide 存在）。

### 32.5 对 deepseek-code 的启示
deepseek-code 现状档案 §1.3 已经支持 `chat`/`ask`/`run` 等子命令 + json output 部分。建议补：
1. **`--bare` 模式** —— 直接抄。
2. **`--output-format json|stream-json|text`** —— stream-json 是关键，event-driven 输出。
3. **`--json-schema`**：deepseek-code Pro 有 json mode，可以利用。
4. **`--init-only` / `--init` / `--maintenance`** —— 配合 deepseek-code 的 hook lifecycle。
5. **Python / TypeScript SDK**：长期工程，先做 CLI 流式 stable。

---

## 33. 日志与遥测

### 33.1 现状描述
**Debug logging**：`--debug` 或 `--debug "api,hooks"` 类别 filter（"!statsig" 否定）。`--debug-file <path>` / `CLAUDE_CODE_DEBUG_LOGS_DIR` env。`/debug [description]` skill mid-session 启用。

**OpenTelemetry**：`OTEL_METRICS_EXPORTER=otlp` 等标准 env vars。**v2.1.130+ subprocesses 不再继承 `OTEL_*` env vars** —— OTEL-instrumented apps run via Bash tool 不再误用 CLI 的 OTLP endpoint。

**Telemetry env vars**：
- `CLAUDE_CODE_ENABLE_TELEMETRY=1`
- `OTEL_*` 标准

**`/feedback` / `/bug`**：提交 feedback（含 session 上下文）。

**`/heapdump`**：JS heap snapshot + memory breakdown 写到 `~/Desktop`（debug 高内存）。

**Logs 位置**：`--debug-file` 指定或默认 `CLAUDE_CODE_DEBUG_LOGS_DIR`。Background sessions 的 logs 通过 `claude logs <id>` 拿。

**`disableTelemetry` / privacy**：`/privacy-settings`（Pro/Max only）+ Zero Data Retention（特定 plan）。

**Server-managed analytics**：管理员通过 [analytics 仪表板](https://code.claude.com/docs/en/analytics.md) 看团队使用。

### 33.2 关键引用
- 🟢 [Env vars](https://code.claude.com/docs/en/env-vars.md)（168 KB —— 极其密集）
- 🟢 [/feedback](https://code.claude.com/docs/en/commands.md)
- 🟢 [/heapdump](https://code.claude.com/docs/en/commands.md)
- 🟢 [Analytics](https://code.claude.com/docs/en/analytics.md)

### 33.3 亮点
- **OTLP 标准遥测** —— enterprise observability stack 直接对接。
- **category-filtered debug**（`api,hooks,!statsig`）—— 不一股脑全开。
- **`/heapdump`** —— 内存调试一键。
- **`/feedback` 带 session 上下文** —— Anthropic 收到的 bug 报告自带 transcript。
- **Subprocess OTEL 隔离**（v2.1.130+） —— 别用户的 OTEL 端点不被误用。
- **Analytics dashboard for admins** —— 团队级 adoption + velocity metrics。

### 33.4 缺点
- env vars 极多（env-vars.md 168 KB —— 几百个），无人 navigable。
- privacy settings 仅 Pro/Max 可见。

### 33.5 对 deepseek-code 的启示
deepseek-code 现状档案 §3.9 已有 TelemetryConfig，但不深。建议：
1. **OTLP 标准遥测** —— 用 `opentelemetry-rust` crate。
2. **Category-filtered debug** —— 直接抄 `--debug "api,mcp,!filesystem"`。
3. **`/heapdump` 等价**：Rust 用 `jemalloc` 输出。
4. **`/feedback` 带 session 上下文**：deepseek-code 的 events.jsonl 已经有，加一个上传/导出命令。
5. **Subprocess OTEL 隔离** —— 直接抄逻辑：spawn 时 unset `OTEL_*`。

---

## 34. Claude Code 4.5+ 的最新特性（2026-03 ~ 2026-05）

按时间倒序整合 Week 13–19 关键特性：

**Week 19 (May 4–8, v2.1.128–.136)**：
- 🆕 **`--plugin-url`** —— URL 加载 plugin zip
- 🆕 **`Ctrl+R` 默认全项目 history**（恢复 2.1.124 前行为）
- 🆕 **`worktree.baseRef: fresh | head`** setting
- 🆕 **`autoMode.hard_deny`** —— 无条件 block
- 🆕 **`$CLAUDE_EFFORT` env in hooks/bash**
- 🆕 **`CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`** —— opt out of alt-screen
- 🆕 **Homebrew/WinGet auto-update**
- 🆕 **`/mcp` shows tool count + 0-tool warning**

**Week 18 (Apr 27 – May 1, v2.1.120–.126)**：
- 🆕 **Browser-less OAuth login**（paste code）
- 🆕 **`claude project purge`**
- 🆕 **`/resume` 接受 PR URL** + `--from-pr`
- 🆕 **Windows without Git Bash**（PowerShell as primary shell）
- 🆕 **MCP `alwaysLoad: true`** opt-out of tool-search deferral
- 🆕 **`claude plugin prune`**
- 🆕 **`/skills` type-to-filter**
- 🆕 **`PostToolUse` 可改任何 tool 的 output**

**Week 17 (Apr 20–24, v2.1.114–.119)**：
- 🆕 **`/ultrareview`** public preview —— 云端 multi-agent code review
- 🆕 **Session recap**（离开 3min 回来时自动生成）
- 🆕 **Custom themes**（`~/.claude/themes/<name>.json`，plugin 也可发布）
- 🆕 **Claude Code on the web redesign**
- 🆕 **Vim visual mode**
- 🆕 **Hooks call MCP tools** via `type: "mcp_tool"`
- 🆕 **`/cost` / `/stats` merged into `/usage`**
- 🆕 **`/config` 改动持久化到 settings**
- 🆕 **Forked subagents**（`CLAUDE_CODE_FORK_SUBAGENT=1`）
- 🆕 **Default effort `high` for Pro/Max on 4.6**
- 🆕 **Native binaries replace Glob/Grep with bfs/ugrep via Bash**
- 🆕 **`--from-pr` 支持 GitLab MR / Bitbucket PR / GHE**
- 🆕 **`/resume` up to 67% faster + summarize stale**

**Week 16 (Apr 13–17, v2.1.105–.113)**：
- 🆕 **Claude Opus 4.7 + `xhigh` effort level**
- 🆕 **Routines**（cloud cron + GitHub events + API trigger）
- 🆕 **`/usage` breakdown**
- 🆕 **Mobile push notifications** via Remote Control
- 🆕 **Native binaries**（npm install pulls platform-specific binary）
- 🆕 **Auto mode for Max on Opus 4.7**
- 🆕 **`/tui` command** classic/fullscreen 切换
- 🆕 **Plugin `monitors`**
- 🆕 **Auto theme**（match terminal）
- 🆕 **`/fewer-permission-prompts`** skill

**Week 15 (Apr 6–10, v2.1.92–.101)**：
- 🆕 **Ultraplan** preview
- 🆕 **Monitor tool**（event-driven background）
- 🆕 **`/autofix-pr`** CLI 命令
- 🆕 **`/team-onboarding`**
- 🆕 **Focus view**（`Ctrl+O` 后简化）
- 🆕 **Bedrock/Vertex setup wizard**
- 🆕 **`/agents` tabbed layout**

**Week 14 (Mar 30 – Apr 3, v2.1.86–.91)**：
- 🆕 **Computer use in CLI**（toggleable via `/mcp` → computer-use server）
- 🆕 **`/powerup`** —— 内置教程
- 🆕 **Flicker-free rendering**（`CLAUDE_CODE_NO_FLICKER=1` → /tui fullscreen）
- 🆕 **MCP `_meta.anthropic/maxResultSizeChars`** per-tool override
- 🆕 **Plugin `bin/` on PATH**
- 🆕 **`PermissionDenied` hook**（auto mode 配套）
- 🆕 **`defer` permissionDecision** for `-p` mode
- 🆕 **Thinking summaries off by default**

**Week 13 (Mar 23–27, v2.1.83–.85)**：
- 🆕 **Auto mode** preview（classifier-based permission）
- 🆕 **Computer use in Desktop**
- 🆕 **PR auto-fix** in web
- 🆕 **Transcript search**（`/` in transcript mode）
- 🆕 **PowerShell tool** preview
- 🆕 **Conditional hooks**（`if` field）

### 34.2 对 deepseek-code 的整合启示
最近 7 周特性中**对 deepseek-code 价值排序**（按 ROI）：
1. **Auto mode classifier** —— 用 DeepSeek Flash 做 risk classifier。
2. **Custom themes**（`~/.deepseek/themes/<name>.toml`）+ Plugin themes。
3. **Session recap / `/recap`** —— 3min idle 回来自动 summary。
4. **`/usage breakdown`** —— 什么在烧 token。
5. **Monitor tool**（event-driven background）—— `/loop` 自适应。
6. **Forked subagents**（fork 当前 context）。
7. **Plugin themes + monitors + bin/**。
8. **`hooks: mcp_tool`** type。
9. **`--plugin-url`** —— URL 加载 plugin。
10. **`worktree.baseRef`** —— `fresh | head` 切换。

---

## TL;DR：Claude Code 最值得 deepseek-code 偷的 12 个设计

按"对 deepseek-code 21 项痛点 ROI"排序：

1. **`/` 触发斜杠命令 popover + fuzzy filter + argument hint**（解 §4 痛点 4、8）—— Claude Code 设计已成熟，slash command panel 在 `plan_tracker.rs` 已经画了，只差 input.rs 触发。
2. **`@` mention popover + fuzzy + 行号范围 `#5-10`**（解痛点 5）—— `mention_prefix_at_cursor` 后端已实现。
3. **`Ctrl+R` reverse history search 三档 scope cycling**（all/project/session via `Ctrl+S`）（解痛点 6）—— 直接抄。
4. **`Esc + Esc` 打开 rewind/summarize 菜单**（解痛点 21）—— UI 先做，持久化 checkpointing 跟上。
5. **Custom statusline 用户脚本（pipe JSON via stdin）**（解痛点 3、§13.5）—— 保留 default chip 模式，加 `[statusline] command =` 字段。
6. **`/recap` 自动 session 摘要**（DeepSeek Flash 跑摘要，开销小）。
7. **Sub-agent frontmatter schema 简化**（解 §16.5 —— 删 team.rs，统一 subagent yaml schema）。
8. **MCP HTTP + SSE + OAuth 2.1**（解痛点 18 —— 最大单点差距）。
9. **CLAUDE.md / AGENTS.md / `.deepseek/rules/<paths:>` 三件套** + **Auto memory**（解 §25.5）。
10. **Permission rule syntax**（`Bash(npm run *)` / `Read(./src/**/*.ts)` / `Agent(name)` / `mcp__server__*`）（解痛点 19、20）。
11. **OS-level sandbox**（macOS Seatbelt + Linux bubblewrap）（解痛点 19 深度问题 —— 静态 perimeter scan 不够）。
12. **`/usage breakdown`**（什么在烧 token：subagents / cache miss / parallel sessions）+ **`/context all` grid 可视化**（解 §24.5）。

## TL;DR：Claude Code 我们绝对不要抄的 5 个设计

1. **70+ slash commands 全集**：deepseek-code 现状档案 §3.10 已有 48 个，**继续追平的边际收益很低**；先把每个做深，命名收敛（`/cost`/`/stats`/`/usage` 三 alias 这种就别学了）。
2. **TodoWrite vs. TaskCreate 双系统过渡期**：直接选**一套**新版（TaskCreate/TaskGet/TaskList/TaskUpdate）做完整实现，不要留 deprecated 别名。
3. **VS Code 扩展自带 CLI 二进制**：deepseek-code 是 Rust 二进制，VS Code 扩展应该 spawn 系统 PATH 上的 `dscode`，**不要打包二进制进 VSIX**（增加 80MB+ 体积，签名也麻烦）。
4. **agent teams 实验特性（`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`）**：deepseek-code swarm.rs 已经太重，**不要再追 teammate 互发消息这种实验**，先把单个 subagent 体验做好。
5. **`Bash` 与 `PowerShell` 双工具**：deepseek-code 跨平台只用 `run_command`（内部按 OS 选 shell），**不要为 Windows 单开 tool**，把抽象做好即可。

## 调研盲区

1. **TUI 渲染层具体实现**：高度怀疑用 Ink（React for terminals）+ 自定义 alt-screen 虚拟滚动，但 cli.js 混淆 + 二进制打包，**无法直接确认 React/Ink/自研**。
2. **Auto mode classifier 的具体模型与 prompt**：文档说"server-configured model，independent of /model selection"，但**模型名 / 系统 prompt / 评估 latency** 均未公开。Anthropic blog 有 [auto-mode 工程深度文](https://www.anthropic.com/engineering/claude-code-auto-mode)（未抓取）可能有部分线索。
3. **背景 supervisor process 的 IPC 协议**：文档说有「supervisor process」托管 background sessions，**通信协议（Unix socket? gRPC? 自研协议?）未公开**。
4. **TodoWrite → TaskCreate 迁移背后的存储 schema 变化**：旧版 TodoWrite 内嵌于 transcript，新版 TaskCreate 似乎是独立资源（`~/.claude/tasks/`），但**完整 jsonl schema 未在 docs 中暴露**。
5. **Subagent 调度上限**：文档说"agent view 中每个 session 独立 quota"，但**单 session 内 subagent 并行上限是否有硬限**（Anthropic API 端口 rate limit 之外的逻辑限）未明说。

---

*报告生成于 2026-05-13，对应 Claude Code 文档 ~ v2.1.140（whats-new 2026-w19 截止）。*
