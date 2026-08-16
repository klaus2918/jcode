# Crate 归属与模块化边界

> **注（2026-08-16）**：以下独立类型 crate 已随 feature-simplification 精简合并：`jcode-gateway-types` → `jcode-base/src/gateway/types.rs`、`jcode-ambient-types` → `jcode-usage-types`、`jcode-batch-types` 与 `jcode-side-panel-types` → `jcode-protocol`、`jcode-tui-tool-display` → `jcode-tui/src/tui/tool_display.rs`、`jcode-update-core` → `jcode-app-core/src/update_core.rs`。本文的归属规则与编译速度方法论仍然有效；验证命令中涉及已删 crate 的请按新家园调整 `-p` 参数。

本文档定义保持 `jcode` 模块化而不让共享 crate 变成倾倒场的目标结构。它刻意实用：决定是否把类型、助手或行为移出根 crate 时使用它。

## 目标

主要目标：通过缩小根 crate 的重编译面使日常开发和自研构建更快。结构整洁有价值，因为它支持编译时目标。

- 把稳定 DTO 和协议安全状态移入小型 crate，使根行为变更不重编译那些契约，契约变更只重编译聚焦的依赖者。
- 保持依赖轻的 crate 依赖轻，使它们编译快，且不把大型运行时/TUI/提供商图拖入无关构建。
- 在完整依赖边界可以不增加依赖扇出地移动之前，把仅根的行为、存储、进程、TUI、服务器和提供商运行时逻辑保留在根 crate 中。
- 避免通过宽泛的 `jcode-core` 再导出产生循环依赖和隐藏耦合。
- 迁移期间保留 serde 兼容性和根再导出，除非所有调用点被有意更新。
- 用编译影响衡量成功：更少的根编辑、更少的根拥有 DTO、更小的依赖扇出，以及常见变更后更快的 `cargo check --profile selfdev` / `selfdev build`。

## 归属规则

### 类型 crate 拥有稳定的数据契约

`*-types` crate 应包含：

- 被多个 crate 或协议层使用的纯数据结构。
- 与数据契约绑定的序列化形态和小型纯助手方法。
- 无文件系统、网络、进程、TUI、提供商客户端、全局状态或存储访问。
- 依赖限于 serde、chrono 和必要时其他类型 crate。

示例：`jcode-session-types`、`jcode-side-panel-types`、`jcode-selfdev-types`、`jcode-background-types`。

### 领域行为模块拥有根运行时行为

当根模块需要以下时，应保留行为：

- `crate::storage`、`crate::config`、`crate::logging`、`crate::server` 或进程派生。
- 提供商 HTTP 客户端和认证管理器。
- Tokio 运行时、后台任务、通道、全局缓存、文件锁或 PID 注册表。
- TUI 渲染和 crossterm/ratatui 状态。

如果类型的内在方法需要这些 API，要么把类型留在根中，要么把行为和依赖一起移入领域 crate。不要只移动结构体，如果那样会在根中强制非法固有实现。

### `jcode-core` 用于真正共享的原语

`jcode-core` 应包含：

- 还没有明显领域 crate 的跨领域原语。
- 被许多 crate 使用的非常小、依赖轻的助手。
- 仅在创建新领域类型 crate 还为时过早时的临时 DTO 暂存。

`jcode-core` 不应无限积累每个提取的 DTO。一旦集群增长，把它拆分为聚焦的领域 crate。

### 编译速度决策规则

当拆分减少根 crate 变更或依赖扇出时，偏好拆分。如果新 crate 增加依赖、扩大重建扇出或迫使频繁跨 crate 编辑，就不要只为文件看起来整洁而拆分。一个好的拆分至少有以下编译时收益之一：

- 常见根行为编辑不再触碰稳定类型定义。
- 类型专属变更可以通过编译小型类型 crate 加聚焦依赖者来检查。
- 重型依赖远离 DTO crate。
- 多个下游 crate 可以使用小型契约而不依赖根 crate。

### 再导出策略

迁移期间：

