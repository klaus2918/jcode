use super::debug_ambient::maybe_handle_ambient_command;
use super::debug_command_exec::{execute_debug_command, resolve_debug_session};
use super::debug_events::{
    maybe_handle_event_query_command, maybe_handle_event_subscription_command,
};
use super::debug_jobs::{maybe_handle_job_command, DebugJob};
use super::debug_server_state::maybe_handle_server_state_command;
use super::debug_session_admin::maybe_handle_session_admin_command;
use super::debug_swarm_read::maybe_handle_swarm_read_command;
use super::debug_swarm_write::maybe_handle_swarm_write_command;
use super::debug_testers::execute_tester_command;
use super::{
    debug_control_allowed, FileAccess, ServerIdentity, SharedContext, SwarmEvent, SwarmMember,
    VersionedPlan,
};
use crate::agent::Agent;
use crate::ambient_runner::AmbientRunnerHandle;
use crate::protocol::{decode_request, encode_event, Request, ServerEvent};
use crate::provider::Provider;
use crate::transport::Stream;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

#[derive(Default)]
pub(super) struct ClientDebugState {
    pub(super) active_id: Option<String>,
    pub(super) clients: HashMap<String, mpsc::UnboundedSender<(u64, String)>>,
}

#[derive(Clone, Debug)]
pub(super) struct ClientConnectionInfo {
    pub(super) client_id: String,
    pub(super) session_id: String,
    pub(super) connected_at: Instant,
    pub(super) last_seen: Instant,
}

impl ClientDebugState {
    pub(super) fn register(&mut self, client_id: String, tx: mpsc::UnboundedSender<(u64, String)>) {
        self.active_id = Some(client_id.clone());
        self.clients.insert(client_id, tx);
    }

    pub(super) fn unregister(&mut self, client_id: &str) {
        self.clients.remove(client_id);
        if self.active_id.as_deref() == Some(client_id) {
            self.active_id = self.clients.keys().next().cloned();
        }
    }

    pub(super) fn active_sender(
        &mut self,
    ) -> Option<(String, mpsc::UnboundedSender<(u64, String)>)> {
        if let Some(active_id) = self.active_id.clone() {
            if let Some(tx) = self.clients.get(&active_id) {
                return Some((active_id, tx.clone()));
            }
        }
        if let Some((id, tx)) = self.clients.iter().next() {
            let id = id.clone();
            self.active_id = Some(id.clone());
            return Some((id, tx.clone()));
        }
        None
    }
}

/// Parse namespaced debug command (e.g., "server:state", "client:frame", "tester:list")
pub(super) fn parse_namespaced_command(command: &str) -> (&str, &str) {
    let trimmed = command.trim();
    if let Some(idx) = trimmed.find(':') {
        let namespace = &trimmed[..idx];
        let rest = &trimmed[idx + 1..];
        // Only recognize known namespaces
        match namespace {
            "server" | "client" | "tester" => (namespace, rest),
            _ => ("server", trimmed), // Default to server namespace
        }
    } else {
        ("server", trimmed) // No namespace = server
    }
}

