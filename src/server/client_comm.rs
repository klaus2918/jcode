use super::{
    queue_soft_interrupt_for_session, record_swarm_event, truncate_detail, FileAccess,
    SessionInterruptQueues, SharedContext, SwarmEvent, SwarmEventType, SwarmMember,
};
use crate::agent::Agent;
use crate::protocol::{AgentInfo, ContextEntry, NotificationType, ServerEvent};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

async fn swarm_id_for_session(
    session_id: &str,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> Option<String> {
    let members = swarm_members.read().await;
    members.get(session_id).and_then(|m| m.swarm_id.clone())
}

async fn friendly_name_for_session(
    session_id: &str,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> Option<String> {
    let members = swarm_members.read().await;
    members
        .get(session_id)
        .and_then(|member| member.friendly_name.clone())
}

pub(super) async fn handle_comm_share(
    id: u64,
    req_session_id: String,
    key: String,
    value: String,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    shared_context: &Arc<RwLock<HashMap<String, HashMap<String, SharedContext>>>>,
    event_history: &Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let swarm_id = swarm_id_for_session(&req_session_id, swarm_members).await;

    if let Some(swarm_id) = swarm_id {
        let friendly_name = friendly_name_for_session(&req_session_id, swarm_members).await;

        {
            let mut ctx = shared_context.write().await;
            let swarm_ctx = ctx.entry(swarm_id.clone()).or_insert_with(HashMap::new);
            let now = Instant::now();
            let created_at = swarm_ctx.get(&key).map(|c| c.created_at).unwrap_or(now);
            swarm_ctx.insert(
                key.clone(),
                SharedContext {
                    key: key.clone(),
                    value: value.clone(),
                    from_session: req_session_id.clone(),
                    from_name: friendly_name.clone(),
                    created_at,
                    updated_at: now,
                },
            );
        }

        let swarm_session_ids: Vec<String> = {
            let swarms = swarms_by_id.read().await;
            swarms
                .get(&swarm_id)
                .map(|sessions| sessions.iter().cloned().collect())
                .unwrap_or_default()
        };

        let members = swarm_members.read().await;
        for sid in &swarm_session_ids {
            if sid != &req_session_id {
                if let Some(member) = members.get(sid) {
                    let _ = member.event_tx.send(ServerEvent::Notification {
                        from_session: req_session_id.clone(),
                        from_name: friendly_name.clone(),
                        notification_type: NotificationType::SharedContext {
                            key: key.clone(),
                            value: value.clone(),
                        },
                        message: format!("Shared context: {} = {}", key, value),
                    });
                }
            }
        }

        record_swarm_event(
            event_history,
            event_counter,
            swarm_event_tx,
            req_session_id.clone(),
            friendly_name.clone(),
            Some(swarm_id.clone()),
            SwarmEventType::ContextUpdate {
                swarm_id: swarm_id.clone(),
                key: key.clone(),
            },
        )
        .await;

        let _ = client_event_tx.send(ServerEvent::Done { id });
    } else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Not in a swarm. Use a git repository to enable swarm features.".to_string(),
            retry_after_secs: None,
        });
    }
}

