# @ 文件引用重构实现计划

日期：2026-08-15
依据规格：`docs/superpowers/specs/2026-08-15-at-file-reference-redesign-design.md`

## 目标

1. 工作区对齐（修复"文件不全"根因）：`ServerEvent::History` 回传 `working_dir`，TUI 写入 `app.session.working_dir`。
2. 引用语义改为 Claude Code 式：发送时保留 `@path`，附加轻量引用清单引导模型 `read`。
3. 选择器打磨：git 感知排序 + 弹窗稳定。

## 依赖顺序

```
T1 协议 → T2 服务器 → T3 TUI 消费（wire 链路）
T4 引用语义（at_file） → T5 引用清单（input）
T6 git 感知（at_file）—— 与 T4 同文件，紧随其后
T7 弹窗打磨 —— 独立
T8 测试汇总 → T9 构建部署验证 → T10 提交推送
```

T4 与 T3 无强依赖（可并行编码，但测试互不干扰）；T6 依赖 T4 完成同文件改造。

---

## Task 1：wire 协议 — History 增加 working_dir 字段

**位置**：`crates/jcode-protocol/src/wire.rs:1084-1182`（`ServerEvent::History`）

**改动**：
- History 变体增加字段 `working_dir: Option<String>`，带 `#[serde(skip_serializing_if = "Option::is_none")]`（对齐附近字段风格，1093-1160 区间）。
- 该字段放`session_id`之后、`messages`之前的顺序性无要求，但放字段组末尾更稳（避免影响现有 positional 构造）。实际使用命名构造。

**验证**：
- `cargo check -p jcode-protocol`
- wire 现有序列化往返测试若存在则补充覆盖 `working_dir: None/Some` 往返。

## Task 2：服务器 — 三个 History 构造点填充 working_dir

**位置**：`crates/jcode-app-core/src/server/client_state.rs`

**改动**：
- 构造点 A（client_state.rs:227，`handle_get_model_catalog`）：`agent_guard.working_dir().map(str::to_string)` 填入。
- 构造点 C（client_state.rs:729，`send_history`）：`agent_guard.working_dir()` 填入。
- 构造点 B（client_state.rs:516，`send_history_from_persisted_session`）：局部 `session.working_dir` 在 500 行 `drop(session)` **之前**捕获到局部变量，再填入。

**验证**：
- `cargo check -p jcode-app-core`
- 构造点 B 注意 borrow 顺序（drop 前 clone）。

## Task 3：TUI — 消费 working_dir 写 app.session.working_dir

**位置**：`crates/jcode-tui/src/tui/app/remote/server_events.rs:1500-1534`（History 解构）、1680-1682（写入 app.session 的既有位置）

**改动**：
- 解构增加 `working_dir`（加入 `..` 之外的显式绑定）。
- 在 1680-1682 附近（`session_changed` 元数据写入区）追加：若 `working_dir` 为 `Some(wd)` 则 `app.session.working_dir = Some(wd)`（仅在实际应用 history 时，即 `should_defer_history_for_runtime_identity` 通过之后）。
- 若 `working_dir` 为 `None`，保持 app.session.working_dir 现状（不覆盖本地已设置值）。

**验证**：
- `cargo check -p jcode-tui`
- 单测：构造 History 事件（含 working_dir）→ 走处理分支 → 断言 `app.session.working_dir` 更新；working_dir=None 时不覆盖。

## Task 4：at_file.rs — 引用语义改为保留 @path（不内联）

**位置**：`crates/jcode-tui/src/tui/app/at_file.rs`

**改动**：
- `expand_at_references`（472-510）+ `expand_at_references_in_root`（479-510）+ `resolve_and_read_reference`（512-520）：**删除内联展开逻辑**，`expand_at_references` 直接返回 `input.to_string()`（保留 `@path` 原样）。删除不再使用的 `resolve_and_read_reference`、`resolve_reference_path`、`MAX_EXPAND_BYTES`（若确无其他引用）。
- `expand_placeholders`（451-455）：仅保留 `expand_paste_markers`（粘贴占位符展开），去掉 `expand_at_references` 调用。
- **新增** `collect_at_references(input: &str) -> Vec<String>`：扫描 `@token`（复用现有 token 切分规则：截至空白或 `]`），返回去重、非粘贴占位符的 token 列表（供 Task 5）。注意过滤 `@[粘贴内容N]` 占位符。
- `workspace_root`（97-104）保持。

