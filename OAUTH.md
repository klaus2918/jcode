# 认证与凭据（本 fork）

本文件说明本仓库（`klaus2918/jcode`）里模型认证实际是怎么工作的。

> **与 upstream 的根本差异**：本 fork **没有交互式登录**。upstream 的这份文档描述的是
> `jcode login --provider <name>` 一族流程（含 Azure / Gemini / Google / Copilot / Cursor /
> Antigravity 等内置 OAuth），**这些在本仓库中不存在**：`jcode auth` 只剩 `status` 与 `doctor`
> 两个子命令，TUI 里也没有 `/login`、`/auth`、`/account`。
>
> 模型接入的完整说明见 [docs/模型接入.md](docs/模型接入.md)；本文只讲"凭据从哪来"。

## 一、凭据的三条来源

| 来源 | 怎么做 | 落点 |
|---|---|---|
| **API 密钥（托管端点）** | `jcode provider add <name> --base-url <url> --model <id> --api-key-stdin`（或 `--api-key-env` 引用已有环境变量） | 统一密钥文件 `<jcode home>/.env` |
| **已有的 OAuth 账户** | 直接用已存在的账户文件（例如从别的工具迁入后保留在 `<jcode home>/auth.json`） | 见下表 |
| **从其它工具导入** | 首次运行会列出检测到的外部登录，逐个批准后**原地读取**（不复制、不改写原文件） | 外部文件本身 |

`<jcode home>` 默认是 `~/.jcode`（设了 `JCODE_HOME` 时就是它本身）。

## 二、凭据文件位置

| 内容 | 路径 | 说明 |
|---|---|---|
| Anthropic OAuth 账户 | `<jcode home>/auth.json` | `anthropic_accounts[]`，当前账户在 `active_anthropic_account` |
| OpenAI OAuth 账户 | `<jcode home>/openai-auth.json` | `openai_accounts[]`，当前账户在 `active_openai_account` |
| API 密钥（统一） | `<jcode home>/.env` | `ANTHROPIC_API_KEY`、`OPENAI_API_KEY`、各 provider 的 `JCODE_PROVIDER_*` 等 |
| jcode 订阅 | `<jcode home>/jcode-subscription.env` | `JCODE_API_KEY`；状态与层级见 TUI `/subscription` |

解析顺序：进程环境变量 → 统一 `.env` → profile 配置指定的 `env_file` → 启动时注册的 OAuth 回退解析器。
`auth.json` 只存 OAuth 账户，**绝不存 API 密钥**。旧式分散密钥文件（`~/.config/jcode/*.env`）
仍可读，启动时自动迁移到统一 `.env`。

## 三、从其它工具导入（同意门控）

首次运行会检测下列来源；**未经批准不会读取**，批准后按路径记住，并且始终原地读取：

| 工具 | 路径 |
|---|---|
| Claude Code | `~/.claude/.credentials.json`，或 macOS 钥匙串项 `Claude Code-credentials`，或 `CLAUDE_CODE_OAUTH_TOKEN` |
| Codex | `~/.codex/auth.json` |
| OpenCode | `~/.local/share/opencode/auth.json` |
| pi | `~/.pi/agent/auth.json` |
| OpenClaw | `~/.openclaw/agent/auth.json`、`~/.openclaw/agents/<id>/agent/auth-profiles.json`、`~/.openclaw/credentials/oauth.json` |
| Hermes | `~/.hermes/auth.json` |

约束：符号链接的外部凭据文件会被拒绝；以 `!`（shell 命令）开头的值**绝不执行**，直接跳过。
Gemini / GitHub Copilot / Cursor 的专用导入器已随功能减法移除，这些厂商请用配置接入。

## 四、诊断

```bash
jcode auth status --json          # 每个 provider 的凭据是否可用、来自哪里
jcode auth doctor [PROVIDER]      # 凭据链路：有没有、从哪来、是否过期、能否刷新
jcode auth doctor --validate      # 额外做一次联网校验
jcode provider-test-coverage      # 每个 (provider, 模型) 对在 12 阶段流水线卡在哪一步
```

`<jcode home>/auth-validation.json` 是**历史记录**而不是当前状态：已自动刷新的令牌仍可能
显示几天前的失败条目，超过 7 天的记录会被标为 `stale`（当作"未知，重新检查"）。

## 五、刷新与失效

- OAuth 令牌在请求时按需刷新；刷新失败会在错误信息里提示
  `jcode auth doctor <provider>` 重新检查凭据，而不再提示已删除的 `/login`。
- 切换多账户：本 fork 没有切换账户的界面入口。若确实需要切换，把上表里的
  `active_anthropic_account` / `active_openai_account` 改成目标账户的 label。