1. 把类型移到目标 crate。
2. 把旧根路径保留为 `pub use ...` 以保留调用点。
3. 验证聚焦测试和自研构建/重载。
4. 之后，仅在下游 crate 可以直接依赖领域 crate 后移除过时的根再导出。

## 移动检查清单

对每个类型或纯助手迁移使用此清单。非平凡移动时把它复制到 PR/提交说明中。

1. 对候选分类。
   - [ ] 它是稳定数据契约或纯助手，而不是根运行时行为？
   - [ ] 它有固有方法吗？
   - [ ] 那些方法需要存储、网络客户端、TUI 状态、进程管理或全局等仅根 API 吗？
   - [ ] 如果行为也必须移动，完整依赖边界可以不增加扇出地移动吗？
2. 检查兼容性。
   - [ ] 它的 serde 表示保持相同吗？
   - [ ] 默认值、跳过、重命名和枚举判别式保留了吗？
   - [ ] 所有字段可见性仍然合适吗？
   - [ ] 根可以保留兼容再导出吗？
3. 检查 crate 健康。
   - [ ] 目标 crate 已有需要的依赖策略吗？
   - [ ] 新依赖限于类型 crate 合适的库，通常是 `serde`、`serde_json`、`chrono` 或兄弟类型 crate 吗？
   - [ ] 目标 crate 仍然无环吗？
   - [ ] `cargo metadata`/`cargo check` 避免把根、TUI、提供商、存储、服务器或进程依赖拖入类型 crate 吗？
4. 验证。
   - [ ] 有覆盖移动类型的聚焦测试过滤器吗？
   - [ ] `cargo check --profile selfdev -p <type-crate> -p jcode --bin jcode` 通过了吗？
   - [ ] 相关聚焦根测试通过了吗？
   - [ ] `cargo fmt` 通过了吗？
   - [ ] 自研构建和重载从干净已提交的 HEAD 通过了吗？

## 依赖边界守卫

添加或更改任何类型 crate 依赖后运行此守卫：

```sh
python3 scripts/check_dependency_boundaries.py
```

守卫阻止 `jcode-*-types` crate 直接依赖根/运行时重型内部 crate，如 `jcode`、`jcode-core`、提供商 crate、TUI crate、协议/运行时 crate 和桌面/移动 crate。类型 crate 可以依赖外部轻量库和其他类型 crate。如果需要新内部依赖，先决定它本身是否应成为类型 crate。

## 测试策略

验证偏好聚焦过滤器。宽泛过滤器常常选中无关的有状态、时序敏感或基准测试。

模块化期间观察到的已知宽泛过滤器危害：

- `side_panel` 选中无关的固定 UI/布局和延迟基准测试。
- `usage` 在纯用量测试之外还选中应用显示测试。
- `session::` 选中实时挂接服务器测试和选择器行为，超出会话持久化。
- `ambient` 选中带配置和调度状态的 TUI/助手集成测试，超出环境模块持久化/运行时测试。

在每个领域 crate/模块旁记录精确过滤器。宽泛过滤器对定期扫查仍然有用，但当精确测试和编译检查通过时，不应阻塞仅 DTO 的提取。

当前 DTO 拆分后的聚焦验证矩阵：

