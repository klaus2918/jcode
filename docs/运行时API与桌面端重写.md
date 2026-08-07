# 运行时 API + 桌面端重写（纯 Rust）

状态：草案，方向已批准（2026-07-24）

## 动机

- 智能体运行时（"harness"）已经是客户端/服务器架构：通过 Unix socket（`~/.jcode/jcode.sock`）传输 NDJSON，`Request` / `ServerEvent` 定义在 `crates/jcode-protocol`。但它是*内部*线格式：无版本号、约 147 个变体、面向 TUI 形状、耦合客户端渲染假设。
- 当前桌面应用（`crates/jcode-desktop`，约 4.4 万行）在 wgpu 0.19 + winit 0.29 + glyphon 上手写渲染，外加自己的宿主/工作线程 IPC。文本布局、富文本、滚动和 markdown 全部自研，是渲染质量问题的主要来源。从头重写。

## 第 1 部分：Harness API

目标：一个稳定、带版本号的边界。每个 UI（TUI、新桌面端、未来的 Web/移动端）都是客户端。运行时内不包含任何 UI 特定逻辑。

### 方案

引入 `crates/jcode-harness-api`：

- **带版本号的信封。** 每个帧在最顶层携带 `v`（协议主版本）。握手：客户端发送 `hello { min_version, max_version, client }`，服务器回复 `hello_ok { version, server, capabilities }`。未知字段被忽略；未知事件类型可跳过（客户端侧用带 catch-all `Unknown` 的标记枚举）。
- **精选表面，而非全量转储。** 从一个小而稳定的核心开始并逐步扩展：
  - 会话生命周期：创建/附加/分离/列出会话、工作目录。
  - 对话：发送消息（文本 + 图片）、取消、软中断、清空、回退、历史获取。
  - 流式事件：文本/推理增量、工具开始/输入/执行/完成、token 用量、回合完成、错误。
  - 权限：权限请求事件 + 客户端响应。
  - 状态：智能体状态快照、todos、计划/任务图摘要。
  其余一切（集群内部、自研开发、调试）在有意提升之前继续使用内部协议。
- **传输。** Unix socket 上的 NDJSON 仍是主要传输方式。API crate 定义与传输无关的类型 + 一个小型客户端（`HarnessClient`）和服务器适配器，这样以后可以添加 WebSocket/TCP 传输而无需改动 schema。
- **与 `jcode-protocol` 的关系。** 短期内服务器适配器把 API 请求映射到现有内部处理上。长期内部协议向 API 收缩。不 fork 语义：API 是门面，运行时仍然是事实来源。

### 交付物

1. `crates/jcode-harness-api`：类型、版本常量、握手、`HarnessClient`（阻塞 + 异步友好的分帧）。
2. 服务器：在现有 socket 上接受 API 握手（嗅探第一行：`hello` = API 客户端，否则为旧协议）。
3. 参考客户端示例（`examples/harness_repl.rs`）：连接、创建会话、发送消息、打印流式事件。这是 API 的验收测试。
4. Schema 快照测试，使意外的不兼容变更在 CI 中失败。

## 第 2 部分：桌面端重写（纯 Rust）

决策：**winit + wgpu + Vello + Parley**（Linebender 技术栈），不用 UI 框架。

为什么选这个中间路线：

- **通用渲染由库支撑。** Vello：GPU 2D 矢量渲染器（路径、填充、字形 run、裁剪、图层）。Parley：真正的文本布局（通过 swash 整形、双向文本、断行、富文本 span）。这两者替代旧桌面端手写最薄弱的两部分。
- **产品级合成保持完全自研。** 我们拥有帧循环、场景图、输入路由和动画。Niri 式控制（平铺工作区、手势驱动的弹簧过渡、每个表面的变换）每帧只需"决定变换、发出场景"。没有框架布局引擎要对抗。
- **通往更多定制的逃生通道。** Vello 渲染到我们控制的 wgpu 纹理/表面。当需要 Vello 缺少的效果（模糊、shader、自定义合成、嵌入终端网格）时，我们在同一帧里添加原始 wgpu pass。从较少定制开始，按功能逐步增加定制，绝不重写。

明确否决：
- egui：即时模式与流畅的工作区动画和长富文本记录冲突。
- gpui：框架锁定、Linux 成熟度、合成器自由度更少。
- 保留当前 wgpu/glyphon 代码：文本/布局层就是问题所在。

### 架构草图

```
jcode-desktop2（新 crate，旧 crate 在达到一致前不动）
├── platform/     winit 事件循环、窗口、输入、剪贴板
├── gpu/          wgpu 设备/表面、Vello 渲染器、自定义 pass
├── scene/        保留场景：带变换 + 内容 + z 的节点
├── text/         Parley 布局缓存、富文本（markdown -> spans）
├── anim/         驱动节点变换的弹簧/时间线（niri 风格）
├── ui/           基于 scene+text 构建的部件（记录、输入、面板）
├── workspace/    平铺/工作区模型、手势、焦点
└── harness/      HarnessClient 接线、会话状态、事件 -> UI 模型
```

关键不变���：
- 桌面端**只**通过 `jcode-harness-api` 与运行时通信。
- 渲染是（场景状态，动画时钟）的纯函数。
- 文本布局被缓存，仅由内容/宽度变化失效。

### 里程碑

1. Harness API crate + 握手 + 参考客户端（验证第 1 部分）。
2. `jcode-desktop2` 骨架：窗口、Vello "hello 场景"、Parley 段落。
3. 记录视图：markdown -> 富 span -> Parley、平滑滚动。
4. 输入框 + 来自 harness API 的实时流（第一个可用构建）。
5. 工作区/平铺 + 弹簧动画（niri 式控制）。
6. 与旧桌面端的一致性检查清单，然后删除 `jcode-desktop`。
