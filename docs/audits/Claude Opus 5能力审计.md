# Claude Opus 5 能力审计

日期：2026-07-24（Opus 5 正式可用）
方法：使用 Anthropic API key 对 `https://api.anthropic.com/v1/messages` 做实时探测，`anthropic-version: 2023-06-01`。

本审计的存在是因为 jcode 的 Claude 能力表是手工维护的，而在线目录的 `max_input_tokens` 已知会过度宣传（见 `anthropic_context_mode`）。下表的每一行都是观测到的 API 响应，而不是文档声明。

## 观测到的行为

| 能力 | 探测 | 结果 | jcode 编码 |
|---|---|---|---|
| 模型 id | `GET /v1/models` | `claude-opus-5`，创建于 2026-07-24 | `ALL_CLAUDE_MODELS` |
| 最大输出 | `max_tokens: 128000` | `200` | `anthropic_max_output_tokens` -> `128_000` |
| 最大输出上限 | `max_tokens: 128001` | `400 "128001 > 128000, which is the maximum allowed number of output tokens for claude-opus-5"` | 同上 |
| 上下文窗口 | 目录 `max_input_tokens` | `1000000` | `AnthropicContextMode::Native1M` |
| 自适应思考 | `thinking: {type: adaptive}` | `200` | `model_supports_adaptive_thinking` |
| 手动思考 | `thinking: {type: enabled, budget_tokens}` | `400 "thinking.type.enabled is not supported for this model"` | 不支持手动思考 |
| 努力等级 | `output_config.effort` 取值 `low/medium/high/xhigh/max` | 全部 `200` | 完整现代等级序列 |
| 优先级层级 | `service_tier: auto` | `200`，响应 `usage.service_tier = standard` | 不符合层级资格（与此账号上的 Opus 4.8 相同） |
| 定价 | Anthropic 定价页 | `$5 / MTok` 输入，`$25 / MTok` 输出 | `anthropic_api_pricing_with_tier` |

## 为什么输出上限很重要

jcode 之前对每个 Claude 模型都发送固定的 `max_tokens = 32768`。Opus 5 允许 128K 且使用常开自适应思考，因此它的思考加上可见工具调用经常超过 32K。回合在工具调用中途被截断，智能体运行提前结束：第一个 Opus 5 基准单元在使用了 20 小时预算的 4.2% 后就干净退出了。

已在 `b9b1470ad` 中通过按模型推导预算修复。Opus 4.6-4.8、Sonnet 5/4.6 和 Fable 5 共享 128K 上限；Haiku 4.5 是 64K；未知和较旧的 id 保留保守的 32K。

## 复现

```bash
set -a; source ~/.config/jcode/anthropic.env; set +a

# 输出上限。
for mt in 128000 128001; do
  curl -s -o /dev/null -w "max_tokens=$mt http=%{http_code}\n" \
    https://api.anthropic.com/v1/messages \
    -H "x-api-key: $ANTHROPIC_API_KEY" -H "anthropic-version: 2023-06-01" \
    -H "content-type: application/json" \
    -d "{\"model\":\"claude-opus-5\",\"max_tokens\":$mt,\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}"
done

# 思考模式。
curl -s https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"model":"claude-opus-5","max_tokens":4096,"thinking":{"type":"enabled","budget_tokens":2048},"messages":[{"role":"user","content":"2+2?"}]}' \
  | jq -r '.error.message'
```
