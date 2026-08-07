# jcode-tui 测试不稳定：根因

`cargo test -p jcode-tui --lib` 每次运行失败 1-4 个测试，且失败集合不同。
这是进程全局状态上的并行竞争，不是逻辑错误。

## 证据

- `cargo test -p jcode-tui --lib -- --test-threads=1` 通过 **2006/2006**（16 个忽略）。
- 失败集合在默认线程数下每次运行都变化。
- 单独运行时，每个失败的测试都能通过。

计数取自 2026-07-27，随着测试增加会漂移。请在空闲机器上复现：在内存压力下（本机 15 GiB 且正在并发构建工作区）`cargo` 会在编译中途被 SIGTERM，这与这里描述的竞争是不同故障。

## 根因

`create_test_app()`（以及它的 `create_named_provider_test_app` 兄弟）在
`crates/jcode-tui/src/tui/app/tests/support_failover/part_01.rs` 中调用：

```rust
crate::tui::ui::clear_test_render_state_for_tests();
```

那会清空**进程全局**渲染状态：闪烁帧历史、布局快照、状态区快照、复制目标和滚动位置。

渲染测试正好用 `render_state_test_lock()` 保护该状态。但 `create_test_app` *不加锁*就清空它，因此它约 810 个调用点中的任何一个都能在断言中途重置并发运行的渲染测试状态。

最常见受害者的机制（`test_changelog_overlay_repeated_renders_are_stable`）在 `clear_test_render_state_for_tests` 本身中有文档：记录到的闪烁事件会给后续渲染添加一行"⚠ 检测到闪烁"通知，使每个布局敏感断言偏移一行。

### 二分证明

把 959 个 `tui::app::tests::` 测试针对 changelog 测试二分，识别出 `test_tui_login_providers_have_real_tui_handlers`，它在循环中调用 `create_test_app()`（每个登录提供商一次）。只运行这两个不会复现；竞争需要足够的并发负载才能交错，这就是它表现为顺序相关不稳定性的原因。

## 什么不起作用

**在 `create_test_app` 内获取 `render_state_test_lock`。** 这正确但会串行化全部约 810 个调用点：套件运行时间从约 12 秒涨到超过 10 分钟。已测量并回退。

**在 changelog 测试的 `buffered_samples` 检查中断言下限而不是精确计数**，以及**在该测试顶部调用 `clear_test_render_state_for_tests`**。两者都测量了 5 次运行：无论有无该变更，测试仍然 5/5 失败。已回退而不是作为 churn 提交。

## 建议方向

真正的修复是停止跨测试共享该状态，而不是串行化对它的访问：

1. 把渲染状态做成线程局部而不是进程全局，使并行测试无法观察到彼此的复位。生产环境只有一个渲染线程，因此这不应改变运行时行为。
2. 如果做不到，让 `create_test_app` 完全跳过渲染状态清空。只有渲染测试依赖它，而它们已经在锁下清空。这需要审计哪些应用测试隐式依赖当前的清空。

选项 1 是首选：它移除共享可变状态，而不是围绕它添加协调。

## 范围说明

这是预先存在的，独立于提交 `0ba0154c6`、`2b8e78e34`、`8b44fc83b`、`8142f1a0b` 中的渲染路径性能工作。通过暂存这些变更并复现相同的失败率验证。
