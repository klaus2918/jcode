# OpenAI 兼容配置运行时迁移计划

## 问题

`OpenRouterProvider` 当前代表两个不同概念：

1. 标准 OpenRouter，带 OpenRouter 特定路由、provider 固定、端点元数据和 `openrouter` 目录命名空间。
2. 直接 OpenAI 兼容 provider，如 NVIDIA NIM、Groq、Cerebras、Chutes 和自定义端点，它们复用相同的 HTTP 传输，但有不同的凭据、API base、目录和模型 ID。

因为 `MultiProvider` 只存储一个 `openrouter` 运行时槽位，从标准 OpenRouter 切换到直接配置会替换活动运行时/目录视图。这导致 issue #274：从 `openrouter/owl-alpha` 切换到 NVIDIA NIM 后，`/model` 不再暴露标准 OpenRouter，并可能把 OpenRouter 模型误关联到 NVIDIA。

## 目标架构

分离传输、配置身份和路由聚合。

```rust
struct OpenAiCompatibleClient {
    api_base: String,
    api_key_env: String,
    env_file: String,
    auth_header: AuthHeaderConfig,
}

struct OpenAiCompatibleProfileRuntime {
    profile_id: String,          // "openrouter"、"nvidia-nim"、"groq"、……
    display_name: String,        // "OpenRouter"、"NVIDIA NIM"、……
    cache_namespace: String,     // 通常等于 profile_id
    default_model: Option<String>,
    provider_routing: bool,      // 标准 OpenRouter 功能时为 true
    client: OpenAiCompatibleClient,
}
```

`MultiProvider` 最终应从：

```rust
openrouter: RwLock<Option<Arc<openrouter::OpenRouterProvider>>>,
```

迁移到类似：

```rust
openai_compatible: RwLock<BTreeMap<String, Arc<OpenAiCompatibleProfileRuntime>>>,
active_openai_compatible_profile: RwLock<Option<String>>,
```

标准 OpenRouter 成为此 map 中的一个配置，而不是每个兼容 provider 的容器。

## 路由聚合规则

`/model` 应从每个已配置配置聚合路由：

```rust
for profile in configured_openai_compatible_profiles() {
    routes.extend(profile.model_routes());
}
```

把活动运行时切换到 NVIDIA NIM 只应更新活动选择：

```rust
active_openai_compatible_profile = Some("nvidia-nim".into());
```

它不应移除或重贴 `openai_compatible["openrouter"]` 的标签。

## 兼容性要求

保持现有面向用户的形式工作：

- `openrouter:<model>` 目标是标准 OpenRouter。
- `nvidia-nim:<model>` 目标是 NVIDIA NIM。
- `openai-compatible:<model>` 目标是已配置的自定义端点。
- `--provider openrouter` 仍是标准 OpenRouter。
- `--provider openai-compatible` 仍是通用/自定义配置。
- 现有 `OpenRouterProvider` 类型可以在内部迁移时保留为兼容包装器。

## 增量迁移切片

1. **路由聚合切片，在 `b1272ae` 中完成**
   - 标准 OpenRouter 缓存路由限定在 `openrouter` 命名空间。
   - 直接配置可以激活而不从 `/model` 隐藏标准 OpenRouter。
   - 回归：OpenRouter `owl-alpha` -> NVIDIA NIM -> `/model` 保持 OpenRouter 路由且不把它重贴为 NVIDIA。

2. **配置运行时结构**
   - 围绕当前 OpenRouter provider 设置引入 `OpenAiCompatibleProfileRuntime`。
   - 最初把 `OpenRouterProvider` 保留为类型别名/包装器。

3. **运行时注册表**
   - 给 `MultiProvider` 添加已配置兼容配置的 map。
   - 在启动和认证变化时从已配置/已保存凭据填充它。

4. **活动配置选择**
   - 用显式配置 ID 替换隐式环境突变作为唯一活动配置状态。
   - 只把环境应用作为兼容/引导层。

5. **选择器和服务器快照**
   - 发出配置限定路由和可用模型快照。
   - 在调试输出中包含配置 ID/API 方法，使误贴标签可测试。

6. **重命名清理**
   - 在准确的地方把通用内部从 OpenRouter 重命名为 OpenAI 兼容。
   - 保持公开命令和配置稳定。

## 验证矩阵

对每个已配置配置对，验证：

- 活动配置 A、非活动配置 B：`/model` 同时显示 A 和 B 路由。
- 选择 B 路由切换到 B 并保持 A 可见。
- 带斜杠 ID 的模型不会自动被视为标准 OpenRouter，除非路由/配置说明如此。
- OpenRouter provider 固定只对标准 OpenRouter 配置可用。
- 直接配置的静态和在线目录保持命名空间限定。

关键回归场景：

- `openrouter/owl-alpha` -> `nvidia-nim:nvidia/llama-...` -> OpenRouter 仍可选择。
- Cerebras 激活且 Groq 已配置 -> 不把 Cerebras 模型重贴为 Groq。
- Chutes 激活且存在陈旧旧版 OpenRouter 缓存 -> Chutes 下无陈旧 OpenRouter 模型。
