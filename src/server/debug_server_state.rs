use super::{ClientConnectionInfo, ClientDebugState, ServerIdentity, SwarmMember};
use crate::agent::Agent;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

#[allow(clippy::too_many_arguments)]
pub(super) async fn maybe_handle_server_state_command(
    cmd: &str,
    sessions: &Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>>,
    client_connections: &Arc<RwLock<HashMap<String, ClientConnectionInfo>>>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    client_debug_state: &Arc<RwLock<ClientDebugState>>,
    server_identity: &ServerIdentity,
    server_start_time: Instant,
) -> Result<Option<String>> {
    if cmd == "sessions" {
        let sessions_guard = sessions.read().await;
        let members = swarm_members.read().await;
        let connections = client_connections.read().await;
        let connected_sessions: HashSet<String> =
            connections.values().map(|c| c.session_id.clone()).collect();
        let mut out: Vec<serde_json::Value> = Vec::new();
        for (sid, agent_arc) in sessions_guard.iter() {
            if !connected_sessions.contains(sid) {
                continue;
            }
            let member_info = members.get(sid);
            let member_status = member_info.map(|m| m.status.as_str());
            let (provider, model, is_processing, working_dir_str, token_usage): (
                Option<String>,
                Option<String>,
                bool,
                Option<String>,
                Option<serde_json::Value>,
            ) = if let Ok(agent) = agent_arc.try_lock() {
                let usage = agent.last_usage();
                (
                    Some(agent.provider_name()),
                    Some(agent.provider_model()),
                    member_status == Some("running"),
                    agent.working_dir().map(|p| p.to_string()),
                    Some(serde_json::json!({
                        "input": usage.input_tokens,
                        "output": usage.output_tokens,
                        "cache_read": usage.cache_read_input_tokens,
                        "cache_write": usage.cache_creation_input_tokens,
                    })),
                )
            } else {
                (None, None, member_status == Some("running"), None, None)
            };
            let final_working_dir: Option<String> = working_dir_str.or_else(|| {
                member_info.and_then(|m| {
                    m.working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                })
            });
            out.push(serde_json::json!({
                "session_id": sid,
                "friendly_name": member_info.and_then(|m| m.friendly_name.clone()),
                "provider": provider,
                "model": model,
                "is_processing": is_processing,
                "working_dir": final_working_dir,
                "swarm_id": member_info.and_then(|m| m.swarm_id.clone()),
                "status": member_info.map(|m| m.status.clone()),
                "detail": member_info.and_then(|m| m.detail.clone()),
                "token_usage": token_usage,
                "server_name": server_identity.name,
                "server_icon": server_identity.icon,
            }));
        }
        return Ok(Some(
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "[]".to_string()),
        ));
    }

    if cmd == "background" || cmd == "background:tasks" {
        let tasks = crate::background::global().list().await;
        return Ok(Some(
            serde_json::json!({
                "count": tasks.len(),
                "tasks": tasks,
            })
            .to_string(),
        ));
    }

    if cmd == "info" || cmd == "server:info" {
        let uptime_secs = server_start_time.elapsed().as_secs();
        let session_count = sessions.read().await.len();
        let member_count = swarm_members.read().await.len();
        let has_update = super::server_has_newer_binary();
        return Ok(Some(
            serde_json::json!({
                "id": server_identity.id,
                "name": server_identity.name,
                "icon": server_identity.icon,
                "version": server_identity.version,
                "git_hash": server_identity.git_hash,
                "uptime_secs": uptime_secs,
                "session_count": session_count,
                "swarm_member_count": member_count,
                "has_update": has_update,
                "debug_control_enabled": super::debug_control_allowed(),
            })
            .to_string(),
        ));
    }

    if cmd == "clients:map" || cmd == "clients:mapping" {
        let connections = client_connections.read().await;
        let members = swarm_members.read().await;
        let mut out: Vec<serde_json::Value> = Vec::new();
        for info in connections.values() {
            let member = members.get(&info.session_id);
            out.push(serde_json::json!({
                "client_id": info.client_id,
                "session_id": info.session_id,
                "friendly_name": member.and_then(|m| m.friendly_name.clone()),
                "working_dir": member.and_then(|m| m.working_dir.clone()),
                "swarm_id": member.and_then(|m| m.swarm_id.clone()),
                "status": member.map(|m| m.status.clone()),
                "detail": member.and_then(|m| m.detail.clone()),
                "connected_secs_ago": info.connected_at.elapsed().as_secs(),
                "last_seen_secs_ago": info.last_seen.elapsed().as_secs(),
            }));
        }
        return Ok(Some(
            serde_json::json!({
                "count": out.len(),
                "clients": out,
            })
            .to_string(),
        ));
    }

    if cmd == "clients" {
        let debug_state = client_debug_state.read().await;
        let client_ids: Vec<&String> = debug_state.clients.keys().collect();
        return Ok(Some(
            serde_json::json!({
                "count": debug_state.clients.len(),
                "active_id": debug_state.active_id,
                "client_ids": client_ids,
            })
            .to_string(),
        ));
    }

    Ok(None)
}
