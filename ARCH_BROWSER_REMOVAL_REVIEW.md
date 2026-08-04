# 架构审查报告：jcode「浏览器能力整体删除」后的架构影响

- 审查日期：2026-08-03
- 审查方式：agentgrep / read / git diff HEAD 逐点核实，未修改任何代码
- 审查对象：工作区未提交改动（50 文件，+116 / -5082），HEAD = 1fd5d17e8
- 审查结论速览：**架构变好**。删除消除了 base 层「共享桥」的跨层隐性耦合，厂商/能力耦合显著下降，边界更清晰；无编译级残留、无崩溃级运行回归，但存在 1 个测试环境依赖回归（中风险）、3 处过时注释、1 个文档参照过时等遗留项，见第 5 节。

---

## 1. 架构边界审查：三层边界是否清晰

### 1.1 删除前的问题（本次改动的动因）
- 桥基础设施 B（`jcode-base/src/browser.rs`）是**三向共享基础设施**：A（app-core 工具）、C（openai-runtime 传输）、CLI（`jcode browser` 命令）都调用它。
- app-core 通过 `pub use jcode_base::*`（lib.rs:24）全局重导出，导致 `tool/bash.rs`、`tool/browser.rs` 里的 `crate::browser::` 实际解析到 **base 层的 B**。这是一个隐蔽的跨层依赖：上层工具模块经全局重导出依赖下层基础设施，编译期不报错但架构上不透明。
- provider 层的「浏览器传输」以**字符串** `"browser"` 与模型 id `gpt-5.6-pro[web]` 形式穿透到 base 的 `route_builders` / `catalog_routes` / `models`，模型路由层被厂商 web 会话语义污染。

### 1.2 删除后的现状（已核实）
| 层 | 浏览器相关残留 | 结论 |
|---|---|---|
| base | `pub mod browser` 已删；`skill.rs` 的 firefox-browser endorsed 条目已删；provider 模型/路由/web 特判全部同步清除 | **无「名为 browser 实为服务别处」的残留** |
| app-core | `tool/browser.rs`、`tool/browser_tests.rs` 删除；`tool/mod.rs` 注册行删除；`bash.rs` 桥命令重写/BROWSER_SESSION 注入删除；`ui_tools.rs`/`state_ui_storage.rs` 渲染分支删除 | 干净 |
| provider-openai-runtime | `chatgpt_web.rs`、`new_browser_only`、`browser_only` 标志、`is_chatgpt_web_model` 全部删除 | 干净 |
| provider-core | `CHATGPT_WEB_MODEL` 常量删除 | 干净 |

**残留的 browser 引用全部属于 D（系统浏览器语义）或通用词，且未被误删**：
- D：`auth::browser_suppressed` 及其全部调用方（oauth/google/gmail/open/helpers）、`--no-browser` 参数、`open.rs` niri 窗口聚焦（`browser_app_stems` 等）——语义正确，auth/mod.rs 无内容级改动。
- 通用词：`browser_download_url`（GitHub API 字段）、sponsors 的 `"browser-automation"` 发现分类、MCP 注释的 Playwright 示例、「browser-style」UI 措辞等——与已删能力无关。
- 全仓搜索 `firefox-agent-bridge` / `chatgpt_web` / `CHATGPT_WEB` / `gpt-5.6-pro[web]` / `new_browser_only` / `BrowserTool` / `run_browser`：**零残留**。

### 1.3 边界结论
删除后依赖方向恢复为清晰单向：`base（provider/模型目录）→ app-core（工具）→ provider-openai-runtime（传输）`。openai runtime 不再依赖 base 的浏览器子系统，provider-core 不再导出 web 模型常量。**三层边界更清晰，是本次改动最大的架构收益。**

---

## 2. 逐项审查：删除是否引入架构隐患

