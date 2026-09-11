<div align="center">

# jcode（本 fork）

[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS-blue?style=flat-square)](docs/发布流程.md)

一个服务当前真实情况的 jcode 工作副本<br>
基于 [1jehuang/jcode](https://github.com/1jehuang/jcode) 的功能减法 fork

</div>

---

本仓库是 **`klaus2918/jcode`**，上游为 **`1jehuang/jcode`**。它保留 jcode 的核心工作方式
（单服务器 + 多客户端、TUI 优先、智能体回合循环、记忆图、集群、自研模式），
但**主动移除了大量外围能力**，并把模型接入改成**纯配置驱动**。

> **先读这一节**：本 fork 与 upstream 有实质差异（删了什么、怎么装、怎么接模型），
> 差异写在最前，避免你按 upstream 的文档操作后踩空。

---

## 一、本 fork 与 upstream 的差异

### 1.1 已移除的能力（本仓库代码中不存在）

| 能力 | 说明 |
|---|---|
| 仓内桌面端 GUI | 无 `jcode-desktop*` crate；仅保留 TUI 与服务器/客户端 |
| 浏览器工具 / `computer` | 不内置浏览器与 macOS 计算机使用工具 |
| 遥测上报 | 无遥测 crate、无遥测后端，不发送任何遥测数据 |
| 赞助商 / 发现（discovery） | 相关工具与转化链路全部移除 |
| 云同步（Jade） | 无 `jcode cloud` 子命令与中继通道 |
| iOS 客户端 | 仓内无 `ios/` |
| 登录 / 账户层 | 无 `/login`、`/logout`、`/account`、`/auth` 与对应 CLI；改为配置驱动接入 |
| 内置第三方 provider 预设 | 不再内置 Claude/OpenAI/Gemini 等预置模型常量，改由配置声明 |
| PDF | 无 `jcode-pdf` 与 `/pdf` |
| 无人值守（overnight）/ 邮件通知 | 无 `jcode-overnight-core`、`jcode-notify-email` |
| 全局热键 / 键位冲突检测 | 无 `jcode-setup-hints` 与 `/keys`（保留 `/hotkeys` 查看当前键位） |
| Harness API / SDK | 无 `jcode-harness-api(-server)`、无 SDK 层 |
| 在线自更新 | `jcode update` 仅支持本地包（`--local`）与源码重建 |

删除带来的直接结果：**crate 数从 82 降到 52**，依赖图与攻击面同步收缩
（例如不再有 PDF/邮件/Azure 相关依赖）。

### 1.2 模型接入：配置驱动

不再有交互式登录。接入方式固定为三条：

1. **声明端点**：`jcode provider add <name> --base-url <url> --model <id> ...`
2. **存密钥**：值写入统一密钥文件 `~/.jcode/.env`（`ENV_VAR=value`），**不写进 `config.toml`**
3. **选用**：`jcode --provider <name> run '...'`，或在 TUI 里用 `/model`、`/provider` 切换

同时支持 resonix 风格的顶层 `[[providers]]` 数组与经典 `[providers.<name>]` 表两种写法
（详见 [模型接入](docs/模型接入.md)）。

### 1.3 发布与安装

- 发布走**本仓库自己的** tag 驱动流水线，**只发布 Windows x86_64**（唯一构建作业）；
  **产物未做 Authenticode 签名**（首次运行会有 SmartScreen 提示）。规范见 [发布流程](docs/发布流程.md)。
- **不要用 upstream 的在线安装脚本安装本 fork 的产物**：`scripts/install.sh` / `install.ps1` 仍指向
  upstream 的仓库与 `jcode.sh` 元数据服务。请用本仓库的本地安装方式（见 [安装](#二安装)）。

### 1.4 文档

本仓库文档以**本仓库代码为准**并全部为中文，位于 `docs/`（[docs/文档索引.md](docs/文档索引.md) 是入口）。

---

## 二、安装

### 2.1 从源码构建（推荐，任何平台）

```bash
git clone https://github.com/klaus2918/jcode.git
cd jcode
cargo build --release
```

开发迭代建议用 selfdev profile（编译更快）：

```bash
scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode
scripts/dev_cargo.sh --print-setup    # 查看实际使用的链接器/缓存配置
```

把构建产物装到本机（脚本会自己用 `release-lto` profile 构建，并写入不可变版本目录、
更新 `stable`/`current` 通道、把启动器指向 `current`；加 `--fast` 用普通 release 跳过 LTO）：

```bash
scripts/install_release.sh          # release-lto
scripts/install_release.sh --fast   # 普通 release，编译更快
```

### 2.2 用本地安装包安装 / 更新（完全离线）

已装好 jcode 后：

```powershell
jcode update --local C:\dist\jcode-windows-x86_64.tar.gz
jcode update --local C:\dist\jcode-windows-x86_64.exe
```

全新安装（Windows）：

```powershell
.\install.ps1 -ArtifactExePath C:\dist\jcode-windows-x86_64.exe
.\install.ps1 -ArtifactTgzPath C:\dist\jcode-windows-x86_64.tar.gz
```

全程不访问网络：不查发布、不下载、不取校验和。

### 2.3 Windows 单 exe 布局（本地自研构建）

```powershell
.\scripts\update_local_install.ps1            # 部署到 %JCODE_HOME%\bin\jcode.exe
```

该脚本会把 `JCODE_HOME` 持久化为用户环境变量。三条安装路径的通道语义互不相同，
详见 [Windows 支持](docs/Windows平台.md)。

### 2.4 平台支持

| 平台 | 从源码构建 | 发布二进制 |
|---|---|---|
| **Windows** x86_64 | 支持 | **唯一发布产物**（缺失则发布停留在 draft） |
| **Windows** aarch64 | 支持 | 不发布；只有 `windows-smoke.yml` 冒烟构建 |
| **Linux** x86_64 / aarch64 | 支持 | 不发布 |
| **macOS** Apple Silicon / Intel | 支持 | 不发布 |
| **FreeBSD** x86_64 | 支持 | 不发布 |

本 fork 只发布 Windows x86_64：其他平台仍可从源码构建与运行，但不再产出发布资产，
因此在 GitHub release 的 `Platform availability` 里会明写「仅发布 Windows x86_64」，
避免被误读成构建失败。

### 2.5 卸载

```bash
bash scripts/uninstall.sh --dry-run   # 先预览会删什么
bash scripts/uninstall.sh --yes       # 删二进制与启动器，保留配置/凭据/会话
bash scripts/uninstall.sh --purge --yes   # 连配置/凭据/会话/日志/记忆一起清空
```

Windows 用 `scripts/uninstall.ps1`。

---

## 三、快速开始

```bash
# 启动 TUI
jcode

# 非交互执行一次
jcode run "say hello"

# 按可记忆的名字恢复会话
jcode --resume fox

# 常驻服务器，再挂接更多客户端
jcode serve
jcode connect
```

TUI 里常用的斜杠命令：`/model`、`/provider` 切换模型与 provider；`/skill` 查看技能；
`/skill-reload`、`/mcp-reload` 热重载技能与 MCP；`/mcp` 查看 MCP 服务器；
`/session`、`/resume` 管理会话；`/swarm` 多代理；`/compact` 压缩上下文；
`/alignment` 切换居中对齐；`/hotkeys` 查看键位。

CLI 共 22 个顶级子命令：`serve`、`acp`、`server`、`connect`、`run`、`repl`、`update`、
`version`、`usage`、`self-dev`、`debug`、`auth`、`provider`、`memory`、`session`、
`ambient`、`pair`、`permissions`、`replay`、`model`、`provider-test-coverage`、`restart`。

---

## 四、模型接入（配置驱动）

### 4.1 一次性添加 provider

```bash
# 托管的 OpenAI 兼容端点，密钥从 stdin 读入（不进 shell 历史）
printf '%s' "$MY_API_KEY" | jcode provider add my-api \
  --base-url https://llm.example.com/v1 \
  --model my-model-id \
  --api-key-stdin \
  --set-default

# 用现有环境变量名引用密钥，不落盘
jcode provider add openrouter --base-url https://openrouter.ai/api/v1 \
  --model deepseek/deepseek-chat --api-key-env OPENROUTER_API_KEY

# 本地无鉴权端点
jcode provider add ollama-local --base-url http://localhost:11434/v1 \
  --model llama3.2 --no-api-key
```

常用参数：`--api-key-env`（引用环境变量）、`--api-key-stdin`（安全读入并写入统一 `.env`）、
`--context-window`（上下文窗口）、`--model-catalog`（用端点的 `/models`）、
`--api`（`openai-compatible` 或 `anthropic`）、`--proxy`、`--overwrite`、`--set-default`、`--json`。

jcode 不硬编码厂商名：每个服务都是一条配置。

### 4.2 本地运行时

Ollama 与 LM Studio 都提供 OpenAI 兼容的 `/v1/models` 与 `/v1/chat/completions`，
jcode 使用流式对话、函数/工具调用与 OpenAI 风格图像内容：

```bash
ollama pull llama3.2
jcode provider add ollama-local --base-url http://localhost:11434/v1 --model llama3.2 --no-api-key

jcode provider add lmstudio --base-url http://localhost:1234/v1 --model '<model-id>' --no-api-key

# 自建 vLLM
jcode provider add local-vllm --base-url http://localhost:8000/v1 \
  --model Qwen/Qwen3-Coder-30B-A3B-Instruct --no-api-key --set-default
```

`http://` 仅允许 localhost 与私有 LAN 地址；公网明文 HTTP 会被拒绝。

### 4.3 配置写法（两种等价风格）

```toml
[provider]
default_provider = "my-api"
default_model = "my-model-id"

# resonix 风格：每个端点一条 [[providers]] 记录
[[providers]]
name = "my-api"
type = "openai-compatible"   # 或 "open-router"
kind = "openai"              # 或 "anthropic"
base_url = "https://llm.example.com/v1"
model = "my-model-id"
api_key_env = "JCODE_PROVIDER_MY_API_API_KEY"
context_window = 128000
```

等价的经典表写法：

```toml
[providers.my-api]
type = "openai-compatible"
base_url = "https://llm.example.com/v1"
api_key_env = "JCODE_PROVIDER_MY_API_KEY"
default_model = "my-model-id"
```

两种写法解析到同一内部结构；jcode 在保存配置时会**保持你写的风格**（手写的
`[[providers]]` 数组不会被改写成表）。

### 4.4 CC Switch 本地网关

[CC Switch](https://github.com/farion1231/cc-switch) 持有真实密钥并跑一个本地代理
（默认 `http://127.0.0.1:15721`）。把 jcode 指向它就得到一个带故障转移与格式转换的入口：

```toml
[provider]
default_provider = "cc-switch"
# default_model 可省略：省略时自动跟随 CC Switch 里启用的 provider

[providers.cc-switch]
type = "openai-compatible"
base_url = "http://127.0.0.1:15721"
api = "anthropic"                  # 代理按 Claude 通道路由
auth = "none"                      # 密钥由 CC Switch 注入
replay_reasoning_content = true    # DeepSeek 风格网关要求回传无签名 thinking

[[providers.cc-switch.models]]
id = "deepseek-v4-flash"
context_window = 1000000
auth = "none"
```

要点：

- **模型可省略**：不写 `default_model`/`models` 时，jcode 启动时拉取代理的 `/v1/models` 目录，
  自动选当前启用 provider 的第一个模型；请求失败会重取目录重试。显式指定的模型永远不会被覆盖。
- **密钥不需要**：`auth = "none"`，由代理注入。
- **DeepSeek 风格网关必须回显 thinking**：这类网关返回没有 Anthropic 签名的 thinking，并在后续
  请求缺少它时拒绝；给该 profile 设 `replay_reasoning_content = true` 即可。官方 Anthropic API
  相反会拒绝无签名 thinking，因此保持默认关闭。

### 4.5 注入非标准请求字段（`extra_body`）

部分 OpenAI 兼容后端需要额外的顶层字段（例如 NVIDIA NIM 的 DeepSeek-V4 需要
`chat_template_kwargs` 才开启思考）。两种注入方式：

```toml
# 方式一：按 profile 配置
[[providers]]
name = "my-nim"
type = "openai-compatible"
base_url = "https://integrate.api.nvidia.com/v1"
model = "deepseek-ai/deepseek-v4-flash"
api_key_env = "NVIDIA_API_KEY"
extra_body = { chat_template_kwargs = { thinking = true, reasoning_effort = "high" } }
```

```bash
# 方式二：环境变量（可放在统一 .env 里）
JCODE_OPENAI_EXTRA_BODY={"chat_template_kwargs":{"thinking":true,"reasoning_effort":"high"}}
```

`extra_body` 的键最后合并并覆盖同名生成字段；环境变量优先于配置。非法值只记日志、不失败请求。

### 4.6 流式与上下文

- `JCODE_STREAM_IDLE_TIMEOUT_SECS`（或 `[provider] stream_idle_timeout_secs`）：拉长流式空闲
  超时（默认 180s），适合长时间静默思考的推理模型；高 reasoning effort 会自动放大（high 2×、
  xhigh 3×、max 4×）。
- 每个模型可在 `[[providers.<name>.models]]` 里设 `context_window`（别名 `context_limit`），
  端点没有可用 `/models` 响应时避免回落到通用 200k 默认值。

### 4.7 MCP 配置

MCP 配置独立于 `config.toml`：

- `~/.jcode/mcp.json`（全局）
- `.jcode/mcp.json`（项目）

兼容 Claude Code 的 `~/.claude.json`（含 `projects.<abs_path>.mcpServers`）与仓库根 `.mcp.json`；
`mcpServers` 与历史 `servers` 键都接受。当前仅支持 stdio（命令式）服务器，
HTTP/SSE 条目会被识别并跳过（有日志）。

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "/path/to/mcp-server",
      "args": ["--root", "/workspace"],
      "env": {},
      "shared": true
    }
  }
}
```

首次运行时，如果 `~/.jcode/mcp.json` 不存在，会尝试从 `~/.claude.json`（或旧版
`~/.claude/mcp.json`）与 `~/.codex/config.toml` 导入。TUI 里 `/mcp` 查看实时列表，
`/mcp-reload` 重新读取配置并在原地重连，无需重启。

---

## 五、能力概览（本仓库实际存在）

### 5.1 记忆

每轮对话会被嵌入为语义向量，并与记忆图做余弦相似度检索，命中结果注入上下文；
可选由记忆伴生智能体（sidecar）校验相关性并做进一步取回。记忆的提取/入库由伴生智能体在
语义漂移、距上次提取 K 轮、会话结束等时机触发。环境模式会周期性对记忆做整合
（去重、陈旧与冲突检查）。

同时提供显式记忆工具（主动检索/写入）与**会话检索**（对历史会话做传统 RAG）。

### 5.2 技能

技能从 `~/.jcode/skills/`（全局）、`./.jcode/skills/`、`./.agents/skills/`（项目）与
`./.claude/skills/`（兼容）加载；首次安装会从 `~/.claude/skills/` 导入一次。

可用技能清单以**静态列表**形式写入系统提示词（名称 + 描述），智能体按需用技能工具
读取正文；`/skill` 查看当前清单，`/skill-reload` 重新读取目录并加载新技能（项目技能按
会话作用域隔离），无需重启。

### 5.3 MCP

见 [4.7](#47-mcp-配置)。`/mcp` 查看、`/mcp-reload` 重载。

### 5.4 集群（Swarm）

在同一仓库里派生多个智能体，由服务器统一管理协作：当 A 改动了 B 读过的文件，
服务器会通知 B，B 可以忽略或查看 diff 确认是否冲突。每个智能体都能私聊单个智能体、
广播给全部或仅仓库内智能体。智能体也可以自行派生集群（自己成为协调者，被派生者成为
worker），有头无头都可以。

### 5.5 环境模式（Ambient）

常驻后台按调度运行：`jcode ambient status|log|trigger|stop` 管理与查看。

### 5.6 界面

- **侧栏**：把文件/内容加载到侧栏实时更新，或当 diff 查看器用。
- **Mermaid**：侧栏与对话内联渲染（自研渲染库，无浏览器/TypeScript 依赖）。
- **信息组件**：只占用屏幕负空间显示信息，没有空间时自动让位。
- **对齐**：默认左对齐；`Alt+C`、`/alignment` 或配置切换到居中。
- **表情**：`[display] emoji = false` 或 `JCODE_NO_EMOJI=1` 全局关闭（用紧凑 ASCII 替代）。
- 自定义滚动回看（比终端原生回看能做更多事）；普通终端的滚动体验同样完整。

### 5.7 会话检索与跨 harness 恢复

`session_search` 覆盖 jcode 自己的会话，以及 **Claude Code、Codex、pi、OpenCode、Cursor**
的外部历史；可以检索并按来源过滤。`/resume` 也会列出这些外部会话，因此其他 harness
中断了可以直接在 jcode 里接着往下做。

### 5.8 远程与配对

`jcode pair`（生成配对码 / 列出 / 吊销设备）用于远程客户端接入；
另有 SSH 远程会话支持（把本地 TUI 挂到远端服务器）。

### 5.9 自研模式

让智能体改自己的源码：`jcode self-dev`（别名 `selfdev`）进入 canary 会话，
配合自研基础设施可以编辑、构建、测试自身源码，然后重载自己的二进制并继续在（可能多个）
会话里工作。建议使用前沿模型做这件事——本代码库不简单，弱模型容易做出隐蔽的破坏性改动。

### 5.10 ACP

`jcode acp`：以 Agent Client Protocol 适配器方式运行，后端仍是 jcode 守护进程。

---

## 六、本仓库没有的东西（避免误解）

- **不内置浏览器自动化**：需要在外部用技能实现（例如 `op-browser`：Playwright + Chromium/Chrome），
  这样二进制与任何浏览器/测试后端解耦。
- **不发送遥测**：本 fork 无遥测链路。
- **没有登录 UI**：模型接入全部走配置（见 [第四节](#四模型接入配置驱动)）。
- **不自动联网更新**：`jcode update` 只处理本地包。

---

## 七、配置速查

| 位置 | 用途 |
|---|---|
| `~/.jcode/config.toml` | 主配置（provider 声明、显示、键位、工具、安全等） |
| `~/.jcode/.env` | 统一密钥文件（所有 API key 与相关覆盖项） |
| `~/.jcode/mcp.json` | 全局 MCP 服务器 |
| `.jcode/mcp.json` | 项目 MCP 服务器 |
| `.jcode/skills/`、`~/.jcode/skills/` | 项目 / 全局技能 |
| `.jcode/swarm-prompt.md` | 集群模型路由指引（`/swarm-prompt` 编辑） |

旧式分散密钥文件（`~/.config/jcode/*.env`）仍可读取，启动时会自动迁移到统一 `.env`。

---

## 八、文档

仓库内文档入口：[docs/文档索引.md](docs/文档索引.md)（共 16 篇，全部中文、只描述本仓库现状）。

常用入口：

- 架构与模块：[架构总览](docs/架构总览.md)、[模块与依赖边界](docs/模块与依赖边界.md)
- 模型接入：[模型接入](docs/模型接入.md)
- 记忆与集群：[记忆系统](docs/记忆系统.md)、[集群](docs/集群.md)
- 常驻自主：[环境模式](docs/环境模式.md)、[安全系统](docs/安全系统.md)
- 交互与界面：[交互机制](docs/交互机制.md)、[TUI 与终端](docs/TUI与终端.md)、[TUI 实现笔记](docs/TUI实现笔记.md)
- 扩展与配置：[扩展钩子](docs/扩展钩子.md)、[脚本与配置](docs/脚本与配置.md)
- 平台与发布：[Windows 平台](docs/Windows平台.md)、[发布流程](docs/发布流程.md)
- 工程质量：[工程质量](docs/工程质量.md)

历史计划、时点审计与未落地提案已不再单独成文（有价值部分并入上述文档并标注「设计意图/未实现」），
需要时请查 git 历史。

上游资料（英文）：[jcode.sh/docs](https://jcode.sh/docs)、[jcode.sh/bench](https://jcode.sh/bench)、
[upstream 仓库](https://github.com/1jehuang/jcode)

> 注意：指向 `jcode.sh` 与 upstream 仓库的资料描述的是 **upstream** 的行为，
> 其中可能包含本 fork 已移除的能力；以本仓库文档与代码为准。

---

## 九、性能与资源占用（upstream 测量数据）

以下数据来自 **upstream** 的测量（被测版本 `jcode v0.9.1888-dev`），**本 fork 未重新测量**。
本 fork 移除了若干能力，只会减小体积与占用，但**没有做过对应基准**，请按"参考量级"理解。

**单会话内存（PSS）**：jcode（关闭本地嵌入）27.8 MB，jcode 167.1 MB；
对照 Claude Code 386.6 MB、OpenCode 371.5 MB、GitHub Copilot CLI 333.3 MB、
Antigravity CLI 243.7 MB、Cursor Agent 214.9 MB、pi 144.4 MB、Codex CLI 140.0 MB。

**10 会话内存（PSS）**：jcode（关闭本地嵌入）117.0 MB，jcode 260.8 MB；
对照 pi 833.0 MB、Antigravity CLI 1021.2 MB、Cursor Agent 1632.4 MB、
GitHub Copilot CLI 1756.5 MB、Claude Code 2300.6 MB、OpenCode 3237.2 MB。

**每新增会话的额外占用**：jcode 约 9.9 MB（关闭本地嵌入）/ 10.4 MB；
对照 Codex CLI 21.6 MB、pi 76.5 MB、Antigravity CLI 86.4 MB 及以上。

**首帧时间**：jcode 14.0 ms（对比 Antigravity 383.5 ms、pi 590.7 ms、Codex CLI 882.8 ms）。
**首次可输入时间**：jcode 48.7 ms。

---

## 十、致谢与许可

- 上游项目：[1jehuang/jcode](https://github.com/1jehuang/jcode)（MIT）。
- 许可：[MIT](LICENSE)。