/// Generate help text for debug commands
pub(super) fn debug_help_text() -> String {
    r#"Debug socket commands (namespaced):

SERVER COMMANDS (server: prefix or no prefix):
  state                    - Get agent state
  history                  - Get conversation history
  tools                    - List available tools (names only)
  tools:full               - List tools with full definitions (input_schema)
  mcp:servers              - List configured + connected MCP servers
  last_response            - Get last assistant response
  message:<text>           - Send message to agent
  message_async:<text>     - Send message async (returns job id)
  swarm_message:<text>     - Plan and run subtasks via swarm workers, then integrate
  swarm_message_async:<text> - Async swarm message (returns job id)
  tool:<name> <json>       - Execute tool directly
  cancel                   - Cancel in-flight generation (urgent interrupt)
  clear                    - Clear conversation history
  agent:info               - Get comprehensive agent internal state
  jobs                     - List async debug jobs
  job_status:<id>          - Get async job status/output
  job_wait:<id>            - Wait for async job to finish
  job_cancel:<id>          - Cancel a running job
  jobs:purge               - Remove completed/failed jobs
  jobs:session:<id>        - List jobs for a session
  background:tasks         - List background tasks
  sessions                 - List all sessions (with full metadata)
  clients                  - List connected TUI clients
  clients:map              - Map connected clients to sessions
  server:info              - Server identity, health, uptime
  swarm                    - List swarm members + status (alias: swarm:members)
  swarm:help               - Full swarm command reference
  create_session           - Create headless session
  create_session:<path>    - Create session with working dir
  destroy_session:<id>     - Destroy a session
  set_model:<model>        - Switch model (may change provider)
  set_provider:<name>      - Switch provider (claude/openai/openrouter/cursor/copilot/antigravity)
  trigger_extraction       - Force end-of-session memory extraction
  available_models         - List all available models
  reload                   - Trigger server reload with current binary

SWARM COMMANDS (swarm: prefix):
  swarm:members            - List all swarm members with details
  swarm:list               - List all swarms with member counts
  swarm:info:<swarm_id>    - Full info for a swarm
  swarm:coordinators       - List all coordinators
  swarm:roles              - List all members with roles
  swarm:plans              - List all swarm plans
  swarm:plan_version:<id>  - Show plan version for a swarm
  swarm:proposals          - List pending plan proposals
  swarm:context            - List all shared context
  swarm:touches            - List all file touches
  swarm:conflicts          - Files touched by multiple sessions
  swarm:channels           - List channel subscriptions
  swarm:broadcast:<msg>    - Broadcast to swarm members
  swarm:notify:<sid> <msg> - Send DM to specific session
  swarm:help               - Full swarm command reference

AMBIENT COMMANDS (ambient: prefix):
  ambient:status              - Current ambient state, cycle count, last run
  ambient:queue               - Scheduled queue contents
  ambient:trigger             - Manually trigger an ambient cycle
  ambient:log                 - Recent transcript summaries
  ambient:permissions         - List pending permission requests
  ambient:approve:<id>        - Approve a permission request
  ambient:deny:<id> [reason]  - Deny a permission request (optional reason)
  ambient:start               - Start/restart ambient mode
  ambient:stop                - Stop ambient mode
  ambient:help                - Ambient command reference

EVENTS COMMANDS (events: prefix):
  events:recent            - Get recent events (default 50)
  events:recent:<N>        - Get recent N events
  events:since:<id>        - Get events since event ID
  events:count             - Event count and latest ID
  events:types             - List available event types
  events:subscribe         - Subscribe to all events (streaming)
  events:subscribe:<types> - Subscribe filtered (e.g. status_change,member_change)

CLIENT COMMANDS (client: prefix):
  client:state             - Get TUI state
  client:frame             - Get latest visual debug frame (JSON)
  client:frame-normalized  - Get normalized frame (for diffs)
  client:screen            - Dump visual debug to file
  client:layout            - Get latest layout JSON
  client:margins           - Get layout margins JSON
  client:widgets           - Get info widget summary/placements
  client:render-stats      - Get render timing + order JSON
  client:render-order      - Get render order list
  client:anomalies         - Get latest visual debug anomalies
  client:theme             - Get palette snapshot
  client:mermaid:stats     - Get mermaid render/cache stats
  client:mermaid:memory    - Mermaid memory profile (RSS + cache estimates)
  client:mermaid:memory-bench [n] - Synthetic Mermaid memory benchmark
  client:mermaid:cache     - List mermaid cache entries
  client:mermaid:state     - Get image state (resize modes)
  client:mermaid:test      - Render test diagram
  client:mermaid:scroll    - Run scroll simulation test
  client:mermaid:render <c> - Render arbitrary mermaid
  client:mermaid:evict     - Clear mermaid cache
  client:markdown:stats    - Get markdown render stats
  client:overlay:on/off    - Toggle overlay boxes
  client:input             - Get current input buffer
  client:set_input:<text>  - Set input buffer
  client:keys:<keyspec>    - Inject key events
  client:message:<text>    - Inject and submit message
  client:inject:<role>:<t> - Inject display message (no send)
  client:scroll:<dir>      - Scroll (up/down/top/bottom)
  client:scroll-test[:<j>] - Run offscreen scroll+diagram test
  client:scroll-suite[:<j>] - Run scroll+diagram test suite
  client:wait              - Check if processing
  client:history           - Get display messages
  client:help              - Client command help

TESTER COMMANDS (tester: prefix):
  tester:spawn             - Spawn new tester instance
  tester:list              - List active testers
  tester:<id>:frame        - Get frame from tester
  tester:<id>:message:<t>  - Send message to tester
  tester:<id>:inject:<t>   - Inject display message (no send)
  tester:<id>:state        - Get tester state
  tester:<id>:scroll-test  - Run offscreen scroll+diagram test
  tester:<id>:scroll-suite - Run scroll+diagram test suite
  tester:<id>:stop         - Stop tester

Examples:
  {"type":"debug_command","id":1,"command":"state"}
  {"type":"debug_command","id":2,"command":"client:frame"}
  {"type":"debug_command","id":3,"command":"tester:list"}
  {"type":"debug_command","id":4,"command":"set_provider:openai","session_id":"..."}
  {"type":"debug_command","id":5,"command":"swarm:info:/home/user/project"}"#
        .to_string()
}

