# 评估报告：jcode 内嵌浏览器测试工具（BrowserTool）剥离可行性

- 评估日期：2026-08-03
- 评估对象：jcode 仓库（Windows，分支 master）
- 评估方式：agentgrep / read 逐点核实代码引用，未做任何代码修改
- 结论速览：**"只移除 A（BrowserTool 工具本体）、保留 B（桥基础设施）与 C（ChatGPT Web 传输）"完全可行**，改动面收敛、无编译阻断风险，但存在 UI 历史数据退化与 CLI 文案误导两类中风险，需按本文第 4 节缓解。

---

## 1. 概念分层与代码现状核实

背景中确认的 4 个纠缠概念在代码中全部核实属实：

| 概念 | 代码位置 | 核实结论 |
|---|---|---|
| A. BrowserTool | `crates/jcode-app-core/src/tool/browser.rs`（约 950 行）+ `browser_tests.rs`（约 256 行，11 个测试） | 走 `Tool` 特征，内部含 `BrowserProvider` trait、`FirefoxBridgeProvider`、动作映射、截图、输出格式化。**A 专属，可整文件删除** |
| B. 桥基础设施 | `crates/jcode-base/src/browser.rs`（约 950 行）+ `browser_tests.rs` | 下载/安装/状态/会话管理，从 GitHub `1jehuang/firefox-agent-bridge` 拉取二进制。**B 共享，必须保留** |
| C. ChatGPT Web 传输 | `crates/jcode-provider-openai-runtime/src/chatgpt_web.rs` 等 | 通过 `jcode_base::browser::browser_binary_path()` 与 `ensure_browser_ready_noninteractive()` 直接驱动 B。**依赖 B，与 A 无代码耦合，保留** |
| D. 系统浏览器无关逻辑 | `auth::browser_suppressed`、`tool/open.rs` Linux niri 聚焦、oauth | 与桥无关，仅命名撞车。**勿动** |

关键结构事实：`crates/jcode-app-core/src/lib.rs:24` 有 `pub use jcode_base::*;` 全局重导出，因此 `tool/bash.rs`、`tool/browser.rs` 里的 `crate::browser::` 实际解析到 **B**（`jcode_base::browser`）。A 删除后，bash.rs 对 `crate::browser` 的引用依旧成立，**编译不受影响**。

---

## 2. 移除 A 的完整影响面（逐点确认）

### 2.1 A 专属，可直接删除

| # | 引用点 | 内容 | 处置 |
|---|---|---|---|
| 1 | `tool/browser.rs` 整文件 | `BrowserTool`、`BrowserInput/Field/ScrollTo`、`BrowserProvider` trait、`FirefoxBridgeProvider`、动作映射 `bridge_request`、`build_press_script`、`firefox_run_bridge_command`、`screenshot_via_bridge`、`format_*` 输出格式化 | 删 |
| 2 | `tool/browser_tests.rs` 整文件 | 11 个单元测试（press/snapshot/eval/interactables/schema/resolve_provider 等） | 删 |
| 3 | `tool/mod.rs:7` | `mod browser;` | 删 |
| 4 | `tool/mod.rs:189` | `Self::insert_tool_timed(&mut m, &mut timings, "browser", browser::BrowserTool::new);` | 删（唯一的工具注册点，`resolve_tool_name` 无 browser 别名，无 OAuth/MCP 别名，无需连带清理） |
| 5 | `jcode-tui/src/tui/ui_tools.rs:449-623` | `browser_target_summary` + `browser_summary`（约 175 行专属渲染） | 删 |
| 6 | `jcode-tui/src/tui/ui_tools.rs:1115` | `"browser" => browser_summary(tool, max_width)` 分发分支 | 删（默认分支可兜底，见 4.5） |
| 7 | `jcode-tui/src/tui/app/state_ui_storage.rs:357-396` | `"browser" => obj([action/url/selector/...])` 存储压缩分支 | 删（默认分支保留 action 兜底） |
| 8 | `jcode-tui/src/tui/ui_tests/tools.rs` | 5 个 browser 专属 UI 测试（open/type/eval/无 selector 变体/activity detail） | 删 |
| 9 | `jcode-tui/src/tui/app/state_ui_storage.rs:714-733` | 测试 `compaction_keeps_browser_action_and_intent_for_transcript_summary` | 删 |
| 10 | `crates/jcode-base/src/config/default_file.rs:286` | 注释 `# disabled = ["browser", "gmail", "swarm"]`（仅示例注释，非默认值） | 顺手改或保留（无害） |
| 11 | `docs/BROWSER_PROVIDER_PROTOCOL.md` | A 时代的桥协议文档（未编译） | 建议更新或归档 |

