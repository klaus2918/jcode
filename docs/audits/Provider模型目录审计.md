# OpenAI 兼容 provider `/models` 审计

范围：`crates/jcode-provider-metadata/src/lib.rs` 中的内置 `OpenAiCompatibleProfile` 条目。

图例：
- `verified data[]`：文档展示或明确引用 OpenAI 兼容的 `GET /models` 形状 `{ object: "list", data: [...] }`，或该 provider 声称实现了 OpenAI Models API。
- `verified top-level array`：文档展示顶层模型数组。
- `supported endpoint, shape not shown`：文档说 `/models` 存在或 OpenAI 兼容性包含它，但没有展示响应体。
- `catalog/static only`：文档指向静态目录/模型页，而不是实时 `/models` 端点。
- `unknown`：找不到能证明 `/models` 存在的 provider 文档。

## 被审计的 provider

| Provider | 证据 | 期望的解析器支持 | 备注 |
|---|---|---:|---|
| OpenCode Zen | OpenCode 文档和 models.dev 目录 | 静态/引导，端点未验证 | OpenCode 本身使用 models.dev。Zen 上的 `/models` 未经独立验证。 |
| OpenCode Go | OpenCode 文档和 models.dev 目录 | 静态/引导，端点未验证 | 与 Zen 相同。 |
| Z.AI | 搜索未找到 `/models`；当前元数据路径下的 provider 文档 URL 返回 404 | unknown | 需要直接的最新文档 URL 或带 key 的实测。 |
| Kimi Code | 找到 Kimi 第三方智能体文档，仅有 OpenAI Compatible 配置 | unknown | 未找到 `/models` 响应形状。 |
| 302.AI | 官方文档包含 `Models（列出模型）GET` 页面 | 可能是 OpenAI 兼容 data[] | 存在专门的列出模型页面。抓取文本中的响应体在示例前被截断。 |
| Baseten | 官方文档说公共 OpenAI 兼容端点 `https://inference.baseten.co/v1` | supported endpoint, shape not shown | 未找到专门的 `/models` 响应。 |
| Cortecs | 仅官方文档概述加 OpenCode provider 条目 | catalog/static only | 未找到 `/models` 端点文档。 |
| DeepSeek | 官方 `GET /models` 文档展示 `{ object, data[] }` | verified data[] | 解析器已覆盖。 |
| Comtegra | 官方文档列出受支持的 `/v1/models` 并链接 OpenAI Models API | supported endpoint, shape OpenAI | 解析器已覆盖。 |
| FPT AI Marketplace | 官方文档展示通过 LiteLLM/OpenAI 的 chat/completions，无 models 端点 | unknown/no evidence | 实时 `/models` 可能失败。 |
| Firmware/FrogBot | 仅 OpenCode provider 文档 | catalog/static only | 未找到直接的 provider API 文档。 |
| Hugging Face | 通用 Inference Providers 文档，OpenAI 兼容 API | supported endpoint, shape not shown | 未验证到专门的 `/models` 页面。 |
| Moonshot AI | 搜索/当前 URL 未暴露 `/models` 文档 | unknown | Kimi API 搜索提示存在模型列表端点，但未抓取到官方 Moonshot 页面。 |
| Nebius | Quickstart 文档中的 OpenAI 兼容端点 | supported endpoint, shape not shown | 未验证专门的 `/models` 页面。 |
| Scaleway | 找到官方 "Using Models API" 文档 | supported endpoint, shape likely OpenAI | 若是 OpenAI 形状则解析器已覆盖。 |
| STACKIT | 官方集成文档说 OpenAI 兼容 API 且模型选择器抓取 `/models` | supported endpoint, shape not shown | 若是 OpenAI 形状则已覆盖。 |
| Groq | API 参考有 Models/List models | verified data[] | 已覆盖。 |
| Mistral | API 参考有 Models/List Available Models | verified data[] 风格 | 已覆盖。 |
| Perplexity | API 文档抓取/搜索未找到 list-models 端点 | unknown | 可能不支持 `/models`；静态文档列出模型。 |
| Together AI | 官方 `GET /models` 文档展示顶层数组 | verified top-level array | 解析器为此修复过。 |
| DeepInfra | 官方 OpenAI 兼容文档指向静态模型目录，未找到 `/models` 页面 | catalog/static only | 实时 `/models` 未验证。 |
| Fireworks | 官方 list-models 文档覆盖账号模型 API `{ models: [...] }`；OpenAI 兼容端点也存在 | 账号 API 的 verified models[] 变体 | 解析器支持 `models[]` 和 `name`。实时基础端点形状仍未验证。 |
| MiniMax | 官方文本生成文档展示 OpenAI 兼容 base 和静态受支持模型表 | catalog/static only | 未找到 `/models` 端点。 |
| xAI | API 参考包含 Models 章节 | verified data[] 很可能 | 已覆盖。 |
| LM Studio | 官方 OpenAI 兼容性文档列出 `GET /v1/models` | supported endpoint, shape not shown | OpenAI 本地服务器预期 data[]。 |
| Ollama | 官方 OpenAI 兼容性博客/文档覆盖聊天；抓取页面中未找到 `/v1/models` 文档 | unknown | 需要原始文档/源码或本地实时测试。 |
| Chutes | 实时用户响应展示 `{ object:"list", data:[...] }` 并带数值定价 | verified data[] 加数值定价 | 解析器已修复，过时的默认值已移除。 |
| Cerebras | 官方 `GET /v1/models` 文档展示 `{ object, data[] }` | verified data[] | 已覆盖。 |
| Alibaba Coding Plan | 官方文档展示 OpenAI 兼容 base URL，但警告 Coding Plan 仅用于编码工具；无 `/models` 文档 | unknown/no evidence | 很可能需要静态默认；实时 `/models` 可能失败。 |
| 通用 openai-compatible | 用户提供的端点 | 解析器契约 | 支持 `{data[]}`、顶层数组、`{models[]}`、id/name 标识符。 |

## `f291f0e` 之后的解析器覆盖

受支持的响应形式：
- `{ "data": [{ "id": "..." }] }`
- 顶层 `[{ "id": "..." }]`
- `{ "models": [{ "id" or "name": "..." }] }`
- 数值或字符串定价字段
- 上下文字段：`context_length`、`contextLength`、`max_context_length`、`maxModelLength`、`max_model_len`、`trainingContextLength`

## 发现的差距

目前没有证明需要额外的解析器形状。剩余问题是 provider 能力/配置的准确性：
- 一些 provider 对聊天是 OpenAI 兼容的，但没有记录实时 `GET /models`。
- 对这些 provider，实时目录刷新应保持尽力而为，并且必须优雅回退到静态目录。
- 长期来看，`OpenAiCompatibleProfile` 可能应该携带 `model_catalog` 能力/策略，这样已知不支持 `/models` 的 provider 不会产生嘈杂的刷新失败。
