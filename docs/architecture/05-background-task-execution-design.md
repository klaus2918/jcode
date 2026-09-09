# jcode 后台任务执行架构设计

> 核心目标：session 切换时，原 session 的 Agent 任务不中断，在 server 端继续执行，完成后通知用户。

## 1. 问题分析

### 1.1 当前切换流程（代码级）

```
TUI 按 session picker 切换到 session B
    │
    ▼
TUI 发送: Resume { target_session_id: "B" }
    │
    ▼
server: handle_resume_session()
    │
    ├─ 1. cleanup_detached_source_session_if_unused(旧 session A)
    │      │
    │      ├─ remove_detached_source_if_unclaimed()
    │      │   └─ sessions_guard.remove("A")  ← 从 SessionAgents 移除
    │      │
    │      ├─ agent_guard.lock().await
    │      │   └─ agent_guard.mark_closed()   ← 标记关闭
    │      │
    │      ├─ signals.remove("A")             ← 清理中断信号
    │      ├─ remove_background_tool_signal   ← 清理后台工具信号
    │      ├─ remove_session_interrupt_queue  ← 清理中断队列
    │      ├─ remove_session_channel_subscriptions
    │      ├─ file_touch.clear_session
    │      └─ remove_session_from_swarm       ← 从 swarm 移除
    │
    ├─ 2. claim_live_target_agent("B")        ← 获取新 session 的 Agent
    │
    └─ 3. handle_subscribe("B")               ← 订阅新 session 事件
```

**问题**：步骤 1 中，即使 Agent A 还在 `Mutex` 内执行（`try_lock().is_err()`），也会被 `mark_closed()` 并移除。

### 1.2 关键代码路径

| 文件 | 函数 | 作用 |
|------|------|------|
| `client_session.rs:1013` | `cleanup_detached_source_session_if_unused` | 切换时清理旧 session |
| `client_session.rs:1085` | `remove_detached_source_if_unclaimed` | 从 SessionAgents 移除旧 Agent |
| `client_disconnect_cleanup.rs:54` | `cleanup_client_connection` | 断连时清理（同样问题） |
| `client_lifecycle.rs:338` | `handle_client` | 主循环，处理 Resume 请求 |

### 1.3 Agent 忙碌判断

```rust
// client_session.rs:1219
let live_target_busy = live_target_agent.try_lock().is_err();
```

当 Agent 在 `Mutex` 内执行 turn 时，`try_lock()` 失败 → `live_target_busy = true`。
但当前代码**没有**利用这个信息来保留旧 session。

## 2. 设计方案

### 2.1 核心改动：切换时保留忙碌的旧 session

```
TUI 切换到 session B
    │
    ▼
handle_resume_session()
    │
    ├─ 1. 检查旧 session A 的 Agent 状态
    │      └─ agent.try_lock() → 如果失败（忙碌），标记为 Background，保留
    │
    ├─ 2. 如果旧 Agent 空闲 → 执行现有清理逻辑（不变）
    │
    └─ 3. 如果旧 Agent 忙碌 →
           ├─ 不从 SessionAgents 移除
           ├─ 不 mark_closed()
           ├─ 不清理信号/订阅
           ├─ 仅：取消 TUI 事件订阅（unregister_event_sender）
           └─ 注册到 BackgroundSessionTracker
```

### 2.2 新增 BackgroundSessionTracker

```rust
// crates/jcode-app-core/src/server/background_session.rs (新增文件)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc};

/// 后台会话跟踪器，管理切换后继续运行的 session
pub struct BackgroundSessionTracker {
    /// session_id -> 后台会话元数据
    sessions: HashMap<String, BackgroundSessionInfo>,
    /// 完成通知发送端（通知 TUI）
    completion_tx: mpsc::UnboundedSender<BackgroundCompletionEvent>,
}

pub struct BackgroundSessionInfo {
    pub session_id: String,
    /// Agent 的 Arc 引用（与 SessionAgents 中共享）
    pub agent: Arc<Mutex<Agent>>,
    /// 切换为后台的时间
    pub moved_to_background_at: chrono::DateTime<chrono::Utc>,
    /// 关联的 swarm 信息（保留用于恢复）
    pub swarm_id: Option<String>,
    pub friendly_name: Option<String>,
}

pub struct BackgroundCompletionEvent {
    pub session_id: String,
    pub title: Option<String>,
    pub summary: String,
    pub duration_secs: f64,
}
```

### 2.3 后台会话完成通知机制

Agent turn 完成后的通知路径：

```
Agent turn 结束（在 server 端的 turn 循环中）
    │
    ├─ 检查该 session 是否在 BackgroundSessionTracker 中
    │
    ├─ 如果是后台 session:
    │   ├─ 1. 持久化 session 结果（现有逻辑不变）
    │   ├─ 2. 发送 BackgroundCompletionEvent 到通知队列
    │   ├─ 3. 从 BackgroundSessionTracker 移除
    │   └─ 4. session 保留在 SessionAgents 中（用户可恢复查看）
    │
    └─ 如果是前台 session:
        └─ 执行现有逻辑（不变）
```

### 2.4 通知队列与 TUI 集成

```
BackgroundSessionTracker
    │
    ├─ completion_tx ──► mpsc::UnboundedReceiver
    │                        │
    │                        ▼
    │               TUI 主循环中 select! 监听
    │                        │
    │                        ▼
    │               显示通知弹窗:
    │               "🔄 Session A 完成了: 重构登录模块 (2m30s)"
    │               [Enter] 查看  [Esc] 稍后
    │
    └─ 定期轮询（每 5 秒）:
        └─ 检查后台 session 是否完成
           └─ 如果完成 → 发送通知
```