**注意**：`ui_tools.rs` 中的 `truncate_url_display` 同时被 `webfetch`（1102 行）与 `open`（1175 行）使用，**不可删**（删 A 时容易误伤）。

### 2.2 B 共享，必须保留

`crates/jcode-base/src/browser.rs` 的公开 API 及其全部使用方（核实无误）：

| B 公开符号 | 使用方 |
|---|---|
| `browser_binary_path()` | A（browser.rs:756）、C（chatgpt_web.rs:748） |
| `ensure_browser_session()` | A（browser.rs:772）、bash 拦截（bash.rs:719） |
| `is_browser_command()` / `rewrite_command_with_full_path()` | bash 拦截（bash.rs:711-712） |
| `ensure_browser_ready_noninteractive()` | A（browser.rs:357/413/435）、C（chatgpt_web.rs:99）、CLI（commands.rs:505） |
| `ensure_browser_setup()` | A（browser.rs:412）、CLI（commands.rs:503 经 `run_setup_command`） |
| `inspect_browser_status()` / `is_setup_complete()` / `run_setup_command()` | CLI（commands.rs:503-505）、B 内部 |
| `BrowserStatus` | A、CLI |

`crates/jcode-base/src/browser_tests.rs`（B 的测试）同样保留。

### 2.3 C 专属，必须保留（与 A 无耦合）

| 位置 | 说明 |
|---|---|
| `chatgpt_web.rs` | `run_turn` 调 `ensure_browser_ready_noninteractive()`；`bridge_command` 调 `browser_binary_path()` 直接跑桥 CLI |
| `openai_provider_impl.rs:983-1035` | `transport()`/`set_transport()`/`available_transports()` 中字符串 `"browser"` 指 **chatgpt-web 传输方式**，非 A 工具，必须保留 |
| `openai-runtime lib.rs:744-871` | `new_browser_only`、`browser_only` 标志、`is_chatgpt_web_model` |
| `jcode-base/src/provider/route_builders.rs:160-170` | `build_chatgpt_web_route()`（api_method `"chatgpt-web"`） |
| `jcode-base/src/provider/catalog_routes.rs:42-43, 385-386` | 模型选择器接入 chatgpt-web 路由 |
| `jcode-base/src/provider/models.rs:993-998` | `CHATGPT_WEB_MODEL` 可用性（source `"browser-session"`） |
| `src/cli/startup.rs:172-179` | `register_external_provider_runtimes` 的 `new_browser_only()` 回退 |
| `jcode-provider-core/src/models.rs:30` | `CHATGPT_WEB_MODEL = "gpt-5.6-pro[web]"` 常量 |

C 侧测试（`openai_tests.rs`、`openai_tests/models_state.rs`、`openai_tests/payloads.rs`、`jcode-base provider tests.rs`、`tests/catalog_subscription.rs`）全部围绕模型/传输逻辑，不触 A，保留。

### 2.4 命名撞车 D，勿误伤（核实为"系统浏览器/通用词"语义）

