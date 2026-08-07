# Provider 诊断

`jcode provider-doctor` 是一个面向用户的诊断命令，回答一个问题：

> 为什么我的 provider/模型（或模型选择器）不工作？

它走一遍与在线覆盖台账（`jcode provider-test-coverage`）跟踪的相同严格端到端检查点，但作为交互式命令你可以自己运行，带清晰的通过/失败输出，并在第一个失败处给出"接下来试什么"的提示。

它适用于 **OpenAI 兼容 provider**（openrouter、gemini-api、ollama、lmstudio、openai-compatible 及其他 `openai-compatible` 配置）。任何用户自定义的 `[providers.<name>]` 配置也可以用 id 诊断（`jcode provider-doctor <name> --tier offline`）。

## 快速开始

```bash
# 验证 jcode 对某个 provider 自身的接线，不需要 API key，不花钱：
jcode provider-doctor openrouter --tier offline

# 验证 key + 在线模型目录（需要 key，花费可忽略）：
jcode provider-doctor openrouter --tier catalog

# 完整就绪度，包括真实聊天、流式与工具调用（花费余额）：
jcode provider-doctor openrouter --tier full

# 固定特定模型并输出 JSON 供脚本/CI 使用：
jcode provider-doctor openrouter --model openai/gpt-5.5 --tier full --json
```

模型默认为 provider 的默认模型（或第一个在线目录模型）。用全局 `--model` 标志固定特定模型。

## 层级（Tier）

选择要测试多少。每个层级在其约束下尽可能多地验证，因此你可以廉价调试，只在需要时升级。

| 层级 | 需要 key? | 花费余额? | 新增内容 | 能发现 |
| --- | --- | --- | --- | --- |
| `offline` | 否 | 否 | 针对合成目录的 jcode 侧接线 | 该 provider 的目录重载、选择器渲染、回退标记和模型切换路由 bug |
| `catalog`（默认） | 是 | 约零 | 在线 `GET /models` | key 错误/缺失、端点失效、模型不在在线目录中 |
| `full` | 是 | 是 | 非流式聊天、流式、工具调用循环 | 模型是否真的能聊天、流式并支持工具调用 |

只有 `full` 层级能获得严格（"READY"）覆盖。较轻的层级有意把依赖 API 的检查点记录为跳过，因此覆盖台账中不会有过度记功。

## 检查点

每次运行按顺序报告这些严格检查点。只有当全部在 `full` 层级通过时，一个（provider，模型）对才算完全就绪。

1. `auth_credential_loaded` - 为该 provider 找到了凭据
2. `model_catalog_live_endpoint` - 在线 `/models` 端点返回了模型
3. `catalog_hot_reload_current_session` - 目录重载进会话
4. `picker_live_models` - 选择器显示在线模型，包括所选模型
5. `picker_fallback_labeling` - 路由由在线目录支撑，而非静态回退
6. `model_switch_route` - 切换模型产生 provider 明确的路由
7. `non_streaming_chat_completion` - 基本聊天回复返回（full 层级）
8. `streaming_chat_completion` - 流式回复返回（full 层级）
9. `tool_call_parse` - 模型发出可解析的工具调用（full 层级）
10. `tool_execution_loop` - 工具调用循环运行（full 层级）
11. `tool_result_followup` - 工具结果被回喂（full 层级）
12. `real_jcode_tool_smoke` - 端到端工具冒烟通过（full 层级）

（检查点 1-2 加上认证生命周期阶段是预检；7-12 是依赖 API 的检查点，由 `--tier full` 门控。）

## 阅读输出

```
Provider doctor: Cerebras / gpt-oss-120b
Tier: catalog (API key, ~no spend: adds live catalog fetch)
...
  [ PASS] Credential loaded                      Loaded credential from CEREBRAS_API_KEY
  [ PASS] Live model catalog endpoint            2 live model(s) returned
  [ PASS] Catalog hot reload in current session  2 catalog route(s) reloaded
  [ PASS] Picker shows live models               2 model(s) in picker, selected `gpt-oss-120b`
  [ PASS] Picker fallback labeling               all routes backed by live catalog (no static fallback)
  [ PASS] Model switch route                     switch request `cerebras:...` routed via `openai-compatible:cerebras`
  [ skip] Non-streaming chat completion          catalog tier: requires --tier full (spends balance)
  ...
Verdict: tier `catalog` passed. Run `--tier full` to confirm full readiness (spends balance).
```

