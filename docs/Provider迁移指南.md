# 内置 provider 预设迁移指南

> 适用版本：jcode v0.64.2+（含 `07a4f03b1` 移除第三方内置预设）
> 目标：默认构建不再内置第三方 OpenAI 兼容厂商；任意厂商通过配置声明接入。

---

## 1. 发生了什么

jcode 默认构建移除了 30 个第三方 OpenAI 兼容预设与 29 个对应登录 provider。 现在默认构建只保留：

- **协议/抽象入口**：`openai-compatible`（自定义端点）、`ollama`、`lmstudio`（本地端点）
- **原生登录骨架**：auto-import / claude / anthropic-api / openai / openai-api / openrouter / bedrock / azure / jcode / cursor / copilot / gemini / gemini-api / antigravity / google
- **订阅**：jcode subscription（独立机制，不受影响）
- **能力注册表**：modelcap.json + 内嵌注册表（不受影响）

移除的是**内置预设数据**，不是接入能力。任意厂商都能用配置声明接入。

---

## 2. 迁移：把旧预设接回 jcode

旧 `--provider <第三方>` 参数不再识别。用 `jcode provider add` 或 `config.toml` 重建。

### 2.1 用 `jcode provider add` 重建（推荐）

```bash
# 通用形式
jcode provider add <name> \
  --base-url <API_BASE> \
  --model <DEFAULT_MODEL> \
  --api-key-env <API_KEY_ENV> \
  --set-default
```

### 2.2 预设 → 重建命令对照表

| 旧预设 | base\_url | api\_key\_env | 默认模型 |
| --- | --- | --- | --- |
| opencode | `https://opencode.ai/zen/v1` | `OPENCODE_API_KEY` | `minimax-m2.7` |
| opencode-go | `https://opencode.ai/zen/go/v1` | `OPENCODE_GO_API_KEY` | `kimi-k2.5` |
| zai | `https://api.z.ai/api/coding/paas/v4` | `ZHIPU_API_KEY` | `glm-4.5` |
| kimi | `https://api.kimi.com/coding/v1` | `KIMI_API_KEY` | `kimi-for-coding` |
| 302ai | `https://api.302.ai/v1` | `302AI_API_KEY` | `qwen3-235b-a22b-instruct-2507` |
| baseten | `https://inference.baseten.co/v1` | `BASETEN_API_KEY` | `zai-org/GLM-4.7` |
| cortecs | `https://api.cortecs.ai/v1` | `CORTECS_API_KEY` | `kimi-k2.5` |
| deepseek | `https://api.deepseek.com` | `DEEPSEEK_API_KEY` | `deepseek-v4-flash` |
| comtegra | `https://llm.comtegra.cloud/v1` | `COMTEGRA_API_KEY` | `glm-51-nvfp4` |
| fpt | `https://mkp-api.fptcloud.com` | `FPT_API_KEY` | `GLM-5.1` |
| firmware | `https://app.frogbot.ai/api/v1` | `FIRMWARE_API_KEY` | `kimi-k2.5` |
| huggingface | `https://router.huggingface.co/v1` | `HF_TOKEN` | `zai-org/GLM-4.7` |
| moonshotai | `https://api.moonshot.ai/v1` | `MOONSHOT_API_KEY` | `kimi-k2.5` |
| nebius | `https://api.tokenfactory.nebius.com/v1` | `NEBIUS_API_KEY` | `openai/gpt-oss-120b` |
| scaleway | `https://api.scaleway.ai/v1` | `SCALEWAY_API_KEY` | `qwen3-coder-30b-a3b-instruct` |
| stackit | `https://api.openai-compat.model-serving.eu01.onstackit.cloud/v1` | `STACKIT_API_KEY` | `openai/gpt-oss-120b` |
| groq | `https://api.groq.com/openai/v1` | `GROQ_API_KEY` | `llama-3.1-8b-instant` |
| mistral | `https://api.mistral.ai/v1` | `MISTRAL_API_KEY` | `devstral-medium-2507` |
| perplexity | `https://api.perplexity.ai` | `PERPLEXITY_API_KEY` | `sonar` |
| togetherai | `https://api.together.xyz/v1` | `TOGETHER_API_KEY` | `moonshotai/Kimi-K2-Instruct` |
| deepinfra | `https://api.deepinfra.com/v1/openai` | `DEEPINFRA_API_KEY` | `moonshotai/Kimi-K2-Instruct` |
| fireworks | `https://api.fireworks.ai/inference/v1` | `FIREWORKS_API_KEY` | `accounts/fireworks/routers/kimi-k2p5-turbo` |
| minimax | `https://api.minimax.io/v1` | `OPENAI_API_KEY` | `MiniMax-M2.7` |
| xai | `https://api.x.ai/v1` | `XAI_API_KEY` | `grok-code-fast-1` |
| nvidia-nim | `https://integrate.api.nvidia.com/v1` | `NVIDIA_API_KEY` | `nvidia/llama-3.1-nemotron-ultra-253b-v1` |
| xiaomi-mimo | `https://api.xiaomimimo.com/v1` | `XIAOMI_MIMO_API_KEY` | `mimo-v2.5` |
| celeris | `https://inference.celeris.ai/celeris-1/v1` | `CELERIS_API_KEY` | `celeris-1` |
| chutes | `https://llm.chutes.ai/v1` | `CHUTES_API_KEY` | （动态） |
| cerebras | `https://api.cerebras.ai/v1` | `CEREBRAS_API_KEY` | `gpt-oss-120b` |
| alibaba-coding-plan | `https://coding-intl.dashscope.aliyuncs.com/v1` | `BAILIAN_CODING_PLAN_API_KEY` | `qwen3-coder-plus` |

示例：