| 位置 | 语义 |
|---|---|
| `auth::browser_suppressed`（`jcode-base/src/auth/mod.rs:105`）及其全部调用方：`auth/oauth.rs`、`auth/google.rs`、`auth/login_diagnostics.rs`、`auth/doctor.rs`、`src/cli/login.rs`、`src/cli/login/scriptable.rs`、`src/cli/account.rs`、`tool/gmail.rs:115`、`tool/open.rs:348/391`、`tui/app/helpers.rs:114` | OAuth/登录是否抑制弹出**系统浏览器**，与桥完全无关 |
| `tool/open.rs:498-720` | Linux niri 打开 URL 后的窗口聚焦（`browser_app_stems` 等），指系统默认浏览器 |
| `src/cli/args.rs:191-193, 328-329, 467-471` | `--no-browser` 参数与 `Command::Browser` 子命令。其中 `Command::Browser`（`jcode browser setup/status`）属于 **B/C 维护入口，保留**；`--no-browser` 属于 D，保留 |
| `auth.rs`（TUI）、`copy_selection.rs`、`ui.rs`、`app.rs`、`tui/mod.rs` 注释 | "browser-style" 滚动/选择等 UI 措辞 |
| `provider-anthropic/lib.rs:397/490/1069` | 注释 + 测试里把 `browser` 当作"自定义工具名"样例（OAuth 工具集透传测试）；测试不依赖 A 存在与否，**保留不破坏** |
| `jcode-base/src/skill.rs:721` | EndorsedSkill `firefox-browser`：推荐的外部 skill 元数据，与 A 无编译依赖；若 B/C 保留，该 skill 仍可经 bash 拦截使用，**保留** |
| `mcp/manager.rs`、`mcp/protocol.rs` 注释 | "Playwright with browser state" 示例措辞 |
| `sponsors.rs:39,79` | 发现目录分类 `"browser-automation"`（通用分类） |
| `update.rs`、`jcode-update-core/lib.rs:49` | GitHub asset 字段 `browser_download_url`（GitHub API 字段名） |
| `gateway.rs:153`、`compaction-core` 提示语、`overnight prompts`、`session_search_tests.rs:510`、`todo.rs:849`、`ui_messages/tests.rs` | 通用词/测试样例/提示文案 |
| `computer/mod.rs:3` | 注释"桌面端等价于 browser 工具"（macOS ComputerTool，未编译关联） |

---

## 3. 判定：能否"只移除 A 而保留 B/C"

**结论：可以。** 依据：

1. **依赖单向**：A→B 单向调用，C→B 单向调用，A 与 C 之间无任何直接引用（核实 `chatgpt_web.rs` 不引用 `BrowserTool`）。
2. **A 完全自治**：A 的全部实现集中在 `tool/browser.rs` 一个文件 + 其测试文件，注册点仅 `tool/mod.rs:189` 一处，无 feature 门控、无全局状态泄漏到其他模块。
3. **删除后编译安全**：删除 `mod browser;` 后，`bash.rs` 的 `crate::browser::*` 仍经 `pub use jcode_base::*` 解析到 B，编译器可立即发现任何遗漏引用。
4. **遗留的耦合全部是"字符串级"**：工具名 `"browser"` 出现在 UI 渲染、存储压缩、配置注释、测试样例里，均为展示/文案层，删除后走通用兜底路径即可，不破坏编译与运行。

**需要同步处理的连带项**（不属于 A 本体但属 A 时代的表述）：
- CLI `commands.rs:546` 文案 `"Built-in browser tool is ready."`（A 时代措辞，B/C 保留时应改为"Browser bridge is ready."）。
- `chatgpt_web.rs` / `browser.rs` 错误提示引用 `jcode browser setup/status`（CLI 保留，提示有效）。

---

## 4. 移除风险清单