- `PASS` / `FAIL` - 检查点运行并通过/失败。
- `skip` - 当前层级不运行此检查点（使用 `--tier full`）。
- 结论行告诉你层级是通过、完全通过（`READY`）还是失败，失败时指向第一个失败检查点并给出下一步。

当所选层级未完全通过时，命令以非零码退出，因此可以当作 CI/脚本门禁使用。

## 花费跟踪（一次运行花多少钱？）

花费余额的层级（`catalog` 做一次目录调用，`full` 做多次聊天/流式/工具调用）精确报告消耗，方便你预算：

```
Spend this run: 3 billable API calls, 554 tokens (289 in + 265 out), cost not reported by provider
```

- **billable API calls** - 实际命中 provider 的请求数。
- **tokens** - 这些调用中提示 + 补全的总和（当 provider 返回 `usage` 块时）。流式探针请求 `stream_options.include_usage`，因此流式调用也被计数。
- **cost** - 仅当 provider 报告 `cost` 字段时显示为美元数字；许多 provider（如 cerebras）只返回 token，因此你会看到"cost not reported by provider"，可以按你的套餐费率乘以 token 数。一次完整的 cerebras 运行大约是 550-620 token（约 $0.0003）。

`--json` 在 `spend` 对象下包含相同数据（`billable_calls`、`prompt_tokens`、`completion_tokens`、`total_tokens`、`has_token_data`、`reported_cost_usd`）。

这个花费会随运行**持久化**到覆盖台账，因此 `jcode provider-test-coverage` 显示累计的"Recorded spend"页脚，对每个（provider，模型）对汇总最近一次运行。这给你一个持久的、一目了然的答案："到目前为止测试这个覆盖花了我多少钱？"

## 典型调试流程

1. **"我的选择器坏了 / 显示错误模型。"**
   运行 `--tier offline`。如果 `picker_live_models`、`picker_fallback_labeling` 或 `model_switch_route` 失败，那是该 provider 的 jcode 侧路由 bug：捕获输出并提交 issue。

2. **"连不上 / 说认证失败。"**
   运行 `--tier catalog`。如果 `auth_credential_loaded` 或 `model_catalog_live_endpoint` 失败，问题在 key/端点。运行 `jcode login --provider <provider>`。

3. **"能连接但模型行为糟糕。"**
   运行 `--tier full`。如果 `non_streaming_chat_completion` / `streaming_chat_completion` / `tool_*` 检查点失败，问题在模型本身；从在线目录试试另一个模型。

## 与覆盖的关系

每次 doctor 运行都会把一次在线验证事件记录进覆盖台账，带层级标签（`doctor_tier`）。通过全部 12 个严格检查点的 `full` 层级通过会把该（provider，模型）对在 `jcode provider-test-coverage` 中翻转为严格（"READY"）。较轻的层级把依赖 API 的检查点记录为跳过，因此从不过度记功。

`jcode provider-test-coverage` 把同样的 12 个检查点渲染成 12 阶段流水线。每个已观测对占一行紧凑输出：一个状态记号（`READY`，或 `N/12` = 它清除了多少阶段）后跟 `provider / model`，然后对任何尚未 READY 的对，给出第一个阻塞点以及把它推过去的精确 `provider-doctor` 命令。因此两个命令是同一流水线的两个视图：覆盖报告显示每个对卡在哪里，并把推进它的 doctor 命令交给你。

每行以新鲜度说明结尾，例如：

```
  READY  cerebras / gpt-oss-120b   last tested 9 minutes ago (2026-05-30) by developer (dev build)
  6/12   nvidia-nim / gemma-4-31b  failed at `streaming reply`; run `jcode provider-doctor nvidia-nim --model gemma-4-31b --tier full`; last tested 2 days ago ...
```

- **多久之前** 最近一次运行发生在何时，用自然语言加绝对日期，这样你能一眼看出证据是否过时。
- **谁运行的**：干净的发布构建标记为 `user (release build)`（真实用户证据），脏/dev 构建为 `developer (dev build)`。这由每次运行记录的构建标志可靠地推导，不是猜测。
