//! 后台会话跟踪器
//!
//! 当用户在 TUI 中切换 session 时，如果原 session 的 Agent 正在执行 turn，
//! 不再强制清理，而是将其注册为后台会话继续执行。
//! 完成后通过通知通道告知 TUI。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// 单个后台会话的元数据
#[derive(Debug, Clone)]
pub(super) struct BackgroundSessionInfo {
    /// session ID
    pub session_id: String,
    /// 切换为后台的时间
    pub moved_to_background_at: Instant,
    /// 关联的友好名称（如有）
    pub friendly_name: Option<String>,
}

/// 后台会话完成事件，通过通道通知 TUI
#[derive(Debug, Clone)]
pub struct BackgroundCompletionEvent {
    pub session_id: String,
    pub friendly_name: Option<String>,
    pub duration: Duration,
}

/// 进程级后台会话跟踪器（全局单例）
///
/// 与 `BACKGROUND_TOOL_SIGNALS` 类似，使用 `StdMutex` 保护，
/// 仅存储轻量元数据，Agent 的 `Arc<Mutex<Agent>>` 由 `SessionAgents` 持有。
static TRACKER: LazyLock<StdMutex<BackgroundSessionTrackerInner>> =
    LazyLock::new(|| StdMutex::new(BackgroundSessionTrackerInner::new()));

struct BackgroundSessionTrackerInner {
    /// session_id -> 元数据
    sessions: HashMap<String, BackgroundSessionInfo>,
    /// 完成通知发送端
    completion_tx: Option<mpsc::UnboundedSender<BackgroundCompletionEvent>>,
}

impl BackgroundSessionTrackerInner {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            completion_tx: None,
        }
    }
}

/// 初始化后台会话跟踪器，返回完成事件接收端。
///
/// 应在 server 启动时调用一次。
pub(super) fn init_background_session_tracker() -> mpsc::UnboundedReceiver<BackgroundCompletionEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    if let Ok(mut tracker) = TRACKER.lock() {
        tracker.completion_tx = Some(tx);
    }
    rx
}

/// 将 session 注册为后台会话。
///
/// 当 Agent 仍在执行 turn 时（Mutex 被锁定），切换 session 会调用此函数，
/// 保留 session 在 `SessionAgents` 中不被清理。
pub(super) fn register_background_session(
    session_id: &str,
    friendly_name: Option<String>,
) {
    if let Ok(mut tracker) = TRACKER.lock() {
        crate::logging::info(&format!(
            "BACKGROUND_SESSION: registered {} (name={:?})",
            session_id, friendly_name
        ));
        let info = BackgroundSessionInfo {
            session_id: session_id.to_string(),
            moved_to_background_at: Instant::now(),
            friendly_name,
        };
        tracker.sessions.insert(session_id.to_string(), info);
    }
}

/// 移除后台会话注册（完成后或恢复前台时调用）
pub(super) fn unregister_background_session(session_id: &str) {
    if let Ok(mut tracker) = TRACKER.lock() {
        if tracker.sessions.remove(session_id).is_some() {
            crate::logging::info(&format!(
                "BACKGROUND_SESSION: unregistered {}",
                session_id
            ));
        }
    }
}

/// 检查 session 是否为后台会话
pub(super) fn is_background_session(session_id: &str) -> bool {
    TRACKER
        .lock()
        .map(|tracker| tracker.sessions.contains_key(session_id))
        .unwrap_or(false)
}

/// 获取所有后台会话的信息快照
pub fn list_background_sessions() -> Vec<BackgroundSessionInfo> {
    TRACKER
        .lock()
        .map(|tracker| tracker.sessions.values().cloned().collect())
        .unwrap_or_default()
}

/// 发送完成通知（由监控任务调用）
pub(super) fn notify_completion(event: BackgroundCompletionEvent) {
    if let Ok(tracker) = TRACKER.lock() {
        if let Some(tx) = &tracker.completion_tx {
            let _ = tx.send(event);
        }
    }
}

/// 清理所有后台会话记录（server 关闭时调用）
pub(super) fn clear_all_background_sessions() {
    if let Ok(mut tracker) = TRACKER.lock() {
        let count = tracker.sessions.len();
        tracker.sessions.clear();
        if count > 0 {
            crate::logging::info(&format!(
                "BACKGROUND_SESSION: cleared {} sessions on shutdown",
                count
            ));
        }
    }
}