### 2a. `startup.rs` 的 `Err(_) => return None`
- **运行时行为：优雅，非崩溃。** `register_external_provider_fallible` 的文档明确「返回 None 视为 provider 不可用，而非接线 bug」（external.rs:183-187）。`instantiate_external_provider` 对 None 的调用方（`instantiate_expected_external_provider`）只打 warn 日志。
- **登录后恢复路径保留。** `provider/mod.rs` 的 `handle_auth_changed`（1161-1172 行）在无 OpenAI provider 且 `load_credentials()` 成功时热初始化 OpenAI runtime。因此「先无凭证启动、后 `jcode login`」的升级流不受影响。
- **隐患（中风险）：测试环境依赖回归。** `startup.rs:423` 的测试 `external_provider_runtimes_register_and_instantiate` 无 `lock_test_env()`、无凭证注入，直接 `instantiate_external_provider(OPENAI_RUNTIME).unwrap_or_else(panic)`。改动前 factory 恒返回 Some（`new_browser_only` 回退），测试与机器无关；改动后在无 OpenAI 凭证的 CI/机器上 `load_credentials()` 返回 Err → factory 返回 None → **测试 panic**。建议：注入假凭证（写临时 auth 文件 + `set_active_account_override`），或改为按 `load_credentials().is_ok()` 条件断言注册（而非强制实例化）。
- **过时注释（低）：** `startup.rs:171-173` 注释仍写「注册未带凭证的 runtime 以便浏览器支持的 ChatGPT 模型通过已登录的 Firefox 会话可用」，与现在 `return None` 的代码矛盾，会误导后人。

### 2b. OpenAI 正常传输完整性
- `transport()`：删除 web 分支后返回 `transport_mode`，非 web 模型路径与原逻辑一致。
- `set_transport()`：删除 web 分支后，`auto` / `https|http|sse` / `websocket` 三态映射与非法值报错完整保留。
- `available_transports()`：恒返回 `["auto", "https", "websocket"]`，完整。
- `supports_image_input()` 从 `!is_chatgpt_web_model(..)` 改为恒 `true`：对非 web 模型语义不变，正确。
- `uses_jcode_compaction()` 从 `is_chatgpt_web_model || native != Auto` 收敛为 `native != Auto`，正确。
- `complete()` 不再短路 web 模型，统一走 responses 路径。
- **结论：完整，无遗漏。**

### 2c. provider-core 模型常量自洽性
- `CHATGPT_WEB_MODEL` 常量、`ALL_OPENAI_MODELS` 中条目、`known_openai_model_ids` 的追加逻辑、`openai_static_model_ids`（从 ALL_OPENAI_MODELS 派生）三者同步移除，无悬空引用。
- `catalog_routes.rs` 两条 web 特判、`route_builders.rs` 的 `build_chatgpt_web_route()`、`models.rs` 的 `model_availability_for_account` web 特判全部同步删除。
- 测试同步更新：`catalog_subscription.rs` 断言 `[0..1]`、`commands_tests.rs` 的 route filter 断言从 2 改 3（web 路由 → api-key 路由）、`openai_tests` 用 `new(假凭证)` 替换 `new_browser_only()`。
- `available_models_for_switching()` 终态：缓存目录 + （有 key 时）API-only Pro 模型，干净。
- **结论：自洽。**

### 2d. 工具注册 / TUI 渲染 / 配置的 browser 引用
- **注册表**：`tool/mod.rs` 的 `mod browser;` 与注册行删除；bash.rs 拦截删除（残留一个多余空行，纯 cosmetic）。agent 的工具列表来自 Registry，删除后自动生效，无硬编码名单。
- **TUI 渲染**：`ui_tools.rs` 的 `browser_summary`/`browser_target_summary`/分发分支删除；`state_ui_storage.rs` 的压缩分支删除。两条默认兜底路径已核实安全：
  - 旧 transcript 中的 `browser` 调用，存储压缩默认分支保留 `action` 字段，摘要渲染默认分支显示 action 字符串（如 `open`）。**不崩溃**，但丢失 url/selector 等富信息，属可接受显示退化；且删除 type 掩码分支后旧记录的敏感输入更不可能被渲染出来（默认分支只显示 action）。
  - 注意 `ui_tools.rs` 的 `truncate_url_display` 仍被 webfetch（925 行）与 open（997 行）使用，**未被误删**。
- **配置**：`default_file.rs` 示例注释的 disabled 列表已移除 browser；`ToolConfig::selection()` 对 disabled 名字是纯集合运算，老用户配置 `disabled = ["browser"]` 是**无害空操作**（工具不存在，过滤无效果），无校验报错。
- **结论：无残留引用，兜底路径安全。**

---

## 3. 「外部配置接入」原则达成度

