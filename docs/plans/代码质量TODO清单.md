# 代码质量计划待办清单

本文件跟踪 `docs/plans/代码质量10-10计划.md` 中描述的代码质量提升计划的执行积压。

状态取值：

- `pending`（待处理）
- `in_progress`（进行中）
- `blocked`（受阻）
- `done`（已完成）

## 阶段 0：防止进一步恶化

- [x] 为 `cargo check --all-targets --all-features` 添加 CI 任务
- [x] 为 `cargo clippy --all-targets --all-features -- -D warnings` 添加 CI 任务
- [x] 保持警告策略为向下收紧的棘轮
- [x] 在贡献者指南中添加有据可查的文件大小和函数大小目标

## 阶段 1：警告与死代码清除

- [x] 盘点所有 `#![allow(dead_code)]` 位置并论证或移除它们
- [x] 将基线警告数量从当前水平显著降低
- [ ] 移除 `setup_hints.rs` 中过时的未使用函数
- [ ] 移除 TUI 支持模块中过时的未使用代码
- [ ] 审计宽泛的抑制并替换为窄范围的局部允许

## 阶段 2：拆分最大的文件

### 最高优先级
- [x] 按功能领域拆分 `tests/e2e/main.rs`
  - 2026-03-24 开始：抽取功能模块 `session_flow`、`transport`、`provider_behavior`、`binary_integration`、`safety` 和 `ambient`
  - 2026-03-24 完成：将共享辅助代码抽取到 `tests/e2e/test_support/mod.rs`
  - 2026-05-18 验证：`tests/e2e/main.rs` 现在只是功能模块入口点，完整的非忽略 e2e 目标通过了 44/44 个测试。
