# jcode iOS 应用

> 状态：v2 重建。纯 Swift。取代早期原型与 Rust-mobile-core/模拟器方向，两者均已在 `ios-app-restart` 分支历史中移除。

## 产品定义

面向运行在你自有机器上的 jcode 服务器的原生 iOS 远程控制端。手机渲染会话并驱动会话；所有重活（LLM 调用、工具、git、文件、MCP）都留在服务器端。可达性假设基于 Tailscale（或局域网）；应用从不直接与 LLM 提供商通信。

设计基调：暗色、平静、终端原生但不复古。薄荷绿强调色（`#4DD9A6`）表示在线/已连接状态。触控卡片承载密集信息。

## 架构决策：纯 Swift

应用是既有服务器自有协议之上的薄客户端。值得共享的行为已经存在于服务端。共享 Rust 核心会第三次重复服务器协议，还需要 FFI 桥、自定义渲染器以及并行的模拟器来保持诚实。取而代之：

- **Swift 拥有客户端。** UI 用 SwiftUI，Swift 6 并发，`@Observable`。
- **服务器协议是唯一事实来源。** Swift 编解码器通过针对真实线上 JSON 的 fixture 测试校验；漂移会导致测试失败。
- **无设备可测性**来自分层而非模拟器：视图层以下的一切都在 macOS 上通过 `swift test` 构建和测试。

## 分层

```
ios/
  Package.swift              SPM 包，可在 macOS + iOS 构建
  Sources/JCodeKit/          平台无关的客户端核心（无 UIKit）
    Gateway.swift            端点：/health、/pair、/ws
    Pairing.swift            配对码 -> 认证令牌
    Wire.swift               Request/ServerEvent 编解码器（WS 上的 NDJSON）
    Transport.swift          WebSocketTransport 协议 + URLSession 实现
    Connection.swift         actor：连接/认证/重连，AsyncStream<ServerEvent>
    SessionReducer.swift     纯状态机：事件 -> 会话/应用状态
    CredentialStore.swift    Keychain 支撑的服务器凭据（协议 + 实现）
  Sources/JCodeMobile/       SwiftUI 应用壳（仅 iOS）
    JCodeMobileApp.swift     入口
    AppModel.swift           @Observable 粘合层：Connection + Reducer -> 视图
    Views/                   配对、聊天、会话、设置等视图
    QRScannerView.swift      相机配对
    Theme.swift              颜色/排版令牌
  Tests/JCodeKitTests/       macOS 上的 swift test：编解码 fixture、reducer、配对
  project.yml                应用目标的 XcodeGen 规格
```

规则：

- `JCodeKit` 绝不导入 UIKit/SwiftUI。它必须能在 macOS 上编译，这样整个行为层就可以在本机由 agent 无头测试。
- 视图不含协议或状态转换逻辑。`AppModel` 只转发动作并发布 reducer 输出。
- `SessionReducer` 是纯函数 `(State, ServerEvent) -> State`（外加本地用户意图）。所有流式/工具/会话边界情况都在这里做单元测试，取代旧 Rust 模拟器的角色。

## 协议

服务端（已上线，不变）：

- `jcode pair` CLI 生成 6 位码（5 分钟 TTL）以及带 `jcode://pair?host=H&port=P&code=C` 的二维码。
- `POST http://host:7643/pair`，请求体 `{code, device_id, device_name}`，返回 `{token, server_name, server_version}`。令牌以哈希形式存储在服务端。
- `GET /health` 用于可达性检查。
- `ws://host:7643/ws?token=...` 升级为 WebSocket，承载与 Unix 套接字 TUI 客户端相同的换行分隔 JSON 协议（`crates/jcode-protocol/src/wire.rs`，`#[serde(tag = "type")]`）。

客户端 v1 请求：`subscribe`、`message`、`cancel`、`soft_interrupt`、`ping`、`get_history`、`resume_session`、`set_model`、`rename_session`、`clear`。

客户端 v1 消费的事件：`ack`、`text_delta`、`reasoning_delta`、`reasoning_done`、`text_replace`、`tool_start`、`tool_input`、`tool_exec`、`tool_done`、`message_end`、`done`、`error`、`pong`、`state`、`session`、`session_renamed`、`history`、`model_changed`、`available_models_updated`、`tokens`、`interrupted`、`status_detail`、`notification`、`compaction`。未知事件类型按设计忽略（向前兼容）。

## 功能范围

v1（本次重建）：

- 通过扫码或手动输入 host/port/code 配对；Keychain 中保存多台服务器
- 连接/断开生命周期，带自动重连与退避
- 聊天：发送、流式助手文本、Markdown 渲染、思考指示器
- 工具调用渲染为可折叠卡片，带实时状态
- 中断（取消）与软中断（运行中排队一条消息）
- 会话列表（来自 history 载荷），通过 `resume_session` 切换、重命名
- 从 `available_models` 选择模型
- 令牌用量与连接状态展示
- 错误与断连横幅，绝不静默丢弃用户输入

后续（明确不在 v1）：推送通知（APNs）、Live Activities、语音输入、图片附件、工具审批 UX、环境模式/集群仪表盘、小组件、Mac Catalyst 打磨。

## 测试策略

1. `swift test`（macOS，无需 Xcode 工程）：
   - 编解码器针对从真实服务器捕获的 fixture JSON 往返校验
   - `SessionReducer` 流式场景（增量、工具生命周期、历史回放、中断、错误、重连后重新订阅）
   - 配对客户端针对桩 URLProtocol
   - connection actor 针对假的 `WebSocketTransport`
2. `xcodebuild build` 构建应用目标（XcodeGen），保持 UI 可编译。
3. 安装模拟器运行时后，通过 `xcrun simctl` 做手动/自动设备验证；不是 CI 门禁。

## CI

macOS 任务在 `ios/` 运行 `swift test`，外加针对应用目标的 `xcodegen` + `xcodebuild build`。TestFlight 交付以后可以再加回（之前的 Codemagic 流水线已随原型移除）。