/// Generate help text for swarm debug commands
pub(super) fn swarm_debug_help_text() -> String {
    r#"Swarm debug commands (swarm: prefix):

MEMBERS & STRUCTURE:
  swarm                    - List all swarm members (alias for swarm:members)
  swarm:members            - List all swarm members with full details
  swarm:list               - List all swarm IDs with member counts and coordinators
  swarm:info:<swarm_id>    - Full info: members, coordinator, plan, context, conflicts

COORDINATORS & ROLES:
  swarm:coordinators            - List all coordinators (swarm_id -> session_id)
  swarm:coordinator:<id>        - Get coordinator for specific swarm
  swarm:clear_coordinator:<id>  - Admin: forcibly clear coordinator so any session can self-promote
  swarm:roles                   - List all members with their roles

PLANS (server-scoped plan items):
  swarm:plans              - List all swarm plans with item counts
  swarm:plan:<swarm_id>    - Get plan items for specific swarm
  swarm:plan_version:<id>  - Show current plan version for a swarm

PLAN PROPOSALS (pending approval):
  swarm:proposals          - List all pending proposals across swarms
  swarm:proposals:<swarm>  - List proposals for a specific swarm (with items)
  swarm:proposals:<sess>   - Get detailed proposal from a session

SHARED CONTEXT (key-value store):
  swarm:context            - List all shared context entries
  swarm:context:<swarm_id> - List context for specific swarm
  swarm:context:<swarm_id>:<key> - Get specific context value

FILE TOUCHES (conflict detection):
  swarm:touches            - List all file touches (path, session, op, age, timestamp)
  swarm:touches:<path>     - Get touches for specific file
  swarm:touches:swarm:<id> - Get touches filtered by swarm members
  swarm:conflicts          - List files touched by multiple sessions

NOTIFICATIONS:
  swarm:broadcast:<msg>    - Broadcast message to all members of your swarm
  swarm:broadcast:<swarm_id> <msg> - Broadcast to specific swarm
  swarm:notify:<session_id> <msg> - Send direct message to specific session

EXECUTION STATE:
  swarm:session:<id>       - Detailed session state (interrupts, provider, usage)
  swarm:interrupts         - List pending interrupts across all sessions

CHANNELS:
  swarm:channels           - List channel subscriptions per swarm

OPERATIONS (debug-only, bypass tool:communicate):
  swarm:set_context:<sess> <key> <value> - Set shared context as session
  swarm:approve_plan:<coord> <proposer>  - Approve plan proposal (coordinator only)
  swarm:reject_plan:<coord> <proposer> [reason] - Reject plan proposal

UTILITIES:
  swarm:id:<path>          - Compute swarm_id for a path and show provenance

REAL-TIME EVENTS:
  events:recent            - Get recent 50 events
  events:recent:<N>        - Get recent N events
  events:since:<id>        - Get events since event ID (for polling)
  events:count             - Get event count and latest ID
  events:types             - List available event types
  events:subscribe         - Subscribe to all events (streaming, keeps connection open)
  events:subscribe:<types> - Subscribe filtered (e.g. events:subscribe:status_change,member_change)

Examples:
  {"type":"debug_command","id":1,"command":"swarm:list"}
  {"type":"debug_command","id":2,"command":"swarm:info:/home/user/myproject"}
  {"type":"debug_command","id":3,"command":"swarm:plan:/home/user/myproject"}
  {"type":"debug_command","id":4,"command":"swarm:broadcast:Build complete, ready for review"}
  {"type":"debug_command","id":5,"command":"swarm:notify:session_fox_123 Please review PR #42"}"#
        .to_string()
}

