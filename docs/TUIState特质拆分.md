# TuiState 特质拆分计划

状态：分析 + 提议计划

本文档审计 `TuiState` 特质（`crates/jcode-tui/src/tui/mod.rs`）并提出安全、增量的拆分。它是 `App` 上帝对象拆分的 Phase 1.5 后续（见 `客户端核心展示拆分计划.md`）。

## 当前状态

- `pub trait TuiState` 暴露 **114 个方法**。
- 实现者：2 个（`tui/app/tui_state.rs` 中的 `App` 和 `tui/ui_tests/mod.rs` 中的 `TestState`）。
- 消费者：29 个文件中约 95 处使用，几乎都是 `&dyn TuiState`（50 个渲染函数签名接受 `app: &dyn TuiState`）。

它是 `App` 上帝对象的展示层对应物：一个把每个渲染模块耦合到整个客户端表面的宽接口。

## 为什么朴素的子特质拆分价值有限

两个结构事实约束了这次重构：

1. **`App` 无论如何都实现整个表面。** 把 `TuiState` 拆成 `TuiTranscriptState + TuiInputState + ...` 不会减少 `App` 必须实现的东西，而且（因为该特质只是展示层数据访问）不改变 crate 级编译耦合。收益是意图/可导航性，不是 `App` 的解耦。

2. **`&dyn TuiState` 不组合。** 渲染函数接受特质对象。Rust 没有稳定的 `&dyn (A + B)`，因此任何需要多个领域方法的消费者必须接受一个重新聚合它们的超特质。两个中央渲染器（`ui.rs`、`ui_viewport.rs`）使用几乎每个领域的方法，因此它们会保留完整超特质约束。

已测量：约 28 个 `&dyn TuiState` 渲染模块中只有 **2** 个是多类别的（`ui.rs`、`ui_viewport.rs`）；其他约 26 个各用单一领域。所以子特质拆分*确实*收窄了大多数渲染模块的声明面，但头条上帝接口（由 2 个中央渲染器驱动）通过超特质保持宽。

结论：拆分对可读性和收窄叶子渲染模块约束值得，但它**不是**编译耦合收益，应增量进行，避免在 29 个文件上产生高冲突的大爆炸。

## 提议的目标形态

```
trait TuiState:
    TuiTranscriptState + TuiInputState + TuiScrollState + TuiStreamStatusState
    + TuiProviderState + TuiSessionServerState + TuiWorkspaceState
    + TuiDiagramPaneState + TuiDiffPaneState + TuiSidePanelState
    + TuiInlineState + TuiOverlayState + TuiCopySelectionState
    + TuiOnboardingState + TuiMiscState
{}
```

`App` 和 `TestState` 每个子特质保留一个 `impl`（机械移动）。2 个中央渲染器接受 `&dyn TuiState`（超特质）。每个叶子渲染模块收窄到它需要的那个子特质。

## 方法分类（全部 114 个）

### TuiTranscriptState
display_messages, display_user_message_count, compacted_hidden_user_prompts,
has_display_edit_tool_messages, side_pane_images, display_messages_version,
render_streaming_markdown

### TuiInputState
input, cursor_pos, queued_messages, interleave_message,
pending_soft_interrupts, has_stashed_input, command_suggestions,
command_suggestion_selected, suggestion_prompts, queue_mode,
next_prompt_new_session_armed, dictation_key_label

### TuiScrollState
scroll_offset, auto_scroll_paused, chat_overscroll_active,
copy_selection_edge_autoscroll_active, chat_native_scrollbar,
has_pending_mouse_scroll_animation

### TuiStreamStatusState
streaming_text, is_processing, streaming_tokens, streaming_cache_tokens,
output_tps, streaming_tool_calls, elapsed, status, active_skill,
subagent_status, batch_progress, time_since_activity, stream_message_ended,
status_notice, status_detail, rate_limit_remaining, animation_elapsed

### TuiProviderState
provider_name, provider_model, upstream_provider, connection_type,
mcp_servers, available_skills, auth_status, update_cost,
total_session_tokens, session_compaction_count, context_info,
context_snapshot, context_limit, cache_ttl_status

### TuiSessionServerState
is_remote_mode, is_canary, is_replay, current_session_id,
session_display_name, server_display_name, server_display_icon,
server_sessions, connected_clients, remote_startup_phase_active,
client_update_available, server_update_available, info_widget_data,
active_experimental_feature_notice

### TuiWorkspaceState
workspace_mode_enabled, workspace_map_rows, workspace_animation_tick

### TuiDiagramPaneState
diagram_mode, diagram_focus, diagram_index, diagram_scroll,
diagram_pane_ratio, diagram_pane_ratio_user_adjusted, diagram_pane_animating,
diagram_pane_enabled, diagram_pane_position, diagram_zoom

### TuiDiffPaneState
diff_mode, diff_pane_scroll, diff_pane_scroll_x, diff_pane_focus,
diff_line_wrap

### TuiSidePanelState
side_panel, side_panel_image_zoom_percent, side_panel_native_scrollbar,
pin_images, pinned_images_auto_hide_remaining_secs

### TuiInlineState
inline_interactive_state, inline_view_state, inline_ui_state

### TuiOverlayState
changelog_scroll, help_scroll, model_status_overlay, session_picker_overlay,
login_picker_overlay, account_picker_overlay, usage_overlay

### TuiCopySelectionState
copy_badge_ui, copy_selection_mode, copy_selection_range,
copy_selection_status

### TuiOnboardingState
onboarding_preview_mode, onboarding_welcome_active, onboarding_welcome_kind

### TuiMiscState
working_dir, now_millis, has_notification, centered_mode

## 增量、低冲突迁移

**不要**在 29 个文件上一次性拆分全部 15 个子特质。推荐顺序：

1. 在特质定义中落地文档化节标题（已完成；纯注释，单文件）。给分类一个规范家。
2. 提取一个带单文件消费者的叶子子特质作为模式证明（如 `TuiCopySelectionState` 或 `TuiDiagramPaneState`）。用 `cargo check -p jcode-tui` 验证。
3. 每次提交提取一个剩余叶子子特质，同一提交收窄对应叶子渲染模块的约束。
4. 全程保持 `ui.rs` 和 `ui_viewport.rs` 在 `TuiState` 超特质上。

每一步都保持行为（只有数据访问器）且独立编译，因此可以在其他智能体的工作之间合并而不会有大爆炸冲突。

## 验证

- 每次子特质提取后 `cargo check -p jcode-tui`（TMPDIR 必须指向真实磁盘而不是 RAM 支持的 tmpfs，否则 ring/aws-lc-sys 构建脚本会报"Disk quota exceeded"）。
- 结束时 `cargo test -p jcode-tui --lib` 一次。注意：lib 测试套件有预先存在的、与并行顺序相关的 flaky 失败，与本特质无关（用 `--test-threads=1` 单独验证任何失败测试）。