| # | 风险项 | 等级 | 被影响功能 | 缓解措施 |
|---|---|---|---|---|
| 1 | 编译断裂：漏删 `crate::browser::`（A 侧）或误删 B 的公共符号 | **低** | 全仓编译 | A 删除后 `tool/mod.rs` 不再引用 `browser::`，唯一遗留是 bash.rs（指向 B，保留）；跑一次 `cargo check` 即可全量捕获；**勿删** `ui_tools.rs::truncate_url_display`（webfetch/open 共用） |
| 2 | C（ChatGPT Web 模型传输）被牵连 | **低** | gpt-5.6-pro[web] 模型 | C 只依赖 B 与 transport 字符串 `"browser"`（传输语义）；删除范围明确排除 `jcode-provider-openai-runtime`、`route_builders/catalog_routes/models`、`startup.rs` 与 `--provider openai` 登录流 |
| 3 | bash 拦截（`is_browser_command`/`rewrite_command_with_full_path`/`ensure_browser_session`）误删 | **中** | 用户经 bash 直接调用桥 CLI 的场景（A 时代遗留的另一种浏览器驱动入口） | 明确这属于 B 共享，**保留**；若一并删除则属于扩大范围，需用户另行确认 |
| 4 | CLI `jcode browser setup/status` 保留但文案误导 | **中** | 桥安装/诊断流程 | 保留命令（C 依赖其维护路径）；更新 `commands.rs:546` 文案去掉"Built-in browser tool"，改为桥就绪语义 |
| 5 | 历史会话数据（已存储 transcript）中残留 browser 工具调用 | **中** | TUI 会话恢复/摘要渲染 | 已核实两条兜底路径均安全：`state_ui_storage.rs` 默认分支保留 `action` 字段不崩溃；`ui_tools.rs` 默认分支渲染 action 字符串。删除 browser 专属分支后，旧记录的摘要从"open https://..."退化为"browser"，属可接受的显示退化。如需更平滑，可在删除分支前保留 1 个版本期（可选） |
| 6 | 配置层遗留 `disabled = ["browser"]` 等用户配置 | **低** | 无 | 删除工具后该配置成为无害空操作（`execute` 对未知工具返回带建议的错误，已核实 `tool/mod.rs:557-571` 的 Unknown tool 路径）；默认配置模板注释 `default_file.rs:286` 顺手清理 |
| 7 | 测试断链：UI/配置测试引用 `"browser"` 工具名 | **低** | 测试套件 | 已核实：`ui_tests/tools.rs` 5 个与 `state_ui_storage.rs` 1 个是 A 专属测试（删除）；`config_tests.rs` 3 个、`provider-anthropic` 1 个用 browser 作"样例工具名"，测试机制本身不依赖 A 存在，**删除后仍通过**（语义上可保留或顺手改名为其它样例名） |
| 8 | 文档与 skill 元数据过时 | **低** | 文档/`/skills` 列表 | `docs/BROWSER_PROVIDER_PROTOCOL.md` 归档或标注"仅桥协议"；`skill.rs` 的 `firefox-browser` endorsed 条目语义仍成立（B 保留），**保留** |
| 9 | 误删 D（命名撞车） | **高（若发生）** | OAuth 登录、系统浏览器打开/聚焦、`--no-browser` | 严格按 2.4 清单执行；`auth::browser_suppressed`、`open.rs` niri 聚焦、`oauth.rs`、`--no-browser` 参数一律不碰 |

---

## 5. 关联功能影响矩阵