pub(super) async fn handle_debug_client(
    stream: Stream,
    sessions: Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>>,
    is_processing: Arc<RwLock<bool>>,
    session_id: Arc<RwLock<String>>,
    provider: Arc<dyn Provider>,
    client_connections: Arc<RwLock<HashMap<String, ClientConnectionInfo>>>,
    swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    shared_context: Arc<RwLock<HashMap<String, HashMap<String, SharedContext>>>>,
    swarm_plans: Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: Arc<RwLock<HashMap<String, String>>>,
    file_touches: Arc<RwLock<HashMap<PathBuf, Vec<FileAccess>>>>,
    channel_subscriptions: Arc<RwLock<HashMap<String, HashMap<String, HashSet<String>>>>>,
    client_debug_state: Arc<RwLock<ClientDebugState>>,
    client_debug_response_tx: broadcast::Sender<(u64, String)>,
    debug_jobs: Arc<RwLock<HashMap<String, DebugJob>>>,
    event_history: Arc<RwLock<Vec<SwarmEvent>>>,
    event_counter: Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: broadcast::Sender<SwarmEvent>,
    server_identity: ServerIdentity,
    server_start_time: std::time::Instant,
    ambient_runner: Option<AmbientRunnerHandle>,
    mcp_pool: Option<Arc<crate::mcp::SharedMcpPool>>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let request = match decode_request(&line) {
            Ok(r) => r,
            Err(e) => {
                let event = ServerEvent::Error {
                    id: 0,
                    message: format!("Invalid request: {}", e),
                    retry_after_secs: None,
                };
                let json = encode_event(&event);
                writer.write_all(json.as_bytes()).await?;
                continue;
            }
        };

        match request {
            Request::Ping { id } => {
                let event = ServerEvent::Pong { id };
                let json = encode_event(&event);
                writer.write_all(json.as_bytes()).await?;
            }

            Request::GetState { id } => {
                let current_session_id = session_id.read().await.clone();
                let sessions = sessions.read().await;
                let message_count = sessions.len();

                let event = ServerEvent::State {
                    id,
                    session_id: current_session_id,
                    message_count,
                    is_processing: *is_processing.read().await,
                };
                let json = encode_event(&event);
                writer.write_all(json.as_bytes()).await?;
            }

            Request::DebugCommand {
                id,
                command,
                session_id: requested_session,
            } => {
                if !debug_control_allowed() {
                    let event = ServerEvent::Error {
                        id,
                        message: "Debug control is disabled. Set JCODE_DEBUG_CONTROL=1 or run in self-dev mode.".to_string(),
                        retry_after_secs: None,
                    };
                    let json = encode_event(&event);
                    writer.write_all(json.as_bytes()).await?;
                    continue;
                }

                // Parse namespaced command
                let (namespace, cmd) = parse_namespaced_command(&command);

                let result = match namespace {
                    "client" => {
                        // Forward to TUI client
                        let mut response_rx = client_debug_response_tx.subscribe();
                        let mut attempts = 0usize;

                        loop {
                            let (client_id, tx) = {
                                let mut debug_state = client_debug_state.write().await;
                                match debug_state.active_sender() {
                                    Some(active) => active,
                                    None => {
                                        break Err(anyhow::anyhow!("No TUI client connected"));
                                    }
                                }
                            };

                            if tx.send((id, cmd.to_string())).is_ok() {
                                // Wait for response with timeout
                                let timeout = tokio::time::Duration::from_secs(30);
                                match tokio::time::timeout(timeout, async {
                                    loop {
                                        if let Ok((resp_id, output)) = response_rx.recv().await {
                                            if resp_id == id {
                                                return Ok(output);
                                            }
                                        }
                                    }
                                })
                                .await
                                {
                                    Ok(result) => break result,
                                    Err(_) => {
                                        break Err(anyhow::anyhow!(
                                            "Timeout waiting for client response"
                                        ));
                                    }
                                }
                            } else {
                                let mut debug_state = client_debug_state.write().await;
                                debug_state.unregister(&client_id);
                                attempts += 1;
                                if debug_state.clients.is_empty() || attempts > 8 {
                                    break Err(anyhow::anyhow!("No TUI client connected"));
                                }
                            }
                        }
                    }
                    "tester" => {
                        // Handle tester commands
                        execute_tester_command(cmd).await
                    }
                    _ => {
                        // Server commands (default)
                        if let Some(output) = maybe_handle_job_command(cmd, &debug_jobs).await? {
                            Ok(output)
                        } else if let Some(output) = maybe_handle_session_admin_command(
                            cmd,
                            &sessions,
                            &session_id,
                            &provider,
                            &swarm_members,
                            &swarms_by_id,
                            &swarm_coordinators,
                            &swarm_plans,
                            &event_history,
                            &event_counter,
                            &swarm_event_tx,
                            mcp_pool.clone(),
                        )
                        .await?
                        {
                            Ok(output)
                        } else if let Some(output) = maybe_handle_server_state_command(
                            cmd,
                            &sessions,
                            &client_connections,
                            &swarm_members,
                            &client_debug_state,
                            &server_identity,
                            server_start_time,
                        )
                        .await?
                        {
                            Ok(output)
                        } else if let Some(output) = maybe_handle_swarm_read_command(
                            cmd,
                            &sessions,
                            &swarm_members,
                            &swarms_by_id,
                            &shared_context,
                            &swarm_plans,
                            &swarm_coordinators,
                            &file_touches,
                            &channel_subscriptions,
                            &server_identity,
                        )
                        .await?
                        {
                            Ok(output)
                        } else if let Some(output) = maybe_handle_swarm_write_command(
                            cmd,
                            &session_id,
                            &swarm_members,
                            &swarms_by_id,
                            &shared_context,
                            &swarm_plans,
                            &swarm_coordinators,
                        )
                        .await?
                        {
                            Ok(output)
                        } else if let Some(output) =
                            maybe_handle_ambient_command(cmd, &ambient_runner, &provider).await?
                        {
                            Ok(output)
                        } else if maybe_handle_event_subscription_command(
                            id,
                            cmd,
                            &swarm_event_tx,
                            &mut writer,
                        )
                        .await?
                        {
                            return Ok(());
                        } else if let Some(output) =
                            maybe_handle_event_query_command(cmd, &event_history).await
                        {
                            Ok(output)
                        } else if cmd == "swarm:help" {
                            Ok(swarm_debug_help_text())
                        } else if cmd == "help" {
                            Ok(debug_help_text())
                        } else {
                            match resolve_debug_session(&sessions, &session_id, requested_session)
                                .await
                            {
                                Ok((_session, agent)) => {
                                    execute_debug_command(
                                        agent,
                                        cmd,
                                        Arc::clone(&debug_jobs),
                                        Some(&server_identity),
                                    )
                                    .await
                                }
                                Err(e) => Err(e),
                            }
                        }
                    }
                };

                let (ok, output) = match result {
                    Ok(output) => (true, output),
                    Err(e) => (false, e.to_string()),
                };
                let event = ServerEvent::DebugResponse { id, ok, output };
                let json = encode_event(&event);
                writer.write_all(json.as_bytes()).await?;
            }

            _ => {
                // Debug socket only allows ping, state, and debug_command
                let event = ServerEvent::Error {
                    id: request.id(),
                    message: "Debug socket only allows ping, state, and debug_command".to_string(),
                    retry_after_secs: None,
                };
                let json = encode_event(&event);
                writer.write_all(json.as_bytes()).await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        crate::storage::lock_test_env()
    }

    struct TestHomeGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev_home: Option<OsString>,
        _temp_home: tempfile::TempDir,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let lock = lock_env();
            let temp_home = tempfile::Builder::new()
                .prefix("jcode-server-debug-test-home-")
                .tempdir()
                .expect("create temp home");
            let prev_home = std::env::var_os("JCODE_HOME");
            std::env::set_var("JCODE_HOME", temp_home.path());
            Self {
                _lock: lock,
                prev_home,
                _temp_home: temp_home,
            }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            if let Some(prev_home) = &self.prev_home {
                std::env::set_var("JCODE_HOME", prev_home);
            } else {
                std::env::remove_var("JCODE_HOME");
            }
        }
    }

    #[test]
    fn client_debug_state_registers_unregisters_and_falls_back() {
        let mut state = ClientDebugState::default();
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, _rx2) = mpsc::unbounded_channel();

        state.register("client-a".to_string(), tx1.clone());
        state.register("client-b".to_string(), tx2.clone());

        let (active_id, _sender) = state.active_sender().expect("active sender present");
        assert_eq!(active_id, "client-b");

        state.unregister("client-b");
        let (fallback_id, _sender) = state.active_sender().expect("fallback sender present");
        assert_eq!(fallback_id, "client-a");

        state.unregister("client-a");
        assert!(state.active_sender().is_none());
    }

    #[test]
    fn debug_job_payloads_include_expected_fields() {
        let now = Instant::now();
        let job = DebugJob {
            id: "job_123".to_string(),
            status: DebugJobStatus::Completed,
            command: "message:hello".to_string(),
            session_id: Some("session_abc".to_string()),
            created_at: now,
            started_at: Some(now),
            finished_at: Some(now),
            output: Some("done".to_string()),
            error: None,
        };

        let summary = job.summary_payload();
        assert_eq!(summary.get("id").and_then(|v| v.as_str()), Some("job_123"));
        assert_eq!(
            summary.get("status").and_then(|v| v.as_str()),
            Some("completed")
        );
        assert_eq!(
            summary.get("session_id").and_then(|v| v.as_str()),
            Some("session_abc")
        );

        let status = job.status_payload();
        assert_eq!(status.get("output").and_then(|v| v.as_str()), Some("done"));
        assert!(status.get("error").is_some());
    }

    #[test]
    fn debug_help_text_mentions_key_namespaces_and_commands() {
        let help = debug_help_text();
        assert!(help.contains("SERVER COMMANDS"));
        assert!(help.contains("CLIENT COMMANDS"));
        assert!(help.contains("TESTER COMMANDS"));
        assert!(help.contains("message_async:<text>"));
        assert!(help.contains("client:frame"));
    }

    #[test]
    fn swarm_debug_help_text_mentions_core_swarm_sections() {
        let help = swarm_debug_help_text();
        assert!(help.contains("MEMBERS & STRUCTURE"));
        assert!(help.contains("PLAN PROPOSALS"));
        assert!(help.contains("REAL-TIME EVENTS"));
        assert!(help.contains("swarm:list"));
    }

    #[test]
    fn parse_namespaced_command_defaults_to_server_namespace() {
        assert_eq!(parse_namespaced_command("state"), ("server", "state"));
        assert_eq!(
            parse_namespaced_command("swarm:list"),
            ("server", "swarm:list")
        );
    }

    #[test]
    fn parse_namespaced_command_recognizes_known_namespaces() {
        assert_eq!(
            parse_namespaced_command("client:frame"),
            ("client", "frame")
        );
        assert_eq!(parse_namespaced_command("tester:list"), ("tester", "list"));
        assert_eq!(
            parse_namespaced_command("server:state"),
            ("server", "state")
        );
    }
}

