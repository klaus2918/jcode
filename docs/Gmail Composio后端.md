# Gmail 工具：Composio 托管后端

原生 `gmail` 工具可以从两个后端之一获取凭据和传输。工具接口、确认门控、访问层级逻辑和精简令牌的输出格式在所有后端完全相同；只有认证/传输层不同。

## 后端

| 后端 | 认证 | 优点 | 缺点 |
|---|---|---|---|
| `direct`（默认） | 本地 Google OAuth 令牌（`jcode login google`） | 没有第三方介入 | 未验证应用警告；Google"测试"模式下 7 天刷新令牌过期 |
| `composio` | Composio 托管的 OAuth（Google 验证应用） | 无未验证应用警告、无 7 天过期、无需每个用户一个 Google Cloud 项目 | Composio 代理 Gmail 令牌保管；外部依赖/成本 |

两个后端调用*相同的* Gmail REST 端点（`https://gmail.googleapis.com/gmail/v1/users/me/...`）。Composio 后端通过 Composio 的 [`proxy-execute`](https://docs.composio.dev/reference/api-reference/tools/postToolsExecuteProxy) 端点路由这些调用，由它附加托管的 Gmail 凭据。因为上游响应形态不变，所有现有的类型化解析和输出格式都被复用。

## 选择后端

后端在 `GmailClient::new()` 时从环境解析：

- `JCODE_GMAIL_BACKEND=direct`（或未设置）-> 直接 Google 后端。
- `JCODE_GMAIL_BACKEND=composio` -> Composio 后端（需要 `COMPOSIO_API_KEY`）。

如果请求了 `composio` 但缺少 `COMPOSIO_API_KEY`，jcode 会警告并回退到 `direct`。

### Composio 环境变量

| 变量 | 必需 | 描述 |
|---|---|---|
| `COMPOSIO_API_KEY` | 是 | 来自 <https://platform.composio.dev> 的项目 API 密钥 |
| `COMPOSIO_BASE_URL` | 否 | 覆盖 API 基址（默认 `https://backend.composio.dev/api/v3.1`） |
| `COMPOSIO_GMAIL_AUTH_CONFIG_ID` | 用于 `connect` | 来自 Composio 仪表盘的 Gmail 认证配置 id（`ac_...`）。定义 connect 流程使用的 OAuth 蓝图/范围。 |
| `COMPOSIO_GMAIL_CONNECTED_ACCOUNT_ID` | 否 | 固定某个已连接账户（`ca_...`）。通常在 `connect` 后自动设置。 |
| `COMPOSIO_GMAIL_USER_ID` / `COMPOSIO_USER_ID` | 否 | 多用户已连接账户的终端用户 id（默认 `default`） |

## 连接 Gmail 账户（智能体内 OAuth）

设置 `COMPOSIO_API_KEY` 和 `COMPOSIO_GMAIL_AUTH_CONFIG_ID` 后，用户（或智能体）用 `action: "connect"` 运行 gmail 工具：

1. jcode 调用 Composio 的 `POST /connected_accounts/link`（托管的"Connect Link"流程）启动 OAuth 会话。
2. 返回的 `redirect_url` 在系统浏览器中打开（打印到 stderr 作为回退，例如在 SSH 上）。
3. 用户在 Google 同意屏幕上批准 Gmail 访问。因为 Composio 拥有 Google 验证的应用，没有"未验证应用"警告。
4. jcode 轮询 `GET /connected_accounts/{id}` 直到连接变为 `ACTIVE`，然后持久化到 `~/.jcode/composio_gmail.json`。

未来的会话加载持久化的 `connected_account_id`，因此 connect 步骤是每个账户的一次性操作。连接存在之前的工具调用会返回提示，告诉智能体先运行 `action: "connect"`。

> 注意：Composio 正在淘汰用于托管 OAuth 的 `initiate()`，转而使用这里使用的 Connect Link `link()` 流程，因此这条路径是今后受支持的。

## 一次性 Composio 设置

1. 在 <https://platform.composio.dev> 登录并复制你的项目 API 密钥。
2. 连接一个 Gmail 账户（Composio 托管的 OAuth，无未验证应用警告）。如果想固定它，记下生成的 `connected_account_id`。
3. 导出变量：
   ```bash
   export JCODE_GMAIL_BACKEND=composio
   export COMPOSIO_API_KEY="ck_..."
   # 可选：
   export COMPOSIO_GMAIL_CONNECTED_ACCOUNT_ID="ca_..."
   export COMPOSIO_GMAIL_USER_ID="me"
   ```
4. 确保 `config.toml` 中启用了 `gmail` 工具：
   ```toml
   [tools]
   enabled = ["*"]
   ```

## 访问层级

- `direct`：遵循 `jcode login google` 时选择的访问层级（只读和草稿登录不能发送/删除，在 OAuth 范围层面强制）。
- `composio`：连接请求完整的 Gmail 范围，因此发送/删除可用。工具仍然要求 send、send_draft 和 trash 显式 `confirmed: true`。

## 信任说明

使用 Composio 后端时，Composio 持有你的 Gmail OAuth 授权并看到 API 流量。这是与直接后端相比的核心权衡。在把它作为默认启用之前，应向用户披露这一点。