| 关联功能 | 与 A 关系 | 移除后影响 | 结论 |
|---|---|---|---|
| ChatGPT Web 模型传输（C） | 仅依赖 B | 无影响；`jcode browser setup/status` 与桥二进制仍可用 | **保留 B/C** |
| bash 拦截（`browser` 命令重写 + 会话绑定） | B 共享（bash.rs:711-719） | 无影响，拦截逻辑保留即不变 | **保留** |
| `jcode browser setup/status` CLI | B 公共 API（commands.rs:501-564） | 无影响；仅 546 行文案需改 | **保留**（文案更新） |
| TUI UI 渲染（`ui_tools.rs`） | A 专属 | browser 行摘要退化为通用 action 渲染；webfetch/open 的 `truncate_url_display` 不受影响 | **删 browser_summary** |
| 存储压缩（`state_ui_storage.rs`） | A 专属 | 旧 browser 记录退化为仅保留 action；不崩溃 | **删 browser 分支** |
| 模型路由（catalog_routes / models / route_builders） | C 专属 | 无影响；`gpt-5.6-pro[web]` 路由与 `browser` transport 字符串保留 | **不动** |
| 配置（default_file 注释、config_tests、用户 disabled 列表） | 字符串级 | 用户配置无感知；注释与样例名可选清理 | **低优先级清理** |
| 编译依赖 | 无 feature 门控（已核实所有引用点均为无条件编译） | 删除 A 即同步移除其约 1200 行代码与其依赖链；B/C 依赖不变化 | **一次性改动** |

---

## 6. 结论与"干净"的合理边界

### 可行性分档

| 档位 | 方案 | 可行度 | 说明 |
|---|---|---|---|
| 第一档（推荐） | **只删 A**：删 `tool/browser.rs` + `browser_tests.rs` + `tool/mod.rs` 的 mod/注册行 + `ui_tools.rs` 的 browser_summary/browser_target_summary/分发分支 + `state_ui_storage.rs` 的 browser 分支 + 相关 6 个 UI 测试；保留 B、C、bash 拦截、CLI 命令；改 `commands.rs:546` 文案 | **完全可行，低风险** | 满足"不深度绑定浏览器测试方式在代码里"的诉求，同时保住 ChatGPT Web 模型与桥维护能力 |
| 第二档 | 第一档 + 顺带删 bash 拦截与 CLI 命令 | 可行但**扩大范围** | 会连带移除"经 bash 用桥"能力，且 C 的错误提示仍指向 CLI；需单独确认，不推荐默认执行 |
| 第三档 | 连 B 一起删（彻底移除 Firefox 桥） | **不可行** | C（ChatGPT Web 模型传输）依赖 B，删除 B 即删除 gpt-5.6-pro[web] 模型能力；除非同步删除 C，否则无法执行 |

### "干净"的合理边界

```
删除：
  crates/jcode-app-core/src/tool/browser.rs
  crates/jcode-app-core/src/tool/browser_tests.rs
  tool/mod.rs:  mod browser;  +  browser::BrowserTool::new 注册行
  jcode-tui ui_tools.rs:  browser_target_summary / browser_summary / "browser" 分发分支
  jcode-tui state_ui_storage.rs:  "browser" 压缩分支 + 对应测试
  jcode-tui ui_tests/tools.rs:  5 个 browser 专属测试
  （可选）default_file.rs:286 注释、docs/BROWSER_PROVIDER_PROTOCOL.md 归档

保留（B/C 共享，勿动）：
  crates/jcode-base/src/browser.rs + browser_tests.rs（桥基础设施，全量）
  bash.rs 的 is_browser_command / rewrite_command_with_full_path / ensure_browser_session 拦截
  cli commands.rs run_browser（jcode browser setup/status）+ dispatch.rs/args.rs 的 Command::Browser
  chatgpt_web.rs / openai_provider_impl / route_builders / catalog_routes / models / startup.rs（C 全量）
  openai_provider_impl 中 transport 字符串 "browser"（chatgpt-web 传输语义）

不碰（命名撞车 D）：
  auth::browser_suppressed 及其全部调用方、open.rs niri 聚焦、--no-browser 参数、
  skill.rs firefox-browser endorsed 条目、sponsors 分类、GitHub browser_download_url 等
```

**最终建议**：按第一档执行。改动预计约 4 个文件删除/瘦身 + 1 处文案修改 + 可选文档归档，无 feature 门控、无新增依赖，`cargo check` + `cargo test`（app-core / tui / base 三个 crate）即可完成验证。用户外置的 op-browser skill（Playwright/CDP/Bridge 双适配器）完全不受影响，因其不编译进 jcode，与本次剥离正交。