```bash
# DeepSeek
jcode provider add deepseek \
  --base-url https://api.deepseek.com \
  --model deepseek-v4-flash \
  --api-key-env DEEPSEEK_API_KEY \
  --set-default

# Anthropic 兼容网关
jcode provider add my-anth-gw \
  --base-url https://gateway.example.com/v1 \
  --model claude-sonnet-4-6 \
  --api anthropic \
  --api-key-env MY_ANTH_GW_KEY \
  --set-default
```

### 2.3 直接写 config.toml

```toml
[providers.deepseek]
type = "openai-compatible"
base_url = "https://api.deepseek.com"
api = "openai-compatible"
api_key_env = "DEEPSEEK_API_KEY"
default_model = "deepseek-v4-flash"

[[providers.deepseek.models]]
id = "deepseek-v4-flash"
context_window = 1000000
```

也支持 **resonix 风格**（`[[providers]]` 数组表 + `kind`/`model`）：

```toml
[[providers]]
name = "deepseek"
kind = "openai"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
context_window = 1000000

[[providers]]
name = "my-anth-gw"
kind = "anthropic"
base_url = "https://gateway.example.com/v1"
model = "claude-sonnet-4-6"
models = ["claude-sonnet-4-6", "claude-haiku-4-5"]
```

### 2.4 已有 env 文件

旧 `*.env`（如 `deepseek.env`）仍可用于 `env_file` 字段：

```toml
[providers.deepseek]
type = "openai-compatible"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
env_file = "deepseek.env"
```

---

## 3. 接入任意新厂商（通用流程）

1. `jcode provider add <name> --base-url <url> --model <model> --api-key-env <KEY>`
2. 或直接编辑 `~/.jcode/config.toml`（`[providers.<name>]` 或 `[[providers]]`）
3. `jcode provider list` 确认
4. `jcode --provider-profile <name> run "hello"` 验证

密钥永远走环境变量（`api_key_env`）或 `.env` 文件，不写进 config.toml。

---

## 4. 接入方式：本地网关（cc-switch 本地代理）

> 场景：已有 [CC Switch](https://github.com/farion1231/cc-switch) 统一管理多路 API provider，通过其**本地代理**统一出口接入 jcode。provider 切换、故障转移、格式转换、用量统计都由 cc-switch 承担，jcode 只面向一个本地端点。

### 4.1 工作方式

- cc-switch 本地代理监听 `127.0.0.1:<port>`（默认 `15721`，可在面板修改），按应用类型（Claude / Codex / Gemini）把请求路由到当前启用的 provider，并在转发时注入该 provider 保存的真实 key。
- jcode 以 **Anthropic Messages 格式**接入，等价于一个"Claude 通道"客户端，请求经代理透传或格式转换（上游为 OpenAI 兼容时自动转换）。
- cc-switch 支持的工具列表（Claude Code / Codex / Gemini CLI / OpenCode 等）**不含 jcode**，它不会自动改写 jcode 配置，jcode 侧需手动写入下面的配置。

### 4.2 jcode 配置示例

```toml
[provider]
default_provider = "cc-switch"
# default_model 可留空：留空时自动跟随 cc-switch 面板当前启用的 provider 模型（见 4.3）

# 坑：cc-switch 必须用 [[providers]] 数组条目（name 必填），不能用
# [providers.cc-switch] 表格——同一文件里混用会让整个 config.toml 解析失败。
[[providers]]
name = "cc-switch"
kind = "anthropic"                    # Anthropic wire 格式，代理按 Claude 通道路由
base_url = "http://127.0.0.1:15721"   # cc-switch 代理地址：以面板显示为准（默认端口 15721）
auth = "none"                         # 真实 key 由 cc-switch 注入，jcode 侧不需要 key
# default 与 models 可留空（自动发现并跟随面板切换），也可以填模型名固定使用
# default = "deepseek-v4-flash"
# models = ["deepseek-v4-flash"]
```

### 4.3 模型和 key：还需要指定吗

| 项 | 结论 | 说明 |
| --- | --- | --- |
| 模型 | **可留空，自动跟随；也可显式指定** | `default_model`（和 `models` 列表）留空时，jcode 启动/首次请求前从代理的 `/v1/models` 拉取模型目录，自动选择 cc-switch 当前启用 provider 的第一个模型；cc-switch 面板切换 provider 后模型名变化，jcode 会刷新目录跟随（请求遇模型不可用也会自动回退）。要固定某个模型时填 `default_model` 或用 `--model` 指定，显式选择不会被自动发现覆盖。模型列表可用 `/model` 切换。 |
| key | **不需要** | 真实 key 保存在 cc-switch 的 provider 配置中，由代理在转发时注入。jcode 侧 `auth = "none"` 即可，无需 `api_key_env`；若填了占位值也不参与请求。 |

### 4.4 注意事项

- `base_url` 以 cc-switch 代理面板显示的"服务地址"为准（端口可改）。
- 代理必须保持运行；停止后请求会失败（cc-switch 会还原它接管的工具配置，但 jcode 配置是手动写的，不会被还原）。
- provider 切换在 cc-switch 面板操作（切换 Claude 通道的启用 provider），jcode 侧不用改配置；模型留空时 jcode 自动跟随新 provider 的模型，若显式指定了模型名则需确保新 provider 支持该模型。
- 故障转移、请求日志、用量统计在 cc-switch 面板查看，jcode 无感知。

---

## 5. 兼容红线（未受影响）

- `[providers.<name>]` 语义不变；已配置的命名 provider 继续工作
- 已登录账户、订阅、能力注册表、路由/协议全部不变
- `ModelRoute` wire 序列化不变（新增能力字段 skip 空值）
- `jcode provider add` 仍是首选接入入口