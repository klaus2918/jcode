use super::{
    ClientConnectionInfo, SwarmEvent, SwarmEventType, SwarmMember, VersionedPlan,
    broadcast_swarm_plan, broadcast_swarm_status, queue_soft_interrupt_for_session,
    record_swarm_event, truncate_detail, update_member_status,
};
use crate::agent::Agent;
use crate::protocol::{AwaitedMemberStatus, NotificationType, ServerEvent};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_comm_assign_role(
    id: u64,
    req_session_id: String,
    target_session: String,
    role: String,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    sessions: &Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let (swarm_id, is_coordinator) = {
        let members = swarm_members.read().await;
        let swarm_id = members
            .get(&req_session_id)
            .and_then(|member| member.swarm_id.clone());

        let is_coordinator = if let Some(ref sid) = swarm_id {
            let coordinators = swarm_coordinators.read().await;
            let current_coordinator = coordinators.get(sid).cloned();
            drop(coordinators);

            crate::logging::info(&format!(
                "[CommAssignRole] req={} target={} role={} swarm={} current_coord={:?}",
                req_session_id, target_session, role, sid, current_coordinator
            ));

            if current_coordinator.as_deref() == Some(req_session_id.as_str()) {
                true
            } else if role == "coordinator" && target_session == req_session_id {
                drop(members);
                if let Some(ref coord_id) = current_coordinator {
                    let (channel_closed, coord_is_headless) = {
                        let members = swarm_members.read().await;
                        members
                            .get(coord_id)
                            .map(|member| (member.event_tx.is_closed(), member.is_headless))
                            .unwrap_or((true, false))
                    };
                    let not_in_sessions = !sessions.read().await.contains_key(coord_id);
                    channel_closed || not_in_sessions || coord_is_headless
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        };
        (swarm_id, is_coordinator)
    };

    if !is_coordinator {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Only the coordinator can assign roles. (Tip: if the coordinator has disconnected, use assign_role with target_session set to your own session ID to self-promote.)".to_string(),
            retry_after_secs: None,
        });
        return;
    }

    let swarm_id = match swarm_id {
        Some(swarm_id) => swarm_id,
        None => {
            let _ = client_event_tx.send(ServerEvent::Error {
                id,
                message: "Not in a swarm.".to_string(),
                retry_after_secs: None,
            });
            return;
        }
    };

    {
        let mut members = swarm_members.write().await;
        if let Some(member) = members.get_mut(&target_session) {
            member.role = role.clone();
        } else {
            let _ = client_event_tx.send(ServerEvent::Error {
                id,
                message: format!("Unknown session '{}'", target_session),
                retry_after_secs: None,
            });
            return;
        }
    }

    if role == "coordinator" {
        {
            let mut coordinators = swarm_coordinators.write().await;
            coordinators.insert(swarm_id.clone(), target_session.clone());
        }
        let mut members = swarm_members.write().await;
        if let Some(member) = members.get_mut(&req_session_id) {
            if member.session_id != target_session {
                member.role = "agent".to_string();
            }
        }
    }

    broadcast_swarm_status(&swarm_id, swarm_members, swarms_by_id).await;
    record_swarm_event(
        event_history,
        event_counter,
        swarm_event_tx,
        req_session_id,
        None,
        Some(swarm_id),
        SwarmEventType::Notification {
            notification_type: "role_assignment".to_string(),
            message: format!("{} -> {}", target_session, role),
        },
    )
    .await;
    let _ = client_event_tx.send(ServerEvent::Done { id });
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_comm_assign_task(
    id: u64,
    req_session_id: String,
    target_session: String,
    task_id: String,
    message: Option<String>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    sessions: &Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>>,
    soft_interrupt_queues: &super::SessionInterruptQueues,
    client_connections: &Arc<RwLock<HashMap<String, ClientConnectionInfo>>>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let swarm_id = match require_coordinator_swarm(
        id,
        &req_session_id,
        "Only the coordinator can assign tasks.",
        client_event_tx,
        swarm_members,
        swarm_coordinators,
    )
    .await
    {
        Some(swarm_id) => swarm_id,
        None => return,
    };

    let (task_content, participant_ids, plan_item_count) = {
        let mut plans = swarm_plans.write().await;
        let plan = plans
            .entry(swarm_id.clone())
            .or_insert_with(VersionedPlan::new);
        let found = plan.items.iter_mut().find(|item| item.id == task_id);
        if let Some(item) = found {
            item.assigned_to = Some(target_session.clone());
            item.status = "queued".to_string();
            plan.version += 1;
            plan.participants.insert(req_session_id.clone());
            plan.participants.insert(target_session.clone());
            (
                Some(item.content.clone()),
                plan.participants.clone(),
                plan.items.len(),
            )
        } else {
            (None, HashSet::new(), 0)
        }
    };

    let Some(content) = task_content else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: format!("Task '{}' not found in swarm plan", task_id),
            retry_after_secs: None,
        });
        return;
    };

    broadcast_swarm_plan(
        &swarm_id,
        Some("task_assigned".to_string()),
        swarm_plans,
        swarm_members,
        swarms_by_id,
    )
    .await;
    record_swarm_event(
        event_history,
        event_counter,
        swarm_event_tx,
        req_session_id.clone(),
        None,
        Some(swarm_id.clone()),
        SwarmEventType::PlanUpdate {
            swarm_id: swarm_id.clone(),
            item_count: plan_item_count,
        },
    )
    .await;

    let coordinator_name = {
        let members = swarm_members.read().await;
        members
            .get(&req_session_id)
            .and_then(|member| member.friendly_name.clone())
    };
    let notification = if let Some(ref extra) = message {
        format!(
            "Task assigned to you by coordinator: {} — {}",
            content, extra
        )
    } else {
        format!("Task assigned to you by coordinator: {}", content)
    };

    let target_agent = {
        let agent_sessions = sessions.read().await;
        agent_sessions.get(&target_session).cloned()
    };
    let _ = queue_soft_interrupt_for_session(
        &target_session,
        notification.clone(),
        false,
        soft_interrupt_queues,
        sessions,
    )
    .await;
    if let Some(member) = swarm_members.read().await.get(&target_session) {
        let _ = member.event_tx.send(ServerEvent::Notification {
            from_session: req_session_id.clone(),
            from_name: coordinator_name.clone(),
            notification_type: NotificationType::Message {
                scope: Some("dm".to_string()),
                channel: None,
            },
            message: notification,
        });
    }

    let target_has_client = {
        let connections = client_connections.read().await;
        connections
            .values()
            .any(|connection| connection.session_id == target_session)
    };
    if !target_has_client {
        if let Some(agent_arc) = target_agent {
            let target_session_for_run = target_session.clone();
            let swarm_members_for_run = Arc::clone(swarm_members);
            let swarms_for_run = Arc::clone(swarms_by_id);
            let swarm_plans_for_run = Arc::clone(swarm_plans);
            let swarm_id_for_run = swarm_id.clone();
            let task_id_for_run = task_id.clone();
            let event_history_for_run = Arc::clone(event_history);
            let event_counter_for_run = Arc::clone(event_counter);
            let swarm_event_tx_for_run = swarm_event_tx.clone();
            let assignment_text = if let Some(extra) = message.clone() {
                format!(
                    "{}\n\nAdditional coordinator instructions:\n{}",
                    content, extra
                )
            } else {
                content.clone()
            };
            tokio::spawn(async move {
                {
                    let mut plans = swarm_plans_for_run.write().await;
                    if let Some(plan) = plans.get_mut(&swarm_id_for_run) {
                        if let Some(item) = plan
                            .items
                            .iter_mut()
                            .find(|item| item.id == task_id_for_run)
                        {
                            item.status = "running".to_string();
                            plan.version += 1;
                        }
                    }
                }
                broadcast_swarm_plan(
                    &swarm_id_for_run,
                    Some("task_running".to_string()),
                    &swarm_plans_for_run,
                    &swarm_members_for_run,
                    &swarms_for_run,
                )
                .await;
                update_member_status(
                    &target_session_for_run,
                    "running",
                    Some(truncate_detail(&assignment_text, 120)),
                    &swarm_members_for_run,
                    &swarms_for_run,
                    Some(&event_history_for_run),
                    Some(&event_counter_for_run),
                    Some(&swarm_event_tx_for_run),
                )
                .await;

                let result = {
                    let mut agent = agent_arc.lock().await;
                    agent.run_once_capture(&assignment_text).await
                };

                match result {
                    Ok(_) => {
                        {
                            let mut plans = swarm_plans_for_run.write().await;
                            if let Some(plan) = plans.get_mut(&swarm_id_for_run) {
                                if let Some(item) = plan
                                    .items
                                    .iter_mut()
                                    .find(|item| item.id == task_id_for_run)
                                {
                                    item.status = "done".to_string();
                                    plan.version += 1;
                                }
                            }
                        }
                        broadcast_swarm_plan(
                            &swarm_id_for_run,
                            Some("task_completed".to_string()),
                            &swarm_plans_for_run,
                            &swarm_members_for_run,
                            &swarms_for_run,
                        )
                        .await;
                        update_member_status(
                            &target_session_for_run,
                            "completed",
                            None,
                            &swarm_members_for_run,
                            &swarms_for_run,
                            Some(&event_history_for_run),
                            Some(&event_counter_for_run),
                            Some(&swarm_event_tx_for_run),
                        )
                        .await;
                    }
                    Err(error) => {
                        {
                            let mut plans = swarm_plans_for_run.write().await;
                            if let Some(plan) = plans.get_mut(&swarm_id_for_run) {
                                if let Some(item) = plan
                                    .items
                                    .iter_mut()
                                    .find(|item| item.id == task_id_for_run)
                                {
                                    item.status = "failed".to_string();
                                    plan.version += 1;
                                }
                            }
                        }
                        broadcast_swarm_plan(
                            &swarm_id_for_run,
                            Some("task_failed".to_string()),
                            &swarm_plans_for_run,
                            &swarm_members_for_run,
                            &swarms_for_run,
                        )
                        .await;
                        update_member_status(
                            &target_session_for_run,
                            "failed",
                            Some(truncate_detail(&error.to_string(), 120)),
                            &swarm_members_for_run,
                            &swarms_for_run,
                            Some(&event_history_for_run),
                            Some(&event_counter_for_run),
                            Some(&swarm_event_tx_for_run),
                        )
                        .await;
                    }
                }
            });
        }
    }

    let plan_msg = format!(
        "Plan updated: task '{}' assigned to {}.",
        task_id, target_session
    );
    let members = swarm_members.read().await;
    for sid in participant_ids {
        if sid == target_session || sid == req_session_id {
            continue;
        }
        if let Some(member) = members.get(&sid) {
            let _ = member.event_tx.send(ServerEvent::Notification {
                from_session: req_session_id.clone(),
                from_name: coordinator_name.clone(),
                notification_type: NotificationType::Message {
                    scope: Some("plan".to_string()),
                    channel: None,
                },
                message: plan_msg.clone(),
            });
        }
    }

    let _ = client_event_tx.send(ServerEvent::Done { id });
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_comm_await_members(
    id: u64,
    req_session_id: String,
    target_status: Vec<String>,
    requested_ids: Vec<String>,
    timeout_secs: Option<u64>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let swarm_id = {
        let members = swarm_members.read().await;
        members
            .get(&req_session_id)
            .and_then(|member| member.swarm_id.clone())
    };

    if let Some(swarm_id) = swarm_id {
        let watch_ids: Vec<String> = if requested_ids.is_empty() {
            let swarms = swarms_by_id.read().await;
            swarms
                .get(&swarm_id)
                .map(|sessions| {
                    sessions
                        .iter()
                        .filter(|session_id| *session_id != &req_session_id)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        } else {
            requested_ids
        };

        if watch_ids.is_empty() {
            let _ = client_event_tx.send(ServerEvent::CommAwaitMembersResponse {
                id,
                completed: true,
                members: vec![],
                summary: "No other members in swarm to wait for.".to_string(),
            });
            return;
        }

        let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(3600));
        let swarm_members_clone = swarm_members.clone();
        let mut event_rx = swarm_event_tx.subscribe();
        let client_tx = client_event_tx.clone();

        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + timeout;

            loop {
                let (all_done, member_statuses) = {
                    let members = swarm_members_clone.read().await;
                    let statuses: Vec<AwaitedMemberStatus> = watch_ids
                        .iter()
                        .map(|session_id| {
                            let (name, status) = members
                                .get(session_id)
                                .map(|member| (member.friendly_name.clone(), member.status.clone()))
                                .unwrap_or((None, "unknown".to_string()));
                            let done = target_status.contains(&status)
                                || (status == "unknown"
                                    && (target_status.contains(&"stopped".to_string())
                                        || target_status.contains(&"completed".to_string())));
                            AwaitedMemberStatus {
                                session_id: session_id.clone(),
                                friendly_name: name,
                                status,
                                done,
                            }
                        })
                        .collect();
                    (statuses.iter().all(|status| status.done), statuses)
                };

                if all_done {
                    let done_names: Vec<String> = member_statuses
                        .iter()
                        .map(|member| {
                            member.friendly_name.clone().unwrap_or_else(|| {
                                member.session_id[..8.min(member.session_id.len())].to_string()
                            })
                        })
                        .collect();
                    let _ = client_tx.send(ServerEvent::CommAwaitMembersResponse {
                        id,
                        completed: true,
                        members: member_statuses,
                        summary: format!(
                            "All {} members are done: {}",
                            done_names.len(),
                            done_names.join(", ")
                        ),
                    });
                    return;
                }

                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    let pending: Vec<String> = member_statuses
                        .iter()
                        .filter(|member| !member.done)
                        .map(|member| {
                            let name = member.friendly_name.clone().unwrap_or_else(|| {
                                member.session_id[..8.min(member.session_id.len())].to_string()
                            });
                            format!("{} ({})", name, member.status)
                        })
                        .collect();
                    let _ = client_tx.send(ServerEvent::CommAwaitMembersResponse {
                        id,
                        completed: false,
                        members: member_statuses,
                        summary: format!("Timed out. Still waiting on: {}", pending.join(", ")),
                    });
                    return;
                }

                match tokio::time::timeout(remaining, event_rx.recv()).await {
                    Ok(Ok(event)) => {
                        if let SwarmEventType::StatusChange { .. } = &event.event {
                            if watch_ids.contains(&event.session_id) {
                                continue;
                            }
                        }
                        if let SwarmEventType::MemberChange { action } = &event.event {
                            if action == "left" && watch_ids.contains(&event.session_id) {
                                continue;
                            }
                        }
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        let _ = client_tx.send(ServerEvent::CommAwaitMembersResponse {
                            id,
                            completed: false,
                            members: member_statuses,
                            summary: "Server shutting down.".to_string(),
                        });
                        return;
                    }
                    Err(_) => continue,
                }
            }
        });
    } else {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: "Not in a swarm. Use a git repository to enable swarm features.".to_string(),
            retry_after_secs: None,
        });
    }
}

