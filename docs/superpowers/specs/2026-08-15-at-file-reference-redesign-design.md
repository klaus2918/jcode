# @ 文件引用重构设计（Claude Code 式）

日期：2026-08-15
状态：已获用户确认（方案 A）

## 背景与问题

当前 `@` 文件引用（`crates/jcode-tui/src/tui/app/at_file.rs`，提交 58c17ddb7）存在两类问题：

1. **选择器异常**：用户实测反馈弹窗"文件不全"（列表缺失、疑似扫错目录）、过滤/预览/确认/弹窗样式均不理想。
2. **交互不符合预期**：当前实现是**提交时内联展开**文件内容（`expand_at_references` 把 `@path` 替换为 `<@path>\n{文件内容}`），用户期望 **Claude Code 式**：保留 `@路径` 引用，模型通过 `read` 工具自行读取，不内联内容、不占上下文。

### 根因（已探明）

- **工作区不一致**：remote 模式下 TUI 的 `app.session.working_dir` 为 `None`（`Session::create(None, None)`，tui_lifecycle.rs:1258-1261），`workspace_root()`（at_file.rs:97-104）fallback 到 TUI 进程 CWD；而模型 `read` 工具的 base 是**服务器** `session.working_dir`（由 Subscribe 绑定）。两者可能不一致 → @ 扫错目录 → 列表不全、引用路径模型读不到。
- 服务器推送的 `ServerEvent::History`（jcode-protocol/src/wire.rs:1086-1145）不含 `working_dir` 字段，TUI 无从获知服务器实际工作区。

### 已具备的能力（可复用）

- 模型内置 `read` 工具，在服务器端执行，`ToolContext.working_dir` 来自 `session.working_dir`（crates/jcode-app-core/src/tool/read.rs、turn_streaming_mpsc.rs:840）。
- TUI 已计算订阅 working_dir（`subscribe_metadata`，tui/mod.rs:1307-1340：`--dir` > 进程 CWD）。
- `@` 选择器已具备 fzf 过滤（basename 优先）、右栏预览、Ctrl+N/P/↑↓ 导航、Tab/Enter 确认、Esc 取消。
- 用户本机部署 shared-server，TUI 与服务器同机，工作区路径一致可读。

## 设计

### 1. 工作区对齐（修复"文件不全"根因）

- **协议**：`ServerEvent::History` 增加 `working_dir: Option<String>` 字段（jcode-protocol/src/wire.rs），服务器推送历史时填充 `session.working_dir`（crates/jcode-app-core 侧）。
- **TUI**：收到后写 `app.session.working_dir = Some(wd)`（remote 下此前为 None）。
- `workspace_root()`（at_file.rs:97-104）保持"优先 `app.session.working_dir`，兜底进程 CWD"，因字段已对齐 → 扫描根 = 模型 read base。

### 2. 引用语义（保留 @路径 + 模型 Read）

- 选中插入 `@相对路径`（不变）。
- **发送时不再内联展开文件内容**：`expand_at_references`（at_file.rs:472-510）改为**保留 `@path` 文本**；仅展开 `@[粘贴内容N]` 粘贴占位符（`expand_paste_markers` 保留）。
- 发送前（`take_prepared_input`，input.rs:2581-2595）**收集**输入中的 `@path` token，在发送文本尾部附加**轻量引用清单**（仅路径、不占内容），提示模型用 `read` 读取：
  ```
  （用户引用了以下文件，请用 read 工具读取以获取内容）
  - src/main.rs
  - docs/x.md
  ```
- 缺失/超限/不可读文件：保留 `@path` 文本，模型 `read` 失败自行处理。
- **引用清单不走协议**：清单作为普通 user 消息文本尾部（沿用现有"输入框显示标记、发送时转换"的模式，如粘贴占位符）。注意这与第 1 节的 `History.working_dir` 字段是两处独立改动：前者仅影响消息文本，后者是协议字段新增。

### 3. 选择器打磨

- **git 感知**：工作区是 git 仓库时优先 `git ls-files` 的 tracked 文件（排前），非 git 或文件不在 tracked 时全扫兜底（`scan_workspace_files` 保留）——避免大仓库 `MAX_INDEX_FILES=3000` 截断。
- 过滤：保留现有 fzf basename 优先逻辑（列表修复后自然生效）。
- 预览：保留现有右栏预览（at_file.rs:198-231、ui_input.rs:269-287）。
- 弹窗：稳定高度、跟随输入框（`command_suggestions_overlay_rect`），排查并修复错位。

### 4. 边界与错误处理

- `@path` 不存在/不可读/超限：保留文本，不阻塞发送。
- 远程 shared-server（本机部署）：TUI 与服务器同机，working_dir 对齐后引用可读。
- 图片引用不在本次范围（沿用粘贴图片）。
- 非 git 工作区：全扫兜底（保持现状行为）。

### 5. 测试

- 工作区对齐：wire 回传字段 + TUI 消费（`app.session.working_dir` 更新）。
- 引用语义：`expand_placeholders` 保留 `@path`、仅展开粘贴占位符；引用清单附加正确。
- 选择器：过滤/预览/确认回归（现有 at_file_references.rs 测试更新）。
- git 感知：tracked 优先排序测试。

## 范围外（后续可做）

- `@目录` 引用（方案 C）
- `@前文` 历史引用（方案 C）
- 引用路径在消息气泡中的语法高亮

## 改动文件预估

- `crates/jcode-protocol/src/wire.rs`（History 加 working_dir 字段）
- `crates/jcode-app-core/src/server/*`（History 事件填充 working_dir）
- `crates/jcode-tui/src/tui/app/at_file.rs`（引用语义 + git 感知 + workspace_root 对齐消费）
- `crates/jcode-tui/src/tui/app/input.rs`（引用清单附加）
- `crates/jcode-tui/src/tui/*`（working_dir 事件消费）
- 相关测试文件