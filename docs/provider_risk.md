# Provider Risk Register

Last verified: 2026-05-21

| Provider | Risk | Product handling |
|---|---|---|
| DeepSeek | Legacy `deepseek-chat` and `deepseek-reasoner` names are compatibility aliases and may stop working. | New configs must emit `deepseek-v4-pro` / `deepseek-v4-flash`; keep migration-only alias parsing. |
| DeepSeek | FIM is only safe in non-thinking lanes. | Keep FIM routed through non-thinking lane validation. |
| Qwen / DashScope | Thinking controls are not OpenAI `reasoning_effort`; some tool-choice modes are unsupported. | Use `DashScopeEnableThinking`; do not force `tool_choice=required`. |
| Kimi / Moonshot | `.cn` and `.ai` endpoint confusion can break users; older K2 models are being retired. | Default to `https://api.moonshot.ai/v1`; keep model names current in profile. |
| Kimi / Moonshot | Thinking, built-in web search, and partial output have model-specific constraints. | Preserve reasoning fields but do not claim complete Kimi-native feature support yet. |
| Zhipu GLM / Z.ai | Reasoning continuity depends on preserving `reasoning_content` correctly. | Preserve reasoning only for active tool-loop turns; do not replay old reasoning across sessions. |
| MiniMax | Thinking can appear in `<think>` text or `reasoning_details`; dropping it weakens tool-loop continuity. | Enable `reasoning_split` and parse `reasoning_details` into internal reasoning content. |
| Tencent TokenHub | Legacy Hunyuan platform is migrating to TokenHub. | Default to TokenHub endpoint; keep legacy endpoint as user override only. |
| Baidu Qianfan | It is OpenAI-like but uses `/v2`, BCE-style Bearer keys, and extra thinking fields. | Treat as dedicated provider profile instead of plain `openai-compatible`. |
| StepFun | Step-Plan uses separate OpenAI-compatible and Anthropic-compatible endpoints. | Standard provider only targets v1 chat; Step-Plan is future adapter work. |
| Doubao / Volcano Ark | Real model access is often bound to user-created Ark endpoints. | Mark endpoint/model override required; do not pretend one default model fits all accounts. |
| OpenAI-compatible / aggregators | Capabilities depend on the upstream model, not the aggregator. | Keep as custom provider; users must configure base URL and model mapping. |

Refresh rules:

- Re-check official docs before changing default model names or context/output limits.
- Every profile change must update `last_verified` and tests for model mapping.
- New provider claims need at least payload-level tests before being shown as first-class.
- Real API smoke tests must remain opt-in to avoid unexpected cost.