pub(super) async fn handle_client_debug_command(
    id: u64,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    let _ = client_event_tx.send(ServerEvent::Error {
        id,
        message: "ClientDebugCommand is for internal use only".to_string(),
        retry_after_secs: None,
    });
}

pub(super) fn handle_client_debug_response(
    id: u64,
    output: String,
    client_debug_response_tx: &broadcast::Sender<(u64, String)>,
) {
    let _ = client_debug_response_tx.send((id, output));
}

async fn require_coordinator_swarm(
    id: u64,
    req_session_id: &str,
    permission_error: &str,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
) -> Option<String> {
    let (swarm_id, is_coordinator) = {
        let members = swarm_members.read().await;
        let swarm_id = members
            .get(req_session_id)
            .and_then(|member| member.swarm_id.clone());
        let is_coordinator = if let Some(ref swarm_id) = swarm_id {
            let coordinators = swarm_coordinators.read().await;
            coordinators
                .get(swarm_id)
                .map(|coordinator| coordinator == req_session_id)
                .unwrap_or(false)
        } else {
            false
        };
        (swarm_id, is_coordinator)
    };

    if !is_coordinator {
        let _ = client_event_tx.send(ServerEvent::Error {
            id,
            message: permission_error.to_string(),
            retry_after_secs: None,
        });
        return None;
    }

    match swarm_id {
        Some(swarm_id) => Some(swarm_id),
        None => {
            let _ = client_event_tx.send(ServerEvent::Error {
                id,
                message: "Not in a swarm.".to_string(),
                retry_after_secs: None,
            });
            None
        }
    }
}
