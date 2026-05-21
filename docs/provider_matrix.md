# Provider Matrix

Last verified: 2026-05-21

This matrix is an internal implementation reference for Octocode provider
profiles. Use official provider documentation as the source of truth when
refreshing model names, prices, and wire-format details.

| Provider | Priority | Strong model | Fast model | Base URL | Key env | Chat | Stream | Tools | Thinking | Context | Max output | Cache | FIM | Vision | Notes |
|---|---:|---|---|---|---|---|---|---|---|---:|---:|---|---|---|---|
| DeepSeek | P0 | `deepseek-v4-pro` | `deepseek-v4-flash` | `https://api.deepseek.com` | `DEEPSEEK_API_KEY` | OpenAI-compatible | yes | yes | `thinking.type` + effort | 1M | 384K | hit/miss usage | yes, non-thinking | no | Keep DeepSeek-native adapter for cache, FIM, and reasoning. |
| Qwen / DashScope | P0 | `qwen3-coder-plus` | `qwen3-coder-flash` | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `DASHSCOPE_API_KEY` | OpenAI-compatible | yes | yes | `enable_thinking` + `thinking_budget` | model-specific | model-specific | implicit/explicit cache on supported models | no | model-specific | Good coding default; do not map to generic OpenAI `reasoning_effort`. |
| Kimi / Moonshot | P0 | `kimi-k2.6` | `kimi-k2.5` | `https://api.moonshot.ai/v1` | `MOONSHOT_API_KEY` | OpenAI-compatible | yes | yes | Kimi `thinking` extension | 256K | model-specific | cached token usage | no | yes | Preserve `reasoning_content`; old K2 models are being retired. |
| Zhipu GLM / Z.ai | P0 | `glm-5.1` | `glm-4.7-flashx` | `https://open.bigmodel.cn/api/paas/v4` | `ZAI_API_KEY` | OpenAI-compatible | yes | yes | `thinking.type` | 200K | 128K | model-specific | no | model-specific | Preserve `reasoning_content` during tool loops. |
| MiniMax | P0 | `MiniMax-M2.7` | `MiniMax-M2.7-highspeed` | `https://api.minimaxi.com/v1` | `MINIMAX_API_KEY` | OpenAI-compatible | yes | yes | `reasoning_split` / `reasoning_details` | model-specific | model-specific | automatic cache | no | model-specific | China endpoint is default; keep reasoning details. |
| Tencent TokenHub | P1 | `hunyuan-2.0-thinking` | `hunyuan-2.0-instruct` | `https://tokenhub.tencentmaas.com/v1` | `TENCENT_TOKENHUB_API_KEY` | OpenAI-compatible | yes | yes | model-specific | model-specific | model-specific | model-specific | no | model-specific | Prefer TokenHub over legacy Hunyuan endpoint. |
| Baidu Qianfan | P1 | `ernie-5.0-thinking-preview` | `ernie-4.5-turbo-128k` | `https://qianfan.baidubce.com/v2` | `QIANFAN_API_KEY` | OpenAI-compatible path under `/v2` | yes | yes | nested + top-level thinking fields | 128K | model-specific | provider context support | no | model-specific | Dedicated profile because auth/key shape and `/v2` differ. |
| StepFun | P1 | `step-2-16k` | `step-2-mini` | `https://api.stepfun.ai/v1` | `STEPFUN_KEY` | OpenAI-compatible | yes | model-specific | normal v1 limited | 16K | model-specific | none declared | no | no | Step-Plan has separate OpenAI/Anthropic endpoints. |
| Doubao / Volcano Ark | P2 | override required | override required | `https://ark.cn-beijing.volces.com/api/v3` | `ARK_API_KEY` | OpenAI-compatible via Ark endpoint | yes | yes | endpoint-specific | 256K | 32K | endpoint-specific | no | yes | Do not hard-code one deployed endpoint/model for users. |

Source checklist:

- DeepSeek API docs: https://api-docs.deepseek.com/
- Alibaba Model Studio Qwen docs: https://help.aliyun.com/zh/model-studio/
- Kimi API docs: https://platform.kimi.ai/docs/
- BigModel/Z.ai docs: https://docs.bigmodel.cn/
- MiniMax API docs: https://platform.minimax.io/docs/
- Tencent TokenHub docs: https://cloud.tencent.com/document/product/1823
- Baidu Qianfan docs: https://cloud.baidu.com/doc/qianfan-api/
- StepFun docs: https://platform.stepfun.com/docs/
- Volcano Ark docs: https://www.volcengine.com/docs/82379/