| 领域 | 快速编译检查 | 拆分期间使用的聚焦根测试 | 说明 |
| --- | --- | --- | --- |
| 用量 DTO | `cargo check --profile selfdev -p jcode-usage-types -p jcode --bin jcode` | 偏好 usage/copilot usage 模块下的精确测试。避免把裸 `usage` 作为必需门，因为它也选中显示/UI 测试。 | DTO crate 拥有报告和本地计数器契约。运行时获取/缓存/显示留在根。 |
| 网关 DTO | `cargo check --profile selfdev -p jcode-gateway-types -p jcode --bin jcode` | 可用时按精确测试名聚焦网关持久化/认证测试。 | 配对/令牌 HTTP/WebSocket 行为留在根。 |
| 环境 DTO | `cargo check --profile selfdev -p jcode-ambient-types -p jcode --bin jcode` | 仅调度器/类型使用者。 | 环境 DTO crate 只拥有用量记录。队列/运行时/提示词行为留在根。 |
| 环境行为模块 | `cargo check --profile selfdev -p jcode --bin jcode` | `cargo test --profile selfdev -p jcode ambient::ambient_tests --lib`；`cargo test --profile selfdev -p jcode ambient::scheduler::tests --lib`；`cargo test --profile selfdev -p jcode ambient::runner::runner_tests --lib` | 避免把裸 `ambient` 作为模块级重构的必需门，因为它选中跨模块 TUI/配置状态测试。 |
| 记忆活动 DTO | `cargo check --profile selfdev -p jcode-memory-types -p jcode-core -p jcode --bin jcode` | `cargo test --profile selfdev -p jcode runtime_memory_log --lib`；`cargo test --profile selfdev -p jcode tui::info_widget::tests --lib` | `memory::activity` 当前不匹配任何测试，因此使用使用者测试。 |
| 目标/待办/追赶核心 DTO | `cargo check --profile selfdev -p jcode-core -p jcode --bin jcode` | 行为变化时用精确的目标/待办/追赶过滤器。 | 当前小/稳定到足以留在 `jcode-core`；若变更增长则重新审视。 |

## 编译基线观察

2026-04-30 在编译速度边界文档提交后用 `scripts/dev_cargo.sh check --profile selfdev -p jcode --bin jcode` 测量。这是粗略的 mtime 触碰基准，不是完整统计研究，但足以指导优先级。

| 场景 | 观察时间 | 解读 |
| --- | ---: | --- |
| 近期仅文档提交后的无操作检查 | 约 65.8 秒 | 环境/缓存状态可能主导第一次检查。把它当作热身/噪音基线，而不是纯无操作稳态。 |
| 触碰根行为模块 `src/usage.rs` | 约 6.25 秒 | 依赖已构建时，仅根行为编辑可以相对便宜。 |
| 触碰 `crates/jcode-core/src/usage_types.rs` | 约 65.35 秒 | 编辑 `jcode-core` 使广泛下游依赖者失效。避免向 `jcode-core` 添加高变更领域 DTO。 |

含义：编译速度目标不只是"把东西移出根"。把稳定、低变更契约移出根是好的，但把许多高变更领域 DTO 放进 `jcode-core` 可能适得其反，因为 `jcode-core` 有高扇出。对可能变化的领域 DTO，偏好聚焦叶子 crate，如 `jcode-usage-types`、`jcode-gateway-types` 和 `jcode-ambient-types`。

## `jcode-core` 扇出审计

在这个检查点，根 crate 是 `jcode-core` 的唯一直接 Cargo 依赖，但根再导出许多 `jcode-core` 模块，而根是高成本重编译目标。上述基线中触碰 `jcode-core` 使广泛下游检查失效。因此即使 Cargo.toml 直接依赖者当前很少，`jcode-core` 也应被视为高扇出 crate。

观察到的根再导出/使用路径：

- `src/catchup.rs` -> `catchup_types`
- `src/goal.rs` -> `goal_types`
- `src/todo.rs` -> `todo_types`
- `src/env.rs`、`src/id.rs`、`src/stdin_detect.rs`、`src/util.rs` 和 panic UI 助手 -> 通用工具

此审计的编译速度优先级：

1. 把聚簇、可能变化的领域 DTO 从 `jcode-core` 移到聚焦叶子 crate。
2. 在 `jcode-core` 中保留稳定通用工具。
3. 除非非常稳定或临时暂存，否则避免向 `jcode-core` 添加新领域 DTO。