- [ ] 继续将 `src/server.rs` 拆分为聚焦子模块（[#53](https://github.com/1jehuang/jcode/issues/53)）
  - 2026-03-24 进展：将共享服务器/集群状态抽取到 `src/server/state.rs`
  - 2026-03-24 进展：将套接字/引导辅助代码抽取到 `src/server/socket.rs`
  - 2026-03-24 进展：将重载标记/信号状态抽取到 `src/server/reload_state.rs`
  - 2026-03-24 进展：将路径/更新/集群身份工具抽取到 `src/server/util.rs`
- [ ] 将 `src/agent.rs` 拆分为编排、流、中断和工具执行模块

### 下一波
- [ ] 将 `src/provider/mod.rs` 拆分为特质、定价、路由和共享 HTTP 辅助模块（[#52](https://github.com/1jehuang/jcode/issues/52)）
- [ ] 将 `src/provider/openai.rs` 拆分为请求、流、工具和响应模块（[#52](https://github.com/1jehuang/jcode/issues/52)）
- [ ] 按渲染职责拆分 `src/tui/ui.rs`（[#51](https://github.com/1jehuang/jcode/issues/51)）
- [ ] 按组件/领域分区拆分 `src/tui/info_widget.rs`（[#51](https://github.com/1jehuang/jcode/issues/51)）

## 阶段 3：错误处理加固

- [ ] 将生产环境的 `unwrap` / `expect` 数量与仅测试使用分开统计
- [ ] 将容易处理的生产 `unwrap` / `expect` 热点替换为显式错误
- [ ] 为提供商流解析失败添加更好的错误上下文
- [ ] 为重载和套接字生命周期失败添加更好的错误上下文（[#53](https://github.com/1jehuang/jcode/issues/53)）

## 阶段 4：测试策略改进

- [ ] 抽取共享的 e2e 测试支持辅助代码
- [ ] 为重载状态转换添加聚焦测试
- [ ] 为格式错误的提供商流分块添加聚焦测试
- [ ] 为稳定的 TUI 渲染输出添加快照或黄金文件测试
- [ ] 为协议序列化和工具解析添加属性测试

## 阶段 5：可靠性与性能护栏

- [ ] 添加重复重载可靠性测试覆盖
- [ ] 添加重复附加/分离和重连覆盖
- [ ] 在有据可查的预算中跟踪内存回归预期
- [ ] 改善重载、集群和工具执行路径的可观测性
- [ ] 执行 `docs/plans/编译性能计划.md` 中的编译性能路线图
- [ ] 为热/冷自研循环添加可重复的编译计时检查点

## 立即进行的活跃工作

- [x] 落地质量计划文档
- [x] 落地本待办清单
- [x] 收紧 CI 护栏
- [ ] 开始第一个高 ROI 的清理或拆分
  - 后续跟踪问题：#51、#52、#53、#54

## 综合审计积压（2026-04-18）

由 `docs/audits/代码质量审计2026-04-18.md` 生成。本节枚举审计发现的完整文件级积压，以便待办清单覆盖所有当前热点。

### 审计快照

- [x] 发布综合审计报告（`50` 个生产文件 >1200 LOC，`62` 个生产文件 801-1200 LOC，`304` 个生产函数 >100 LOC，分布于 `165` 个文件）
- [ ] 在每次重大清理波次后刷新此审计积压

### 结构性积压：超过 1200 LOC 的生产文件

- [ ] 拆分 `src/server/comm_control.rs`（3228 LOC）
- [ ] 拆分 `src/tool/communicate.rs`（3165 LOC）
- [ ] 拆分 `src/session.rs`（2729 LOC）
- [ ] 拆分 `src/server/client_lifecycle.rs`（2704 LOC）
- [ ] 拆分 `src/provider/openai.rs`（2683 LOC）
- [ ] 拆分 `src/tui/ui.rs`（2437 LOC）
- [ ] 拆分 `src/memory.rs`（2397 LOC）
- [ ] 拆分 `src/provider/mod.rs`（2365 LOC）
- [ ] 拆分 `src/telemetry.rs`（2217 LOC）
- [ ] 拆分 `src/tui/ui_messages.rs`（2131 LOC）
- [ ] 拆分 `src/tui/session_picker.rs`（2115 LOC）
- [ ] 拆分 `src/tui/app/inline_interactive.rs`（2041 LOC）
- [ ] 拆分 `src/tui/app/input.rs`（2023 LOC）
- [ ] 拆分 `src/config.rs`（2005 LOC）
- [ ] 拆分 `src/provider/anthropic.rs`（1969 LOC）
- [ ] 拆分 `src/tui/app/remote/key_handling.rs`（1919 LOC）
- [ ] 拆分 `src/tui/app/auth.rs`（1912 LOC）
- [ ] 拆分 `src/usage.rs`（1900 LOC）
- [ ] 拆分 `src/tui/session_picker/loading.rs`（1888 LOC）
- [ ] 拆分 `src/cli/login.rs`（1881 LOC）
- [ ] 拆分 `src/replay.rs`（1794 LOC）
- [ ] 拆分 `src/cli/provider_init.rs`（1769 LOC）
- [ ] 拆分 `src/bin/tui_bench.rs`（1738 LOC）
- [ ] 拆分 `src/compaction.rs`（1718 LOC）
- [ ] 拆分 `src/tui/ui_prepare.rs`（1708 LOC）
- [ ] 拆分 `src/memory_agent.rs`（1696 LOC）
- [ ] 拆分 `src/tui/info_widget.rs`（1688 LOC）
- [ ] 拆分 `src/tui/ui_pinned.rs`（1678 LOC）
- [ ] 拆分 `src/cli/tui_launch.rs`（1670 LOC）
- [ ] 拆分 `src/tui/app/commands.rs`（1630 LOC）
- [ ] 拆分 `src/auth/mod.rs`（1607 LOC）
- [ ] 拆分 `src/tui/ui_input.rs`（1572 LOC）
- [ ] 拆分 `src/server.rs`（1559 LOC）
- [ ] 拆分 `src/tui/app/helpers.rs`（1551 LOC）
- [ ] 拆分 `src/tool/agentgrep.rs`（1516 LOC）
- [ ] 拆分 `src/import.rs`（1504 LOC）
- [ ] 拆分 `src/ambient.rs`（1496 LOC）
- [ ] 拆分 `src/server/swarm.rs`（1491 LOC）
- [ ] 拆分 `src/tui/ui_tools.rs`（1446 LOC）
- [ ] 拆分 `src/tui/markdown.rs`（1375 LOC）
- [ ] 拆分 `src/protocol.rs`（1362 LOC）
- [ ] 拆分 `src/tool/ambient.rs`（1341 LOC）
- [ ] 拆分 `src/auth/oauth.rs`（1308 LOC）
- [ ] 拆分 `src/tui/app/remote.rs`（1300 LOC）
- [ ] 拆分 `src/tui/app/turn.rs`（1292 LOC）
- [ ] 拆分 `src/provider/models.rs`（1263 LOC）
- [ ] 拆分 `src/server/client_actions.rs`（1257 LOC）
- [ ] 拆分 `src/tui/app/model_context.rs`（1211 LOC）
- [ ] 拆分 `src/tui/app/tui_state.rs`（1210 LOC）
- [ ] 拆分 `src/provider/gemini.rs`（1202 LOC）

### 结构性积压：801 到 1200 LOC 之间的生产文件

- [ ] 将 `src/video_export.rs` 降到 800 LOC 以下（今天 1195 LOC）
- [ ] 将 `src/tui/app/auth_account_picker.rs` 降到 800 LOC 以下（今天 1192 LOC）
- [ ] 将 `src/tui/mod.rs` 降到 800 LOC 以下（今天 1167 LOC）
- [ ] 将 `src/provider/copilot.rs` 降到 800 LOC 以下（今天 1155 LOC）
- [ ] 将 `src/tui/app/state_ui.rs` 降到 800 LOC 以下（今天 1150 LOC）
- [ ] 将 `src/tool/browser.rs` 降到 800 LOC 以下（今天 1144 LOC）
- [ ] 将 `src/provider/claude.rs` 降到 800 LOC 以下（今天 1142 LOC）
- [ ] 将 `src/provider/openrouter.rs` 降到 800 LOC 以下（今天 1132 LOC）
- [ ] 将 `src/tui/app/remote/server_events.rs` 降到 800 LOC 以下（今天 1125 LOC）
- [ ] 将 `src/tui/app/debug_bench.rs` 降到 800 LOC 以下（今天 1124 LOC）
- [ ] 将 `src/tui/mermaid.rs` 降到 800 LOC 以下（今天 1116 LOC）
- [ ] 将 `src/update.rs` 降到 800 LOC 以下（今天 1109 LOC）
- [ ] 将 `src/server/client_session.rs` 降到 800 LOC 以下（今天 1094 LOC）
- [ ] 将 `src/provider/openai_stream_runtime.rs` 降到 800 LOC 以下（今天 1093 LOC）
- [ ] 将 `src/tool/mod.rs` 降到 800 LOC 以下（今天 1087 LOC）
- [ ] 将 `src/tui/app/state_ui_input_helpers.rs` 降到 800 LOC 以下（今天 1075 LOC）
- [ ] 将 `src/server/comm_session.rs` 降到 800 LOC 以下（今天 1071 LOC）
- [ ] 将 `src/ambient/runner.rs` 降到 800 LOC 以下（今天 1057 LOC）
- [ ] 将 `src/provider/cursor.rs` 降到 800 LOC 以下（今天 1043 LOC）
- [ ] 将 `src/cli/commands.rs` 降到 800 LOC 以下（今天 1039 LOC）
- [ ] 将 `src/server/debug.rs` 降到 800 LOC 以下（今天 1038 LOC）
- [ ] 将 `src/message.rs` 降到 800 LOC 以下（今天 1038 LOC）
- [ ] 将 `src/tui/app/commands_review.rs` 降到 800 LOC 以下（今天 1037 LOC）
- [ ] 将 `src/tui/app/navigation.rs` 降到 800 LOC 以下（今天 1014 LOC）
- [ ] 将 `src/tui/account_picker.rs` 降到 800 LOC 以下（今天 1012 LOC）
- [ ] 将 `src/goal.rs` 降到 800 LOC 以下（今天 995 LOC）
- [ ] 将 `src/memory_graph.rs` 降到 800 LOC 以下（今天 980 LOC）
- [ ] 将 `src/tui/markdown_render_full.rs` 降到 800 LOC 以下（今天 979 LOC）
- [ ] 将 `src/auth/claude.rs` 降到 800 LOC 以下（今天 976 LOC）
- [ ] 将 `src/auth/cursor.rs` 降到 800 LOC 以下（今天 970 LOC）
- [ ] 将 `src/browser.rs` 降到 800 LOC 以下（今天 958 LOC）
- [ ] 将 `src/runtime_memory_log.rs` 降到 800 LOC 以下（今天 956 LOC）
- [ ] 将 `src/agent/turn_streaming_mpsc.rs` 降到 800 LOC 以下（今天 945 LOC）
- [ ] 将 `src/cli/dispatch.rs` 降到 800 LOC 以下（今天 929 LOC）
- [ ] 将 `src/tui/ui_animations.rs` 降到 800 LOC 以下（今天 925 LOC）
- [ ] 将 `src/tui/app/auth_account_commands.rs` 降到 800 LOC 以下（今天 923 LOC）
- [ ] 将 `src/tui/test_harness.rs` 降到 800 LOC 以下（今天 918 LOC）
- [ ] 将 `src/auth/codex.rs` 降到 800 LOC 以下（今天 911 LOC）
- [ ] 将 `src/tui/keybind.rs` 降到 800 LOC 以下（今天 902 LOC）
- [ ] 将 `src/tui/ui_inline_interactive.rs` 降到 800 LOC 以下（今天 900 LOC）
- [ ] 将 `src/tui/ui_header.rs` 降到 800 LOC 以下（今天 897 LOC）
- [ ] 将 `src/server/state.rs` 降到 800 LOC 以下（今天 895 LOC）
- [ ] 将 `src/build.rs` 降到 800 LOC 以下（今天 892 LOC）
- [ ] 将 `src/tui/backend.rs` 降到 800 LOC 以下（今天 881 LOC）
- [ ] 将 `src/tui/login_picker.rs` 降到 800 LOC 以下（今天 878 LOC）
- [ ] 将 `src/sidecar.rs` 降到 800 LOC 以下（今天 872 LOC）
- [ ] 将 `src/tui/app/tui_lifecycle.rs` 降到 800 LOC 以下（今天 868 LOC）
- [ ] 将 `src/tui/permissions.rs` 降到 800 LOC 以下（今天 865 LOC）
- [ ] 将 `src/tui/markdown_render_lazy.rs` 降到 800 LOC 以下（今天 865 LOC）
- [ ] 将 `src/gateway.rs` 降到 800 LOC 以下（今天 863 LOC）
- [ ] 将 `src/tool/read.rs` 降到 800 LOC 以下（今天 862 LOC）
- [ ] 将 `src/provider/antigravity.rs` 降到 800 LOC 以下（今天 860 LOC）
- [ ] 将 `src/tool/apply_patch.rs` 降到 800 LOC 以下（今天 859 LOC）
- [ ] 将 `src/tool/bash.rs` 降到 800 LOC 以下（今天 858 LOC）
- [ ] 将 `src/auth/gemini.rs` 降到 800 LOC 以下（今天 849 LOC）
- [ ] 将 `src/tui/visual_debug.rs` 降到 800 LOC 以下（今天 847 LOC）
- [ ] 将 `src/setup_hints.rs` 降到 800 LOC 以下（今天 827 LOC）
- [ ] 将 `src/server/reload.rs` 降到 800 LOC 以下（今天 826 LOC）
- [ ] 将 `src/auth/copilot.rs` 降到 800 LOC 以下（今天 815 LOC）
- [ ] 将 `src/tui/app.rs` 降到 800 LOC 以下（今天 812 LOC）
- [ ] 将 `src/tui/app/remote/reconnect.rs` 降到 800 LOC 以下（今天 804 LOC）
- [ ] 将 `src/server/debug_swarm_read.rs` 降到 800 LOC 以下（今天 803 LOC）

### 测试集中度积压：超过 1200 LOC 的测试文件

- [x] 拆分测试热点 `src/tui/app/tests.rs`（原 13615 LOC；拆分为聚焦的 `src/tui/app/tests/*.rs` 包含模块）
- [x] 拆分测试热点 `src/server/client_session_tests/resume.rs`（原 1263 LOC；拆分为聚焦的 `src/server/client_session_tests/resume/*.rs` 包含模块）
- [x] 拆分测试热点 `src/provider/tests.rs`（原 1252 LOC；拆分为聚焦的 `src/provider/tests/*.rs` 包含模块）
- [x] 拆分测试热点 `src/cli/auth_test.rs`（原 1226 LOC；拆分为聚焦的 `src/cli/auth_test/*.rs` 包含模块）

### 超大文件之外的超长函数积压

- [ ] 分解 `src/server/client_comm.rs` 中 >100 LOC 的函数（4 个超大函数）
- [ ] 分解 `src/tui/app/debug_profile.rs` 中 >100 LOC 的函数（3 个超大函数）
- [ ] 分解 `src/server/comm_plan.rs` 中 >100 LOC 的函数（3 个超大函数）
- [ ] 分解 `src/tui/ui_file_diff.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/session_picker/render.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/mermaid_widget.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/mermaid_cache_render.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/info_widget_todos.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/info_widget_model.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/app/tui_lifecycle_runtime.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tool/selfdev/build_queue.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/server/debug_server_state.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/server/client_state.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/provider/dispatch.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/background.rs` 中 >100 LOC 的函数（2 个超大函数）
- [ ] 分解 `src/tui/ui_viewport.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/ui_overlays.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/ui_memory.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/ui_diagram_pane.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/session_picker/filter.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/mermaid_viewport.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/mermaid_debug.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/memory_profile.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/markdown_wrap.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/markdown_render_support.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/info_widget_layout.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/state_ui_storage.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/state_ui_maintenance.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/runtime_memory.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/run_shell.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/dictation.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/debug_script.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/debug_cmds.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tui/app/debug.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/task.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/session_search.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/selfdev/status.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/selfdev/reload.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/memory.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/grep.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/goal.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/gmail.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/conversation_search.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/bg.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/tool/batch.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/setup_hints/windows_setup.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/swarm_persistence.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/reload_state.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/headless.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/debug_swarm_write.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/debug_session_admin.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/debug_jobs.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/debug_help.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/debug_command_exec.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/debug_ambient.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/comm_await.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/client_disconnect_cleanup.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/client_comm_message.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/server/client_comm_context.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/provider/startup.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/provider/openrouter_sse_stream.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/provider/openrouter_provider_impl.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/provider/openai_request.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/provider/openai_provider_impl.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/provider/cli_common.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/memory_log.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/mcp/client.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/cli/selfdev.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/cli/hot_exec.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/catchup.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/bin/harness.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/agent/turn_streaming_broadcast.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/agent/turn_loops.rs` 中 >100 LOC 的函数（1 个超大函数）
- [ ] 分解 `src/agent/response_recovery.rs` 中 >100 LOC 的函数（1 个超大函数）

### 失败路径加固积压：含 panic 倾向调用的生产文件

- [ ] 加固 `src/tool/communicate.rs`（`unwrap`: 0, `expect`: 136, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 136）
- [ ] 加固 `src/build.rs`（`unwrap`: 9, `expect`: 53, `panic!`: 2, `todo!`: 0, `unimplemented!`: 0, 总计: 64）
- [ ] 加固 `src/provider/openai.rs`（`unwrap`: 7, `expect`: 38, `panic!`: 9, `todo!`: 0, `unimplemented!`: 0, 总计: 54）
- [ ] 加固 `src/auth/cursor.rs`（`unwrap`: 48, `expect`: 4, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 52）
- [ ] 加固 `src/auth/codex.rs`（`unwrap`: 45, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 46）
- [ ] 加固 `src/server/comm_control.rs`（`unwrap`: 0, `expect`: 30, `panic!`: 11, `todo!`: 0, `unimplemented!`: 0, 总计: 41）
- [ ] 加固 `src/cli/args.rs`（`unwrap`: 24, `expect`: 0, `panic!`: 16, `todo!`: 0, `unimplemented!`: 0, 总计: 40）
- [ ] 加固 `src/auth/claude.rs`（`unwrap`: 28, `expect`: 9, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 37）
- [ ] 加固 `src/cli/dispatch.rs`（`unwrap`: 0, `expect`: 28, `panic!`: 2, `todo!`: 0, `unimplemented!`: 0, 总计: 30）
- [ ] 加固 `src/tool/bash.rs`（`unwrap`: 7, `expect`: 21, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 28）
- [ ] 加固 `src/storage.rs`（`unwrap`: 0, `expect`: 26, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 26）
- [ ] 加固 `src/tui/session_picker/loading.rs`（`unwrap`: 0, `expect`: 25, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 25）
- [ ] 加固 `src/tool/read.rs`（`unwrap`: 0, `expect`: 25, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 25）
- [ ] 加固 `src/auth/gemini.rs`（`unwrap`: 4, `expect`: 21, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 25）
- [ ] 加固 `src/tool/apply_patch.rs`（`unwrap`: 15, `expect`: 1, `panic!`: 8, `todo!`: 0, `unimplemented!`: 0, 总计: 24）
- [ ] 加固 `src/side_panel.rs`（`unwrap`: 0, `expect`: 24, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 24）
- [ ] 加固 `src/server/client_comm.rs`（`unwrap`: 0, `expect`: 12, `panic!`: 11, `todo!`: 0, `unimplemented!`: 1, 总计: 24）
- [ ] 加固 `src/server/reload.rs`（`unwrap`: 0, `expect`: 23, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 23）
- [ ] 加固 `src/tui/session_picker.rs`（`unwrap`: 7, `expect`: 13, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 21）
- [ ] 加固 `src/server/debug.rs`（`unwrap`: 0, `expect`: 18, `panic!`: 2, `todo!`: 0, `unimplemented!`: 1, 总计: 21）
- [ ] 加固 `src/tool/goal.rs`（`unwrap`: 0, `expect`: 19, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 20）
- [ ] 加固 `src/server/comm_session.rs`（`unwrap`: 0, `expect`: 20, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 20）
- [ ] 加固 `src/cli/tui_launch.rs`（`unwrap`: 0, `expect`: 18, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 19）
- [ ] 加固 `src/auth/external.rs`（`unwrap`: 19, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 19）
- [ ] 加固 `src/provider/gemini.rs`（`unwrap`: 7, `expect`: 10, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 18）
- [ ] 加固 `src/restart_snapshot.rs`（`unwrap`: 0, `expect`: 17, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 17）
- [ ] 加固 `src/server/client_state.rs`（`unwrap`: 0, `expect`: 14, `panic!`: 1, `todo!`: 0, `unimplemented!`: 1, 总计: 16）
- [ ] 加固 `src/replay.rs`（`unwrap`: 11, `expect`: 2, `panic!`: 3, `todo!`: 0, `unimplemented!`: 0, 总计: 16）
- [ ] 加固 `src/goal.rs`（`unwrap`: 0, `expect`: 16, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 16）
- [ ] 加固 `src/server/client_actions.rs`（`unwrap`: 3, `expect`: 9, `panic!`: 2, `todo!`: 0, `unimplemented!`: 1, 总计: 15）
- [ ] 加固 `src/tui/app/remote.rs`（`unwrap`: 0, `expect`: 13, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 14）
- [ ] 加固 `src/memory_graph.rs`（`unwrap`: 12, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 14）
- [ ] 加固 `src/mcp/protocol.rs`（`unwrap`: 11, `expect`: 2, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 14）
- [ ] 加固 `src/cli/selfdev.rs`（`unwrap`: 1, `expect`: 12, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 14）
- [ ] 加固 `src/setup_hints/macos_launcher.rs`（`unwrap`: 0, `expect`: 13, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 13）
- [ ] 加固 `src/server/client_lifecycle.rs`（`unwrap`: 0, `expect`: 10, `panic!`: 3, `todo!`: 0, `unimplemented!`: 0, 总计: 13）
- [ ] 加固 `src/registry.rs`（`unwrap`: 0, `expect`: 13, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 13）
- [ ] 加固 `src/tool/batch.rs`（`unwrap`: 12, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 12）
- [ ] 加固 `src/server/swarm_mutation_state.rs`（`unwrap`: 0, `expect`: 8, `panic!`: 4, `todo!`: 0, `unimplemented!`: 0, 总计: 12）
- [ ] 加固 `src/provider_catalog.rs`（`unwrap`: 0, `expect`: 12, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 12��
- [ ] 加固 `src/prompt.rs`（`unwrap`: 11, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 12）
- [ ] 加固 `src/tool/agentgrep.rs`（`unwrap`: 0, `expect`: 11, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 11）
- [ ] 加固 `src/tool/ambient.rs`（`unwrap`: 10, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 10）
- [ ] 加固 `src/soft_interrupt_store.rs`（`unwrap`: 0, `expect`: 9, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 9）
- [ ] 加固 `src/server/provider_control.rs`（`unwrap`: 3, `expect`: 6, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 9）
- [ ] 加固 `src/platform.rs`（`unwrap`: 0, `expect`: 9, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 9）
- [ ] 加固 `src/cli/login.rs`（`unwrap`: 0, `expect`: 8, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 9）
- [ ] 加固 `src/cli/commands/restart.rs`（`unwrap`: 0, `expect`: 9, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 9）
- [ ] 加固 `src/tool/side_panel.rs`（`unwrap`: 0, `expect`: 8, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/tool/browser.rs`（`unwrap`: 6, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/stdin_detect.rs`（`unwrap`: 0, `expect`: 8, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/sidecar.rs`（`unwrap`: 0, `expect`: 8, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/runtime_memory_log.rs`（`unwrap`: 0, `expect`: 8, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/message.rs`（`unwrap`: 4, `expect`: 1, `panic!`: 3, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/gateway.rs`（`unwrap`: 1, `expect`: 7, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/ambient.rs`（`unwrap`: 8, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 8）
- [ ] 加固 `src/server/swarm.rs`（`unwrap`: 0, `expect`: 6, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 7）
- [ ] 加固 `src/server/debug_testers.rs`（`unwrap`: 0, `expect`: 7, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 7）
- [ ] 加固 `src/provider/cursor.rs`（`unwrap`: 4, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 7）
- [ ] 加固 `src/dictation.rs`（`unwrap`: 0, `expect`: 7, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 7）
- [ ] 加固 `src/browser.rs`（`unwrap`: 2, `expect`: 5, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 7）
- [ ] 加固 `src/tui/app/helpers.rs`（`unwrap`: 0, `expect`: 6, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/tool/session_search.rs`（`unwrap`: 1, `expect`: 5, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/tool/open.rs`（`unwrap`: 6, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/setup_hints.rs`（`unwrap`: 0, `expect`: 6, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/server/swarm_persistence.rs`（`unwrap`: 0, `expect`: 6, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/provider/antigravity.rs`（`unwrap`: 0, `expect`: 6, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/logging.rs`（`unwrap`: 0, `expect`: 6, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 6）
- [ ] 加固 `src/tool/mcp.rs`（`unwrap`: 4, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 5）
- [ ] 加固 `src/tool/conversation_search.rs`（`unwrap`: 5, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 5）
- [ ] 加固 `src/telegram.rs`（`unwrap`: 5, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 5）
- [ ] 加固 `src/server/debug_command_exec.rs`（`unwrap`: 0, `expect`: 4, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 5）
- [ ] 加固 `src/provider/pricing.rs`（`unwrap`: 0, `expect`: 5, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 5）
- [ ] 加固 `src/tui/ui.rs`（`unwrap`: 0, `expect`: 4, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `src/transport/windows.rs`（`unwrap`: 0, `expect`: 4, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `src/tool/skill.rs`（`unwrap`: 4, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `src/safety.rs`（`unwrap`: 2, `expect`: 1, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `src/login_qr.rs`（`unwrap`: 3, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `src/channel.rs`（`unwrap`: 4, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `crates/jcode-tui-workspace/src/workspace_map.rs`（`unwrap`: 0, `expect`: 4, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 4）
- [ ] 加固 `src/tui/ui_messages.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/tui/ui_header.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 3）
- [ ] 加固 `src/tui/login_picker.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/tui/keybind.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/tui/app/auth.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/session.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/server/comm_plan.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/cli/terminal.rs`（`unwrap`: 2, `expect`: 0, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/bin/tui_bench.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `crates/jcode-provider-openrouter/src/lib.rs`（`unwrap`: 0, `expect`: 3, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 3）
- [ ] 加固 `src/video_export.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/tui/ui_animations.rs`（`unwrap`: 0, `expect`: 0, `panic!`: 2, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/tui/backend.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/tui/account_picker.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/tool/mod.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 2）
- [ ] 加固 `src/server/debug_server_state.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/server/client_disconnect_cleanup.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/server/client_comm_channels.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/provider/openrouter_sse_stream.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/provider/jcode.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/perf.rs`（`unwrap`: 2, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/memory/activity.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/mcp/pool.rs`（`unwrap`: 0, `expect`: 0, `panic!`: 2, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/mcp/manager.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/copilot_usage.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/cache_tracker.rs`（`unwrap`: 2, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/auth/antigravity.rs`（`unwrap`: 0, `expect`: 2, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 2）
- [ ] 加固 `src/ambient/runner.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 1, 总计: 2）
- [ ] 加固 `src/tui/workspace_client.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/ui_prepare.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/ui_diagram_pane.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/test_harness.rs`（`unwrap`: 1, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/color_support.rs`（`unwrap`: 0, `expect`: 0, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/app/remote/reconnect.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/app/remote/input_dispatch.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/app/dictation.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/app/debug_bench.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tui/app/commands.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tool/todo.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tool/selfdev/reload.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/tool/memory.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/telemetry.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server/headless.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server/debug_swarm_read.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server/debug_session_admin.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server/comm_sync.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server/client_comm_message.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server/client_comm_context.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/server.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/provider/claude.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/provider/anthropic.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/protocol.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/plan.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/memory/pending.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/gmail.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/config.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/background.rs`（`unwrap`: 0, `expect`: 1, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `src/ambient/scheduler.rs`（`unwrap`: 1, `expect`: 0, `panic!`: 0, `todo!`: 0, `unimplemented!`: 0, 总计: 1）
- [ ] 加固 `crates/jcode-tui-workspace/src/color_support.rs`（`unwrap`: 0, `expect`: 0, `panic!`: 1, `todo!`: 0, `unimplemented!`: 0, 总计: 1）

### 抑制清理积压

- [ ] 移除或论证 `src/agent/turn_loops.rs` 中的抑制（unused_variables）
- [ ] 移除或论证 `src/auth/mod.rs` 中的抑制（unused_mut）
- [ ] 移除或论证 `src/cli/dispatch.rs` 中的抑制（deprecated, unused_mut, unused_mut）
- [ ] 移除或论证 `src/main.rs` 中的抑制（non_upper_case_globals, non_upper_case_globals）
- [ ] 移除或论证 `src/perf.rs` 中的抑制（non_snake_case）
- [ ] 移除或论证 `src/server.rs` 中的抑制（unused_mut, unused_mut）
- [ ] 移除或论证 `src/server/client_actions.rs` 中的抑制（clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/client_lifecycle.rs` 中的抑制（clippy::too_many_arguments, clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/client_session.rs` 中的抑制（clippy::too_many_arguments, clippy::too_many_arguments, clippy::too_many_arguments, clippy::too_many_arguments, clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/comm_await.rs` 中的抑制（clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/comm_session.rs` 中的抑制（clippy::too_many_arguments, clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/comm_sync.rs` 中的抑制（clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/debug_swarm_write.rs` 中的抑制（clippy::too_many_arguments）
- [ ] 移除或论证 `src/server/startup_tests.rs` 中的抑制（unused_mut）
- [ ] 移除或论证 `src/tui/app/remote.rs` 中的抑制（unused_imports, unused_imports）
- [ ] 移除或论证 `src/tui/app/state_ui.rs` 中的抑制（unused_mut）
- [ ] 移除或论证 `src/tui/info_widget.rs` 中的抑制（deprecated）

### 生产环境 `todo!` / `unimplemented!` 积压

- [ ] 从 `src/tui/ui_header.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/tui/app/remote.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/tool/mod.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/server/debug_command_exec.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/server/debug.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/server/client_state.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/server/client_comm.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/server/client_actions.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/provider/gemini.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/cli/selfdev.rs` 移除 `todo!` / `unimplemented!`（1 处）
- [ ] 从 `src/ambient/runner.rs` 移除 `todo!` / `unimplemented!`（1 处）

### 测试环境 `todo!` / `unimplemented!` 积压

- [ ] 替换 `src/tui/app/tests.rs` 中的测试 `todo!` / `unimplemented!`（7 处）
- [ ] 替换 `src/server/startup_tests.rs` 中的测试 `todo!` / `unimplemented!`（1 处）
- [ ] 替换 `src/server/queue_tests.rs` 中的测试 `todo!` / `unimplemented!`（1 处）
- [ ] 替换 `src/server/client_session_tests.rs` 中的测试 `todo!` / `unimplemented!`（1 处）

### TODO / FIXME / HACK 标记积压

- [ ] 解决 `docs/audits/代码质量审计2026-04-18.md` 中的标记（9 处）
- [ ] 解决 `src/tui/ui_tests/prepare.rs` 中的标记（5 处）
- [ ] 解决 `src/tui/ui_tests/tools.rs` 中的标记（4 处）
- [ ] 解决 `src/stdin_detect.rs` 中的标记（1 处）
- [ ] 解决 `docs/记忆架构.md` 中的标记（1 处）
- [ ] 解决 `docs/IOS_CLIENT.md` 中的标记（1 处）