/// 启动后台会话完成监控任务。
///
/// 每 2 秒检查一次所有后台 session 的 Agent 是否已完成 turn。
/// 当 Agent 的 Mutex 可以成功锁定时，说明 turn 已完成。
pub(super) fn spawn_background_session_monitor(
    sessions: crate::server::SessionAgents,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;

            let session_ids: Vec<String> = {
                match TRACKER.lock() {
                    Ok(tracker) => tracker.sessions.keys().cloned().collect(),
                    Err(_) => continue,
                }
            };

            if session_ids.is_empty() {
                continue;
            }

            // 从 SessionAgents 中获取 Agent 引用，检查是否完成
            let agents_snapshot = {
                match sessions.read().await {
                    guard => guard.clone(),
                }
            };

            for session_id in &session_ids {
                let agent_arc = match agents_snapshot.get(session_id) {
                    Some(arc) => arc.clone(),
                    None => {
                        // Agent 已从 SessionAgents 移除（可能被其他路径清理）
                        // 移除后台注册
                        unregister_background_session(session_id);
                        continue;
                    }
                };

                // 尝试锁定 Agent（try_lock 是非阻塞的）
                // 如果成功锁定，说明 turn 已完成（Agent 空闲）
                match agent_arc.try_lock() {
                    Ok(mut agent) => {
                        // Agent 空闲，turn 已完成
                        let info = {
                            TRACKER
                                .lock()
                                .ok()
                                .and_then(|mut tracker| {
                                    tracker.sessions.remove(session_id)
                                })
                        };

                        if let Some(info) = info {
                            let duration = info.moved_to_background_at.elapsed();
                            crate::logging::info(&format!(
                                "BACKGROUND_SESSION: completed {} (took {:?})",
                                session_id, duration
                            ));

                            // 标记 session 为已关闭并持久化状态
                            agent.mark_closed();

                            // 发送完成通知
                            notify_completion(BackgroundCompletionEvent {
                                session_id: session_id.clone(),
                                friendly_name: info.friendly_name,
                                duration,
                            });
                        }
                    }
                    Err(_) => {
                        // try_lock 失败 — Agent 仍在执行中，继续等待
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // 所有测试合并为一个，避免并行执行时共享全局 TRACKER 的竞争条件。
    #[test]
    fn tracker_lifecycle() {
        // --- 清理初始状态 ---
        if let Ok(mut tracker) = TRACKER.lock() {
            tracker.sessions.clear();
            tracker.completion_tx = None;
        }

        // --- register + list ---
        register_background_session("sess-1", Some("优化查询".to_string()));
        register_background_session("sess-2", None);

        let list = list_background_sessions();
        assert_eq!(list.len(), 2);
        let names: Vec<Option<&str>> = list
            .iter()
            .map(|info| info.friendly_name.as_deref())
            .collect();
        assert!(names.contains(&Some("优化查询")));
        assert!(names.contains(&None));

        // --- unregister ---
        assert!(is_background_session("sess-1"));
        unregister_background_session("sess-1");
        assert!(!is_background_session("sess-1"));
        unregister_background_session("nonexistent");

        // --- is_background_session ---
        assert!(!is_background_session("sess-x"));
        register_background_session("sess-x", None);
        assert!(is_background_session("sess-x"));
        unregister_background_session("sess-x");
        assert!(!is_background_session("sess-x"));

        // --- clear_all ---
        register_background_session("a", None);
        register_background_session("b", None);
        register_background_session("c", None);
        // sess-2 从前面注册段遗留，加上 a/b/c 共 4 个
        assert_eq!(list_background_sessions().len(), 4);
        clear_all_background_sessions();
        assert_eq!(list_background_sessions().len(), 0);

        // --- register 替换已有条目 ---
        register_background_session("sess-1", Some("旧名".to_string()));
        register_background_session("sess-1", Some("新名".to_string()));
        let list = list_background_sessions();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].friendly_name.as_deref(), Some("新名"));

        // --- notify_completion 通道 ---
        let mut rx = init_background_session_tracker();
        notify_completion(BackgroundCompletionEvent {
            session_id: "sess-done".to_string(),
            friendly_name: Some("测试任务".to_string()),
            duration: Duration::from_secs(5),
        });
        let event = rx.try_recv().unwrap();
        assert_eq!(event.session_id, "sess-done");
        assert_eq!(event.friendly_name.as_deref(), Some("测试任务"));
        assert_eq!(event.duration, Duration::from_secs(5));

        // --- list 为空 ---
        clear_all_background_sessions();
        assert!(list_background_sessions().is_empty());

        // --- 清理 ---
        if let Ok(mut tracker) = TRACKER.lock() {
            tracker.sessions.clear();
            tracker.completion_tx = None;
        }
    }
}