| 模块 | 当前内容 | 首选长期家园 | 说明 |
| --- | --- | --- | --- |
| `ambient_usage_types` | 环境调度器用量记录/速率限制 DTO | 已移至 `jcode-ambient-types` | 兼容再导出保留在根模块中。 |
| `catchup_types` | 追赶持久化状态和渲染摘要 DTO | `jcode-catchup-types` 或留在核心 | 小且低变更。仅当追赶增长时拆分。 |
| `copilot_usage_types` | 本地 Copilot 用量计数器 | 已移至 `jcode-usage-types` | 兼容再导出保留在根模块中。 |
| `gateway_types` | 配对设备和配对码持久化记录 | 已移至 `jcode-gateway-types` | 配对/令牌行为留在根。 |
| `goal_types` | 目标状态、里程碑、状态、更新 | `jcode-goal-types` 或 `jcode-task-types` | 较大领域。如果目标/工具工作增长，值得拆分。 |
| `memory_types` | 记忆活动 DTO | 已移至 `jcode-memory-types` | 记忆有足够领域重量拥有自己的类型 crate。 |
| `todo_types` | 待办项 DTO | `jcode-task-types`、`jcode-todo-types` 或核心 | 微小。可以加入目标/追赶任务状态 crate。 |
| `usage_types` | 提供商用量报告 DTO | 已移至 `jcode-usage-types` | 运行时获取/缓存/显示留在根。 |
| `env` | 环境变量助手 | 留在核心 | 通用工具，无需领域 crate。 |
| `id` | ID 助手 | 留在核心 | 通用工具。 |
| `panic_util` | Panic 格式化助手 | 留在核心 | 通用运行时工具。 |
| `stdin_detect` | 标准输入检测助手 | 留在核心 | 通用平台/运行时工具。 |
| `util` | 杂项工具 | 稍后审计 | 不应变成收集处。 |

## 目标领域类型 crate

已完成/高价值领域类型拆分：

1. `jcode-usage-types`
   - `usage_types`
   - `copilot_usage_types`
   - 从根格式化/运行时助手分离后的纯账号用量 DTO

2. `jcode-gateway-types`
   - `gateway_types`
   - 决定配置是否拥有它之后的可能 `GatewayConfig`
   - 移动 crate 需要时面向移动网关的协议安全 DTO

3. `jcode-ambient-types`
   - `ambient_usage_types`
   - 仅在仅根 `AmbientState::load/save/record_cycle` 方法分离为根自由函数或持久化层后的环境状态/请求/结果 DTO

4. `jcode-memory-types`
   - `memory_types`
   - 跨服务器/TUI/工具使用的任何记忆协议/活动 DTO

5. 可选任务状态 crate
   - `goal_types`
   - `todo_types`
   - 产品模型想要分组时的 `catchup_types`

## 大模块重构目标

这些不是简单 DTO 移动。先重构行为边界。

### `src/session.rs`

目标拆分：

- 元数据/会话模型
- 持久化和日志重放
- 启动桩和远程启动快照
- 内存分析/缓存归因
- 渲染住在现有 `session/render.rs`
- 崩溃恢复住在现有 `session/crash.rs`

### `src/ambient.rs`

目标拆分：

- 可见周期上下文 I/O
- 状态持久化
- 指令持久化
- 调度队列和锁定
- 提示词构建
- 管理器/运行时编排

在 load/save/record 行为与结构体分离前，不要移动 `AmbientState` 作为 DTO。

### `src/usage.rs`

目标拆分：

- API 获取提供商
- 提供商响应解析
- 本地缓存/同步
- 显示格式化
- 账号选择/指导
- `jcode-usage-types` 中的公共报告 DTO

### `src/gateway.rs`

目标拆分：

- 注册表持久化
- 配对/令牌认证
- HTTP 路由处理
- WebSocket 认证/提取
- WebSocket 中继
- `jcode-gateway-types` 中的公共网关 DTO

## "足够优"的定义

结构足够好的标准：

- 每个类型 crate 有清晰领域和最小依赖集。
- `jcode-core` 只包含真正原语或有文档的临时暂存模块。
- 根模块不再在一个文件中混合大 DTO 块、持久化、运行时编排和渲染。
- 每个领域都有聚焦验证命令。
- 每次结构变更后自研构建/重载干净工作。