**验证（TDD，先 RED）**：
- 更新 `at_file.rs` 内单测：`expand_at_references_reads_workspace_files` → 改为断言 `@note.txt` 保留原样、缺失文件保留、大文件保留。
- 新增 `collect_at_references` 单测：普通 token、粘贴占位符过滤、去重。

## Task 5：input.rs — 发送时附加引用清单

**位置**：`crates/jcode-tui/src/tui/app/input.rs:1312-1323`（`expand_paste_placeholders`）、2581-2595（`take_prepared_input`）

**改动**：
- `expand_paste_placeholders` 内、`expand_placeholders` 之后：收集 `collect_at_references(&raw_input)`；若非空，构造清单段落追加到 expanded 尾部：
  ```
  \n\n（用户引用了以下文件，请用 read 工具读取以获取内容）
  - <token>
  - <token>
  ```
- 清单段落仅在存在引用时附加；不引用时 expanded 与原逻辑一致。

**验证（TDD，先 RED）**：
- 单测：输入含 `@a.rs @b.rs` → expanded 保留 `@a.rs @b.rs` + 尾部含两行清单；无 @ 时无清单。
- 注意 `@[粘贴内容N]` 占位符已由粘贴展开为内容，collect 过滤不受影响。

## Task 6：at_file.rs — git 感知排序

**位置**：`crates/jcode-tui/src/tui/app/at_file.rs`（`scan_workspace_files` 373-378 / `open_file_pick` 121-144）

**改动**：
- 工作区含 `.git` 时执行 `git ls-files`（从工作区根运行，捕获 stdout 相对路径，行数上限沿用 `MAX_INDEX_FILES`）；tracked 文件排在 filtered 列表前部。
- 非 tracked 文件由全扫 `scan_workspace_files` 兜底（append）。
- 失败/无 git：回退现有全扫。
- `FilePickState.entries` 顺序体现 tracked 优先。

**验证（TDD，先 RED）**：
- 单测：git 仓库 fixture（含 tracked + untracked）→ entries 中 tracked 排前。
- 现有 `scan_skips_vcs_and_build_dirs` 兼容。

## Task 7：弹窗打磨

**位置**：`crates/jcode-tui/src/tui/ui_input.rs`（`draw_file_pick_overlay` 213-322）、`crates/jcode-tui/src/tui/ui.rs:3447`（调用点）

**改动**：
- 排查 `command_suggestions_overlay_rect`（弹窗 rect 计算）与输入框区域（chunks[7]）的一致性；修复高度/宽度错位。
- 预览面板与列表分离逻辑（272-287）保持；修正边界（如列表宽度过窄时预览挤压）。
- 稳定高度：候选数变化时 rect 高度稳定（避免跳动）。

**验证**：
- 渲染相关现有测试；人工验证（部署后）。

## Task 8：测试汇总 + 静态检查

- `cargo test -p jcode-protocol`（wire 往返）
- `cargo test -p jcode-tui --lib at_file`（引用语义 + git 感知 + collect）
- `cargo test -p jcode-tui --lib` 相关（input 清单附加）
- `cargo check -p jcode-app-core -p jcode-tui -p jcode-protocol`
- guardrails：`bash scripts/check_guardrails.sh --skip-slow`（fmt/ratchets，注意 test_size_budget.json 更新）

## Task 9：构建 + 部署 + 验证

- `cargo build --profile release-lto --locked -p jcode`（~14min）
- 部署到：`current`、`current-release-lto`、`shared-server`（含 bin launcher）
- 重启 shared-server（`JCODE_DEBUG_CONTROL=1`）
- 验证：服务器 History 事件含 working_dir（debug socket / 日志）；@ 扫描根正确（TUI 人工验证，用户侧）
- 说明：wire 协议改动要求 TUI 与服务器同版本部署（同机部署满足）

## Task 10：提交 + 推送

- commit（改动文件：wire.rs、client_state.rs、server_events.rs、at_file.rs、input.rs、相关测试、预算文件）
- push origin master（先前 9bac95b73/6d5618bcf 未推送，网络恢复后一并推送）

## 风险

- wire 协议改动：TUI/服务器版本耦合（同机部署缓解；若远端旧 TUI 连接新服务器，History 新增字段带 serde default/option 容错）。
- 引用清单依赖模型自觉 read：提示措辞需清晰；备选为小文件内联（本次不做）。
- `git ls-files` 执行开销：限制输出、超时/失败回退。
