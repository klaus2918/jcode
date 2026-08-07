# 编译期 crate 拆分计划

## 目标

最小化迭代 Jcode 时必须重新检查或重新构建的代码量。根 `jcode` crate 仍然是集成外壳，但稳定的叶子代码应放在具有单向依赖的小型 crate 中。

## 原则

1. 先提取稳定的叶子：文件系统/存储、协议/类型、解析器、提供商请求/流编解码器，以及 TUI 渲染原语。
2. 避免循环的领域 crate。根 `jcode` 可以依赖叶子 crate，但叶子 crate 不得直接回调根日志/配置/运行时。在边界使用数据类型、回调或显式事件。
3. 按重新编译的易变性拆分，而不是按目录名拆分。经常编辑的代码不应在非必要时强制重型提供商/TUI/服务器模块重建。
4. 将重型可选依赖放在 crate/feature 后面。嵌入、PDF、桌面/移动、浏览器和图像/渲染流水线应保持隔离。
5. 迁移期间保留兼容外观。`crate::storage::*` 可以重新导出 `jcode-storage::*`，调用方逐步迁移。

## 当前第一步

`jcode-storage` 现在是一个叶子 crate，负责应用路径、权限加固、原子 JSON 写入和追加式 JSONL 辅助。根 `src/storage.rs` 模块是一个薄的兼容外观，为备份恢复保留现有日志行为。

在本机提取后的测量：

- `cargo check -p jcode-storage`：依赖构建完成后约 0.9 秒。
- `cargo check -p jcode --lib`：当前热缓存状态下约 14 秒。

## 推荐的后续提取

1. `jcode-provider-anthropic`：将 Anthropic 请求/流转换移出根 `src/provider/anthropic.rs`，只依赖 `jcode-provider-core`、`jcode-message-types` 以及 serde/reqwest 原语。
2. `jcode-provider-openai`：OpenAI 请求/流处理同样处理。这减少了编辑服务器/TUI 代码时的重建，并使提供商测试更便宜。
3. `jcode-session-core`：一旦对根提示词/日志的依赖通过回调切断，就移动会话存储路径、日志元数据和记忆配置文件的纯转换。
4. `jcode-tui-app-state`：将按键/输入/导航状态转换与渲染分离。将 ratatui 渲染保留在 `jcode-tui-render`/根中，同时状态测试无需编译整个根 crate。
5. `jcode-server-protocol-runtime`：将 websocket/客户端事件扇出胶水与智能体执行分离，使服务器测试不需要重建 TUI/提供商内部。

## 应避免的反模式

- 提取依赖根 `jcode` 的 crate。那会保留编译时瓶颈并产生依赖循环。
- 为每个文件建立微型 crate。过多的 crate 会增加元数据开销并让重构痛苦。
- 只移动类型别名而把实现留在根中。昂贵的编译单元仍然昂贵。