- **达成。** 内嵌浏览器工具（A）、桥基础设施（B，含 GitHub 二进制下载/安装/状态/会话）、chatgpt.com 网页传输（C）全部移除。jcode 不再内嵌驱动任何厂商网页/浏览器，浏览器测试外置到 op-browser skill（Python/Playwright/Chromium），不编译进二进制，与仓库正交。
- 剩余「硬编码」逐项核验：
  - **jcode 订阅**（产品自身）——按任务前提豁免。
  - **静态模型目录**（`ALL_OPENAI_MODELS`/`ALL_CLAUDE_MODELS`/`OPENAI_API_ONLY_PRO_MODELS`、`DEFAULT_*_MODEL`）——这是模型选择器的注册表数据，不是厂商服务嵌入；模型接入仍走外部配置（API key / base URL / OAuth），符合原则。
  - **OpenAI/Anthropic runtime 的端点与 `ORIGINATOR="codex_cli_rs"`**——常规 API 集成，非内嵌能力。
- **结论：原则达成，无残留厂商/能力嵌入。**

---

## 4. 整体稳定性评估

### 编译期
- 已删 API 零残留（全局搜索确认）、Cargo.toml 无浏览器 feature/依赖、provider-core 导出清理干净，理论可一次编译通过。
- **注意**：工作区是「浏览器删除」与先前「B 档：5 个原生登录 runtime 移除」的混合批（`tool/tests.rs` 中 gemini #655 回归测试的删除是 B 档连带，不是浏览器删除），且 HEAD 本身是该重构的中间态（HEAD 的 tests.rs 仍引用已删的 `crate::provider::gemini`）。**需一次全量 cargo check/test 验证。**
- 测试覆盖损失（低-中）：`gemini_build_tools_from_registry_definitions_omits_const_keywords`（#655：const 关键字 / dangling required 导致 Gemini 400 的回归保护）被删且无替代。openai-compatible 的 gemini-api 路径仍存在，建议在别处恢复等价断言。

### 运行期
- 历史 transcript 的 browser 调用：兜底路径安全（见 2d）。
- 登录后 OpenAI 热初始化路径保留（`handle_auth_changed`）。
- 模型选择器/路由不再出现 chatgpt-web 路由，命令测试同步更新。
- **风险点**：2a 的 startup.rs 测试环境依赖（中）；3 处过时注释；`docs/proposals/computer-use-tool.md` 仍以已删 browser 工具为参照（若该提案实施需改写）；`BROWSER_TOOL_REMOVAL_ASSESSMENT.md` 是「只删 A、保留 B/C」时代的评估，与最终「整体删除」决策不一致，建议归档或改写。

---

## 5. 结论与遗留建议

### 结论：架构变好
1. **边界清晰化**：消除了 base 层「名为 browser 的共享桥」跨层隐性耦合与 app-core `crate::browser::` 重导出歧义，三层依赖恢复单向。
2. **厂商/能力耦合下降**：不再内嵌浏览器二进制下载/安装/会话管理与厂商网页传输，符合「内部不依赖厂商、外部配置接入模型、浏览器测试外置」原则。
3. **删除彻底且自洽**：A/B/C 三概念及全部字符串级穿透（模型 id、transport、路由、工具名）同步清除，D 系统浏览器语义零误伤，编译与运行兜底路径均安全。

### 遗留建议（按优先级）
| # | 项 | 优先级 |
|---|---|---|
| 1 | 修复 `startup.rs` 测试 `external_provider_runtimes_register_and_instantiate` 的环境依赖（注入凭证或按 `load_credentials().is_ok()` 条件断言） | 高（CI 稳定性） |
| 2 | 全量跑 guardrails（cargo check/test + fmt + clippy），覆盖整个工作区（含 B 档重构混入的改动） | 高（发布前） |
| 3 | 清理 3 处过时注释：`startup.rs:171-173`、`state_ui_storage.rs:280`（"Gmail/browser rows"）、`computer/mod.rs:3`（"desktop analog of the browser tool"） | 低 |
| 4 | 改写或归档 `docs/proposals/computer-use-tool.md`（以已删工具为参照）与 `BROWSER_TOOL_REMOVAL_ASSESSMENT.md`（与最终决策不一致） | 低 |
| 5 | 评估恢复 gemini #655 的等价回归断言（openai-compatible gemini-api 路径仍存在） | 低 |
| 6 | （可选）清理 bash.rs 删除拦截后遗留的多余空行、agent/turn_loops/mermaid 等 rustfmt 噪音（纯 cosmetic，建议独立提交以便回滚隔离） | 低 |
