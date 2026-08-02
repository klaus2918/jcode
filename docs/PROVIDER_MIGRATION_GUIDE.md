# 内置 provider 预设迁移指南

> 适用版本：jcode v0.64.2+（含 `07a4f03b1` 移除第三方内置预设）
> 目标：默认构建不再内置第三方 OpenAI 兼容厂商；任意厂商通过配置声明接入。

---

## 1. 发生了什么

jcode 默认构建移除了 30 个第三方 OpenAI 兼容预设与 29 个对应登录 provider。
现在默认构建只保留：

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

| 旧预设 | base_url | api_key_env | 默认模型 |
|---|---|---|---|
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

## 4. 兼容红线（未受影响）

- `[providers.<name>]` 语义不变；已配置的命名 provider 继续工作
- 已登录账户、订阅、能力注册表、路由/协议全部不变
- `ModelRoute` wire 序列化不变（新增能力字段 skip 空值）
- `jcode provider add` 仍是首选接入入口