#[cfg(test)]
mod debug_execution_tests {
    use super::debug_command_exec::{debug_message_timeout_secs, resolve_debug_session};
    use crate::agent::Agent;
    use crate::provider;
    use crate::tool::Registry;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::sync::Arc;
    use tokio::sync::{Mutex as AsyncMutex, RwLock};

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        crate::storage::lock_test_env()
    }

    struct EnvVarGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = lock_env();
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self {
                _lock: lock,
                key,
                previous,
            }
        }

        fn remove(key: &'static str) -> Self {
            let lock = lock_env();
            let previous = std::env::var_os(key);
            std::env::remove_var(key);
            Self {
                _lock: lock,
                key,
                previous,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(prev) = &self.previous {
                std::env::set_var(self.key, prev);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    struct TestProvider;

    #[async_trait::async_trait]
    impl provider::Provider for TestProvider {
        fn name(&self) -> &str {
            "test"
        }

        fn model(&self) -> String {
            "test".to_string()
        }

        fn available_models(&self) -> Vec<&'static str> {
            vec![]
        }

        fn available_models_display(&self) -> Vec<String> {
            vec![]
        }

        async fn prefetch_models(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn set_model(&self, _model: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn handles_tools_internally(&self) -> bool {
            false
        }

        async fn complete(
            &self,
            _messages: &[crate::message::Message],
            _tools: &[crate::message::ToolDefinition],
            _system: &str,
            _session_id: Option<&str>,
        ) -> anyhow::Result<crate::provider::EventStream> {
            unimplemented!()
        }

        fn fork(&self) -> Arc<dyn provider::Provider> {
            Arc::new(TestProvider)
        }
    }

    async fn test_agent() -> Arc<AsyncMutex<Agent>> {
        let provider = Arc::new(TestProvider) as Arc<dyn provider::Provider>;
        let registry = Registry::new(provider.clone()).await;
        Arc::new(AsyncMutex::new(Agent::new(provider, registry)))
    }

    #[tokio::test]
    async fn resolve_debug_session_uses_requested_session_when_present() {
        let agent = test_agent().await;
        let session_id = {
            let agent = agent.lock().await;
            agent.session_id().to_string()
        };
        let sessions = Arc::new(RwLock::new(HashMap::from([(
            session_id.clone(),
            agent.clone(),
        )])));
        let current = Arc::new(RwLock::new(String::new()));

        let (resolved_id, resolved_agent) =
            resolve_debug_session(&sessions, &current, Some(session_id.clone()))
                .await
                .expect("resolve requested session");

        assert_eq!(resolved_id, session_id);
        assert!(Arc::ptr_eq(&resolved_agent, &agent));
    }

    #[tokio::test]
    async fn resolve_debug_session_falls_back_to_current_session() {
        let agent = test_agent().await;
        let session_id = {
            let agent = agent.lock().await;
            agent.session_id().to_string()
        };
        let sessions = Arc::new(RwLock::new(HashMap::from([(
            session_id.clone(),
            agent.clone(),
        )])));
        let current = Arc::new(RwLock::new(session_id.clone()));

        let (resolved_id, resolved_agent) = resolve_debug_session(&sessions, &current, None)
            .await
            .expect("resolve current session");

        assert_eq!(resolved_id, session_id);
        assert!(Arc::ptr_eq(&resolved_agent, &agent));
    }

    #[tokio::test]
    async fn resolve_debug_session_uses_only_session_when_singleton() {
        let agent = test_agent().await;
        let session_id = {
            let agent = agent.lock().await;
            agent.session_id().to_string()
        };
        let sessions = Arc::new(RwLock::new(HashMap::from([(
            session_id.clone(),
            agent.clone(),
        )])));
        let current = Arc::new(RwLock::new(String::new()));

        let (resolved_id, _) = resolve_debug_session(&sessions, &current, None)
            .await
            .expect("resolve single session");

        assert_eq!(resolved_id, session_id);
    }

    #[tokio::test]
    async fn resolve_debug_session_errors_for_unknown_or_missing_session() {
        let agent_a = test_agent().await;
        let id_a = {
            let agent = agent_a.lock().await;
            agent.session_id().to_string()
        };
        let agent_b = test_agent().await;
        let id_b = {
            let agent = agent_b.lock().await;
            agent.session_id().to_string()
        };

        let sessions = Arc::new(RwLock::new(HashMap::from([
            (id_a.clone(), agent_a),
            (id_b.clone(), agent_b),
        ])));
        let current = Arc::new(RwLock::new(String::new()));

        let unknown = resolve_debug_session(&sessions, &current, Some("missing".to_string())).await;
        let unknown_err = match unknown {
            Ok(_) => panic!("expected unknown session to error"),
            Err(err) => err,
        };
        assert!(unknown_err.to_string().contains("Unknown session_id"));

        let missing = resolve_debug_session(&sessions, &current, None).await;
        let missing_err = match missing {
            Ok(_) => panic!("expected missing active session to error"),
            Err(err) => err,
        };
        assert!(missing_err.to_string().contains("No active session found"));
    }

    #[test]
    fn debug_message_timeout_secs_reads_valid_env_values() {
        let _guard = EnvVarGuard::set("JCODE_DEBUG_MESSAGE_TIMEOUT_SECS", "17");
        assert_eq!(debug_message_timeout_secs(), Some(17));
    }

    #[test]
    fn debug_message_timeout_secs_ignores_missing_empty_invalid_and_zero() {
        let _guard = EnvVarGuard::remove("JCODE_DEBUG_MESSAGE_TIMEOUT_SECS");
        assert_eq!(debug_message_timeout_secs(), None);
        drop(_guard);

        let _guard = EnvVarGuard::set("JCODE_DEBUG_MESSAGE_TIMEOUT_SECS", "   ");
        assert_eq!(debug_message_timeout_secs(), None);
        drop(_guard);

        let _guard = EnvVarGuard::set("JCODE_DEBUG_MESSAGE_TIMEOUT_SECS", "abc");
        assert_eq!(debug_message_timeout_secs(), None);
        drop(_guard);

        let _guard = EnvVarGuard::set("JCODE_DEBUG_MESSAGE_TIMEOUT_SECS", "0");
        assert_eq!(debug_message_timeout_secs(), None);
    }
}
