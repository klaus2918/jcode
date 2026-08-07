# AWS Bedrock 提供商

Jcode 支持原生 AWS Bedrock 提供商，使用 AWS Rust SDK 和 `ConverseStream` 直接与 Bedrock Runtime 通信。

## 配置凭据

Jcode 支持两种 Bedrock 认证风格：

- **Bedrock API 密钥 / bearer 令牌**：本地入门最简单。Jcode 将令牌存储在其配置 env 文件中，并通过 AWS SDK 以 `AWS_BEARER_TOKEN_BEDROCK` 发送。
- **AWS IAM 凭据**：适合普通 AWS 客户环境。可以是 AWS CLI/SSO 配置、环境访问密钥、web identity、EC2/ECS 元数据凭据，或其他标准 AWS SDK 凭据来源。

引导式 API 密钥流程，运行：

```bash
jcode login --provider bedrock
```

这会把 `AWS_BEARER_TOKEN_BEDROCK` 和 `JCODE_BEDROCK_REGION` 保存到 `~/.config/jcode/bedrock.env`。

也可以手动配置：

```bash
export AWS_BEARER_TOKEN_BEDROCK=your-bedrock-api-key
export AWS_REGION=us-east-1
```

AWS CLI/IAM/SSO 凭据：

```bash
export AWS_PROFILE=my-profile
export AWS_REGION=us-east-1
# 可选的 Jcode 特定覆盖：
export JCODE_BEDROCK_PROFILE=my-profile
export JCODE_BEDROCK_REGION=us-east-1
```

如果你依赖实例/容器元数据凭据且没有本地配置 env 变量，请显式启用：

```bash
export JCODE_BEDROCK_ENABLE=1
export AWS_REGION=us-east-1
```

AWS SSO 配置：

```bash
aws sso login --profile my-profile
```

对于 AWS CLI 控制台登录配置，Jcode 也可以使用以下命令导出的凭据：

```bash
aws configure export-credentials --profile my-profile --format env-no-export
```

Jcode 不存储这些导出的会话凭据；它在 Bedrock 提供商初始化时向 AWS CLI 配置提供商询问。

## IAM 权限

运行时路径至少需要：

```json
{
  "Effect": "Allow",
  "Action": [
    "bedrock:InvokeModel",
    "bedrock:InvokeModelWithResponseStream"
  ],
  "Resource": "*"
}
```

模型发现额外使用：

```json
{
  "Effect": "Allow",
  "Action": [
    "bedrock:ListFoundationModels",
    "bedrock:ListInferenceProfiles"
  ],
  "Resource": "*"
}
```

如果你用 `JCODE_BEDROCK_VALIDATE_STS=1` 启用 STS 验证，允许 `sts:GetCallerIdentity`。

## 用 Bedrock 运行 Jcode

```bash
jcode --provider bedrock --model anthropic.claude-3-5-sonnet-20241022-v2:0
```

或：

```bash
jcode --model bedrock:anthropic.claude-3-5-sonnet-20241022-v2:0
```

推理配置 ID/ARN 可作为模型 ID 使用，例如：

```bash
jcode --model bedrock:us.anthropic.claude-3-5-sonnet-20241022-v2:0
```

当你的账户有权限时，推荐的活动配置风格选择包括：

```text
us.amazon.nova-2-lite-v1:0
us.amazon.nova-lite-v1:0
us.amazon.nova-micro-v1:0
us.anthropic.claude-sonnet-4-6
us.anthropic.claude-haiku-4-5-20251001-v1:0
us.deepseek.r1-v1:0
```

当基础模型 ID 和配置 ID 同时出现时，优先使用区域/配置 ID，如 `us.amazon.nova-2-lite-v1:0`。一些 Bedrock 模型不支持按需调用，必须通过推理配置调用。

## 模型选择器 UX

`/model` 保持 Bedrock 行紧凑：

- `×` 表示该路由不可选择。选择该行可看到完整原因，例如旧版模型访问或缺少凭据。
- `⚠` 表示该路由可选择但受限，最常见的是不支持工具使用。
- 选中的推理配置路由会显示它指向哪个基础模型。

在启用模型访问、更改区域或刷新凭据后模型列表看起来过时，运行：

```text
/refresh-model-list
```

这会强制 `ListFoundationModels` 和 `ListInferenceProfiles`，更新缓存的旧版/配置元数据，并在有可用推理配置路由时移除过时的重复基础模型行。

## 可选请求参数

```bash
export JCODE_BEDROCK_MAX_TOKENS=4096
export JCODE_BEDROCK_TEMPERATURE=0.2
export JCODE_BEDROCK_TOP_P=0.9
export JCODE_BEDROCK_STOP_SEQUENCES='</done>,STOP'
```

## 模型发现

Jcode 会立即使用静态 Bedrock 模型列表。当模型预取/目录刷新运行时，它调用 `ListFoundationModels` 和 `ListInferenceProfiles`，然后把结果缓存在 Jcode 的配置目录中。缓存的 Bedrock 目录按区域隔离；如果你切换 `JCODE_BEDROCK_REGION`/`AWS_REGION`，Jcode 会忽略旧区域缓存并为新区域刷新。

## 实时冒烟测试

实时测试默认忽略。只在有有效 AWS 凭据和已启用模型访问时运行：

```bash
JCODE_BEDROCK_LIVE_TEST=1 \
AWS_PROFILE=my-profile \
AWS_REGION=us-east-1 \
cargo test -p jcode --lib provider::bedrock::tests::bedrock_live_smoke_test -- --ignored
```

## 故障排查

- `AccessDenied`：授予 Bedrock 调用/列出权限并在 AWS 控制台启用模型访问。
- `model not found` 或验证错误：检查模型 ID/推理配置和区域支持。
- SSO 令牌错误：运行 `aws sso login --profile <profile>`。
- API 密钥认证：设置 `AWS_BEARER_TOKEN_BEDROCK` 和 `AWS_REGION`。
- 缺少区域：设置 `AWS_REGION` 或 `JCODE_BEDROCK_REGION`。