pub(super) async fn handle_comm_read(
    id: u64,
    req_session_id: String,
    key: Option<String>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    shared_context: &Arc<RwLock<HashMap<String, HashMap<String, SharedContext>>>>,
) {
    let swarm_id = swarm_id_for_session(&req_session_id, swarm_members).await;

    let entries = if let Some(swarm_id) = swarm_id {
        let ctx = shared_context.read().await;
        if let Some(swarm_ctx) = ctx.get(&swarm_id) {
            if let Some(k) = key {
                swarm_ctx
                    .get(&k)
                    .map(|c| {
                        vec![ContextEntry {
                            key: c.key.clone(),
                            value: c.value.clone(),
                            from_session: c.from_session.clone(),
                            from_name: c.from_name.clone(),
                        }]
                    })
                    .unwrap_or_default()
            } else {
                swarm_ctx
                    .values()
                    .map(|c| ContextEntry {
                        key: c.key.clone(),
                        value: c.value.clone(),
                        from_session: c.from_session.clone(),
                        from_name: c.from_name.clone(),
                    })
                    .collect()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let _ = client_event_tx.send(ServerEvent::CommContext { id, entries });
}

pub(super) async fn handle_comm_list(
    id: u64,
    req_session_id: String,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    file_touches: &Arc<RwLock<HashMap<PathBuf, Vec<FileAccess>>>>,
) {
    let swarm_id = swarm_id_for_session(&req_session_id, swarm_members).await;

    if let Some(swarm_id) = swarm_id {
        let swarm_session_ids: Vec<String> = {
            let swarms = swarms_by_id.read().await;
            swarms
                .get(&swarm_id)
                .map(|sessions| sessions.iter().cloned().collect())
                .unwrap_or_default()
        };

        let members = swarm_members.read().await;
        let touches = file_touches.read().await;

        let member_list: Vec<AgentInfo> = swarm_session_ids
            .iter()
            .filter_map(|sid| {
                members.get(sid).map(|member| {
                    let files: Vec<String> = touches
                        .iter()
                        .filter_map(|(path, accesses)| {
                            if accesses.iter().any(|access| &access.session_id == sid) {
                                Some(path.display().to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    AgentInfo {
                        session_id: sid.clone(),
                        friendly_name: member.friendly_name.clone(),
                        files_touched: files,
                        role: Some(member.role.clone()),
                    }
                })
            })
            .collect();

        let _ = client_event_tx.send(ServerEvent::CommMembers {
            id,
            members: member_list,
        });
    } else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Not in a swarm. Use a git repository to enable swarm features.".to_string(),
            retry_after_secs: None,
        });
    }
}

pub(super) async fn handle_comm_subscribe_channel(
    id: u64,
    req_session_id: String,
    channel: String,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    channel_subscriptions: &Arc<RwLock<HashMap<String, HashMap<String, HashSet<String>>>>>,
    event_history: &Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let swarm_id = swarm_id_for_session(&req_session_id, swarm_members).await;

    if let Some(swarm_id) = swarm_id {
        let mut subs = channel_subscriptions.write().await;
        subs.entry(swarm_id.clone())
            .or_default()
            .entry(channel.clone())
            .or_default()
            .insert(req_session_id.clone());
        drop(subs);

        record_swarm_event(
            event_history,
            event_counter,
            swarm_event_tx,
            req_session_id.clone(),
            None,
            Some(swarm_id.clone()),
            SwarmEventType::Notification {
                notification_type: "channel_subscribe".to_string(),
                message: channel.clone(),
            },
        )
        .await;

        let _ = client_event_tx.send(ServerEvent::Done { id });
    } else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Not in a swarm.".to_string(),
            retry_after_secs: None,
        });
    }
}

pub(super) async fn handle_comm_unsubscribe_channel(
    id: u64,
    req_session_id: String,
    channel: String,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    channel_subscriptions: &Arc<RwLock<HashMap<String, HashMap<String, HashSet<String>>>>>,
    event_history: &Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let swarm_id = swarm_id_for_session(&req_session_id, swarm_members).await;

    if let Some(swarm_id) = swarm_id {
        let mut subs = channel_subscriptions.write().await;
        if let Some(swarm_subs) = subs.get_mut(&swarm_id) {
            if let Some(channel_subs) = swarm_subs.get_mut(&channel) {
                channel_subs.remove(&req_session_id);
                if channel_subs.is_empty() {
                    swarm_subs.remove(&channel);
                }
            }
        }
        drop(subs);

        record_swarm_event(
            event_history,
            event_counter,
            swarm_event_tx,
            req_session_id.clone(),
            None,
            Some(swarm_id.clone()),
            SwarmEventType::Notification {
                notification_type: "channel_unsubscribe".to_string(),
                message: channel.clone(),
            },
        )
        .await;

        let _ = client_event_tx.send(ServerEvent::Done { id });
    } else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Not in a swarm.".to_string(),
            retry_after_secs: None,
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_comm_message(
    id: u64,
    from_session: String,
    message: String,
    to_session: Option<String>,
    channel: Option<String>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    sessions: &Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>>,
    soft_interrupt_queues: &SessionInterruptQueues,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    channel_subscriptions: &Arc<RwLock<HashMap<String, HashMap<String, HashSet<String>>>>>,
    event_history: &Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let swarm_id = swarm_id_for_session(&from_session, swarm_members).await;

    if let Some(swarm_id) = swarm_id {
        let friendly_name = friendly_name_for_session(&from_session, swarm_members).await;

        let swarm_session_ids: Vec<String> = {
            let swarms = swarms_by_id.read().await;
            swarms
                .get(&swarm_id)
                .map(|sessions| sessions.iter().cloned().collect())
                .unwrap_or_default()
        };

        if let Some(ref target) = to_session {
            if !swarm_session_ids.contains(target) {
                let _ = client_event_tx.send(ServerEvent::Error {
                    id,
                    message: format!("DM failed: session '{}' not in swarm", target),
                    retry_after_secs: None,
                });
                return;
            }
        }

        let scope = if to_session.is_some() {
            "dm"
        } else if channel.is_some() {
            "channel"
        } else {
            "broadcast"
        };

        let members = swarm_members.read().await;

        let target_sessions: Vec<String> = if let Some(target) = to_session.clone() {
            vec![target]
        } else if let Some(ref channel_name) = channel {
            let subs = channel_subscriptions.read().await;
            if let Some(channel_subs) = subs
                .get(&swarm_id)
                .and_then(|channels| channels.get(channel_name))
            {
                channel_subs
                    .iter()
                    .filter(|session_id| *session_id != &from_session)
                    .cloned()
                    .collect()
            } else {
                swarm_session_ids
                    .iter()
                    .filter(|session_id| *session_id != &from_session)
                    .cloned()
                    .collect()
            }
        } else {
            swarm_session_ids
                .iter()
                .filter(|session_id| *session_id != &from_session)
                .cloned()
                .collect()
        };

        for session_id in &target_sessions {
            if !swarm_session_ids.contains(session_id) {
                continue;
            }
            if let Some(member) = members.get(session_id) {
                let from_label = friendly_name
                    .as_deref()
                    .unwrap_or(&from_session[..8.min(from_session.len())]);
                let scope_label = match (scope, channel.as_deref()) {
                    ("channel", Some(channel_name)) => format!("#{}", channel_name),
                    ("dm", _) => "DM".to_string(),
                    _ => "broadcast".to_string(),
                };
                let notification_msg = format!("{} from {}: {}", scope_label, from_label, message);
                let _ = member.event_tx.send(ServerEvent::Notification {
                    from_session: from_session.clone(),
                    from_name: friendly_name.clone(),
                    notification_type: NotificationType::Message {
                        scope: Some(scope.to_string()),
                        channel: channel.clone(),
                    },
                    message: notification_msg.clone(),
                });

                let _ = queue_soft_interrupt_for_session(
                    session_id,
                    notification_msg.clone(),
                    false,
                    soft_interrupt_queues,
                    sessions,
                )
                .await;
            }
        }

        let scope_value = if scope == "channel" {
            format!("#{}", channel.clone().unwrap_or_default())
        } else {
            scope.to_string()
        };
        record_swarm_event(
            event_history,
            event_counter,
            swarm_event_tx,
            from_session.clone(),
            friendly_name.clone(),
            Some(swarm_id.clone()),
            SwarmEventType::Notification {
                notification_type: scope_value,
                message: truncate_detail(&message, 220),
            },
        )
        .await;

        let _ = client_event_tx.send(ServerEvent::Done { id });
    } else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Not in a swarm. Use a git repository to enable swarm features.".to_string(),
            retry_after_secs: None,
        });
    }
}