### 2.5 从后台恢复到前台

```
用户在 session picker 选择后台 session A
    │
    ▼
handle_resume_session("A")
    │
    ├─ 1. 检查是否在 BackgroundSessionTracker 中
    │      └─ 是 → 从 tracker 移除
    │
    ├─ 2. 检查 Agent 状态
    │      ├─ 如果已完成 → 正常恢复，显示结果
    │      └─ 如果还在执行 → 重新建立 TUI 事件订阅，实时显示进度
    │
    └─ 3. 恢复完成
```

### 2.6 断连时的处理

当前 `cleanup_client_connection` 在断连时也会清理 Agent。需要区分：

```
断连场景:
├── TUI 主动关闭 → 检查是否需要保留 Agent（如果忙碌）
├── TUI 崩溃 → 同上
└── 服务器重载 → 保持现有逻辑（强制清理所有）
```

改动 `cleanup_client_connection`:

```rust
// 新增判断：如果 Agent 正在执行 turn，保留它
if disconnected_while_processing {
    // 不移除 session，不 mark_closed
    // 仅清理 TUI 相关的订阅
    unregister_session_event_sender(swarm_members, session_id, client_id).await;
    // 注册到 BackgroundSessionTracker
    register_background_session(session_id, agent_arc.clone(), ...);
} else {
    // 执行现有清理逻辑
}
```

## 3. 协议扩展

### 3.1 新增 Request

```rust
pub enum Request {
    // ... 现有类型

    /// 获取后台会话列表
    GetBackgroundSessions,

    /// 清理已完成的后台会话记录
    CleanupBackgroundSessions {
        max_age_hours: Option<u64>,
        dry_run: Option<bool>,
    },
}
```

### 3.2 新增 ServerEvent

```rust
pub enum ServerEvent {
    // ... 现有类型

    /// 后台会话完成通知
    BackgroundSessionCompleted {
        session_id: String,
        title: Option<String>,
        summary: String,
        duration_secs: f64,
    },

    /// 后台会话列表更新
    BackgroundSessionsList {
        sessions: Vec<BackgroundSessionSnapshot>,
    },
}
```

## 4. TUI 变更

### 4.1 Session Picker 增强

在 Session Picker 中显示后台 session 状态：

```
┌─────────────────────────────────────────────────────┐
│ Sessions                                    [All ▼] │
├─────────────────────────────────────────────────────┤
│  ▶ session-b7c1  "优化查询性能"          ← 当前前台  │
│  🔄 session-a3f2  "重构登录模块"    Running 2m30s   │
│  ✓  session-d4e5  "更新文档"           Done  0m45s  │
│  ●  session-f2a1  "新功能开发"          ← 可选择    │
└─────────────────────────────────────────────────────┘
```

### 4.2 通知弹窗

```
┌──────────────────────────────────────┐
│  🔄 后台任务完成                      │
│                                      │
│  Session A: 重构登录模块              │
│  耗时: 2m30s                         │
│                                      │
│  [Enter] 查看  [Esc] 稍后            │
└──────────────────────────────────────┘
```

### 4.3 后台会话面板（可选）

新增侧边栏面板，显示所有后台 session 的状态：

```
┌─────────────────────────────────────┐
│ 🔄 Background Sessions    [2 active]│
├─────────────────────────────────────┤
│  ▶ a3f2 "重构登录模块"  Running 2m30│
│    └─ 最后: cargo build             │
│  ✓ d4e5 "更新文档"      Done  0m45 │
│    └─ 修改了 3 个文件               │
├─────────────────────────────────────┤
│ [Enter] 切换  [s] 停止  [Esc] 返回 │
└─────────────────────────────────────┘
```

## 5. 实现计划

### Phase 1: 核心逻辑（2-3 天）

改动文件：
- `crates/jcode-app-core/src/server/background_session.rs` — 新增 BackgroundSessionTracker
- `crates/jcode-app-core/src/server/client_session.rs` — 修改 `cleanup_detached_source_session_if_unused`
- `crates/jcode-app-core/src/server/client_disconnect_cleanup.rs` — 修改 `cleanup_client_connection`

核心改动：
1. `cleanup_detached_source_session_if_unused` 中检查 Agent 是否忙碌
2. 如果忙碌，不移除 session，注册到 BackgroundSessionTracker
3. Agent turn 完成后检查是否需要通知

### Phase 2: 通知与恢复（1-2 天）

改动文件：
- `crates/jcode-app-core/src/server/client_lifecycle.rs` — 处理新的 Request
- `crates/jcode-protocol/src/lib.rs` — 新增 Request/Event 类型

核心改动：
1. Agent turn 完成时检查 BackgroundSessionTracker
2. 发送完成通知
3. 处理 GetBackgroundSessions / CleanupBackgroundSessions

### Phase 3: TUI 集成（2-3 天）

改动文件：
- `crates/jcode-tui-session-picker/` — 新增后台状态显示
- `crates/jcode-tui/` — 新增通知弹窗组件

核心改动：
1. Session Picker 显示后台 session 状态
2. 监听 BackgroundSessionCompleted 事件
3. 显示通知弹窗

## 6. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 多个 Agent 并发占用内存 | 限制最大后台数（默认 3） |
| Agent 并发写同一文件 | 依赖现有的文件锁机制 |
| 后台 Agent 长时间不完成 | 设置最大执行时间（30 分钟） |
| 进程退出时后台 Agent 状态丢失 | graceful_shutdown 持久化状态 |

## 7. 向后兼容

- 现有的 session 切换行为不变（空闲 session 切换）
- 仅对忙碌 session 的切换行为做增强
- 新增的 Request/Event 不影响旧版 TUI
- 配置项使用合理默认值
