use super::{
    CommunicateInput, CommunicateTool, default_await_target_statuses, format_awaited_members,
    format_members, format_plan_status,
};
use crate::message::{Message, StreamEvent, ToolDefinition};
use crate::protocol::{
    AgentInfo, AgentStatusSnapshot, AwaitedMemberStatus, Request, ServerEvent,
    SessionActivitySnapshot, ToolCallSummary,
};
use crate::provider::{EventStream, Provider};
use crate::server::Server;
use crate::tool::{Tool, ToolContext, ToolExecutionMode};
use crate::transport::{ReadHalf, Stream, WriteHalf};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn tool_is_named_swarm() {
    assert_eq!(CommunicateTool::new().name(), "swarm");
}

#[test]
fn format_plan_status_includes_next_ready() {
    let output = format_plan_status(&crate::protocol::PlanGraphStatus {
        swarm_id: Some("swarm-a".to_string()),
        version: 3,
        item_count: 4,
        ready_ids: vec!["task-2".to_string(), "task-3".to_string()],
        blocked_ids: vec!["task-4".to_string()],
        active_ids: vec!["task-1".to_string()],
        completed_ids: vec!["setup".to_string()],
        cycle_ids: Vec::new(),
        unresolved_dependency_ids: Vec::new(),
        next_ready_ids: vec!["task-2".to_string()],
        newly_ready_ids: vec!["task-3".to_string()],
    });
    let text = output.output;
    assert!(text.contains("Plan status for swarm swarm-a"));
    assert!(text.contains("Next up: task-2"));
    assert!(text.contains("Newly ready: task-3"));
    assert!(text.contains("Blocked: task-4"));
}

#[test]
fn schema_still_requires_action() {
    let schema = CommunicateTool::new().parameters_schema();
    assert_eq!(schema["required"], json!(["action"]));
}

#[test]
fn schema_advertises_supported_swarm_fields() {
    let schema = CommunicateTool::new().parameters_schema();
    let props = schema["properties"]
        .as_object()
        .expect("swarm schema should have properties");

    assert!(props.contains_key("action"));
    assert!(props.contains_key("key"));
    assert!(props.contains_key("value"));
    assert!(props.contains_key("message"));
    assert!(props.contains_key("to_session"));
    assert_eq!(
        props["to_session"]["description"],
        json!(
            "DM target. Accepts an exact session ID or a unique friendly name within the swarm. If a friendly name is ambiguous, run swarm list and use the exact session ID."
        )
    );
    assert!(props.contains_key("channel"));
    assert!(props.contains_key("proposer_session"));
    assert!(props.contains_key("reason"));
    assert!(props.contains_key("target_session"));
    assert!(props.contains_key("role"));
    assert!(props.contains_key("prompt"));
    assert!(props.contains_key("working_dir"));
    assert!(props.contains_key("limit"));
    assert!(props.contains_key("task_id"));
    assert!(props.contains_key("spawn_if_needed"));
    assert!(props.contains_key("prefer_spawn"));
    assert!(props.contains_key("session_ids"));
    assert!(props.contains_key("mode"));
    assert!(props.contains_key("target_status"));
    assert!(props.contains_key("timeout_minutes"));
    assert!(props.contains_key("concurrency_limit"));
    assert!(props.contains_key("wake"));
    assert!(props.contains_key("delivery"));
    assert!(props.contains_key("plan_items"));
    assert!(!props.contains_key("initial_message"));
    assert_eq!(
        props["delivery"]["enum"],
        json!(["notify", "interrupt", "wake"])
    );
    assert_eq!(
        props["plan_items"]["items"]["additionalProperties"],
        json!(true)
    );
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("status"))
    );
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("plan_status"))
    );
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("start"))
    );
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("assign_next"))
    );
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("fill_slots"))
    );
    assert!(
        schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .contains(&json!("salvage"))
    );
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(key);
        crate::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.take() {
            crate::env::set_var(self.key, value);
        } else {
            crate::env::remove_var(self.key);
        }
    }
}

struct DelayedTestProvider {
    delay: Duration,
}

#[async_trait]
impl Provider for DelayedTestProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let delay = self.delay;
        let stream = futures::stream::once(async move {
            tokio::time::sleep(delay).await;
            Ok(StreamEvent::TextDelta("ok".to_string()))
        })
        .chain(futures::stream::once(async {
            Ok(StreamEvent::MessageEnd { stop_reason: None })
        }));
        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "test"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self { delay: self.delay })
    }
}

struct RawClient {
    reader: BufReader<ReadHalf>,
    writer: WriteHalf,
    next_id: u64,
}

impl RawClient {
    async fn connect(path: &Path) -> Result<Self> {
        let stream = Stream::connect(path).await?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
        })
    }

    async fn send_request(&mut self, request: Request) -> Result<u64> {
        let id = request.id();
        let json = serde_json::to_string(&request)? + "\n";
        self.writer.write_all(json.as_bytes()).await?;
        Ok(id)
    }

    async fn read_event(&mut self) -> Result<ServerEvent> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("server disconnected")
        }
        Ok(serde_json::from_str(&line)?)
    }

    async fn read_until<F>(&mut self, timeout: Duration, mut predicate: F) -> Result<ServerEvent>
    where
        F: FnMut(&ServerEvent) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let event = tokio::time::timeout(remaining, self.read_event()).await??;
            if predicate(&event) {
                return Ok(event);
            }
        }
    }

    async fn subscribe(&mut self, working_dir: &Path) -> Result<()> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_request(Request::Subscribe {
            id,
            working_dir: Some(working_dir.display().to_string()),
            selfdev: None,
            target_session_id: None,
            client_instance_id: None,
            client_has_local_history: false,
            allow_session_takeover: false,
        })
        .await?;
        self.read_until(
            Duration::from_secs(5),
            |event| matches!(event, ServerEvent::Done { id: done_id } if *done_id == id),
        )
        .await?;
        Ok(())
    }

    async fn session_id(&mut self) -> Result<String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_request(Request::GetState { id }).await?;
        match self
            .read_until(
                Duration::from_secs(5),
                |event| matches!(event, ServerEvent::State { id: event_id, .. } if *event_id == id),
            )
            .await?
        {
            ServerEvent::State { session_id, .. } => Ok(session_id),
            other => anyhow::bail!("unexpected state response: {other:?}"),
        }
    }

    async fn send_message(&mut self, content: &str) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_request(Request::Message {
            id,
            content: content.to_string(),
            images: vec![],
            system_reminder: None,
        })
        .await
    }

    async fn wait_for_done(&mut self, request_id: u64) -> Result<()> {
        self.read_until(
            Duration::from_secs(10),
            |event| matches!(event, ServerEvent::Done { id } if *id == request_id),
        )
        .await?;
        Ok(())
    }

    async fn comm_list(&mut self, session_id: &str) -> Result<Vec<AgentInfo>> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_request(Request::CommList {
            id,
            session_id: session_id.to_string(),
        })
        .await?;
        match self
                .read_until(Duration::from_secs(5), |event| {
                    matches!(event, ServerEvent::CommMembers { id: event_id, .. } if *event_id == id)
                })
                .await?
            {
                ServerEvent::CommMembers { members, .. } => Ok(members),
                other => anyhow::bail!("unexpected comm_list response: {other:?}"),
            }
    }

    async fn comm_status(
        &mut self,
        session_id: &str,
        target_session: &str,
    ) -> Result<AgentStatusSnapshot> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_request(Request::CommStatus {
            id,
            session_id: session_id.to_string(),
            target_session: target_session.to_string(),
        })
        .await?;
        match self
                .read_until(Duration::from_secs(5), |event| {
                    matches!(event, ServerEvent::CommStatusResponse { id: event_id, .. } if *event_id == id)
                })
                .await?
            {
                ServerEvent::CommStatusResponse { snapshot, .. } => Ok(snapshot),
                other => anyhow::bail!("unexpected comm_status response: {other:?}"),
            }
    }
}

async fn wait_for_server_socket(
    path: &Path,
    server_task: &mut tokio::task::JoinHandle<Result<()>>,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if server_task.is_finished() {
            let result = server_task.await?;
            return Err(anyhow::anyhow!(
                "server exited before socket became ready: {:?}",
                result
            ));
        }
        match Stream::connect(path).await {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(err) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(err.into());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

fn test_ctx(session_id: &str, working_dir: &Path) -> ToolContext {
    ToolContext {
        session_id: session_id.to_string(),
        message_id: "msg-1".to_string(),
        tool_call_id: "call-1".to_string(),
        working_dir: Some(working_dir.to_path_buf()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    }
}

async fn wait_for_member_status(
    client: &mut RawClient,
    requester_session: &str,
    target_session: &str,
    expected_status: &str,
) -> Result<Vec<AgentInfo>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let members = client.comm_list(requester_session).await?;
        if members
            .iter()
            .find(|member| member.session_id == target_session)
            .and_then(|member| member.status.as_deref())
            == Some(expected_status)
        {
            return Ok(members);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "timed out waiting for member {} to reach status {}",
                target_session,
                expected_status
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_member_presence(
    client: &mut RawClient,
    requester_session: &str,
    target_session: &str,
) -> Result<Vec<AgentInfo>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let members = client.comm_list(requester_session).await?;
        if members
            .iter()
            .any(|member| member.session_id == target_session)
        {
            return Ok(members);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for member {} to appear", target_session);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[test]
fn default_await_members_targets_include_ready() {
    assert_eq!(
        default_await_target_statuses(),
        vec!["ready", "completed", "stopped", "failed"]
    );
}

#[test]
fn spawn_initial_message_accepts_prompt_alias_and_prefers_explicit_initial_message() {
    let from_prompt: CommunicateInput = serde_json::from_value(serde_json::json!({
        "action": "spawn",
        "prompt": "review the diff"
    }))
    .expect("prompt alias should deserialize");
    assert_eq!(
        from_prompt.spawn_initial_message().as_deref(),
        Some("review the diff")
    );

    let preferred: CommunicateInput = serde_json::from_value(serde_json::json!({
        "action": "spawn",
        "initial_message": "preferred",
        "prompt": "fallback"
    }))
    .expect("spawn payload should deserialize");
    assert_eq!(
        preferred.spawn_initial_message().as_deref(),
        Some("preferred")
    );
}

#[test]
fn communicate_input_accepts_delivery_and_share_append() {
    let delivery: CommunicateInput = serde_json::from_value(serde_json::json!({
        "action": "dm",
        "message": "ping",
        "to_session": "sess-2",
        "delivery": "wake"
    }))
    .expect("delivery mode should deserialize");
    assert_eq!(
        delivery.delivery,
        Some(crate::protocol::CommDeliveryMode::Wake)
    );

    let append: CommunicateInput = serde_json::from_value(serde_json::json!({
        "action": "share_append",
        "key": "task/123/notes",
        "value": "new line"
    }))
    .expect("share_append should deserialize");
    assert_eq!(append.action, "share_append");
}

#[test]
fn communicate_input_accepts_spawn_if_needed() {
    let parsed: CommunicateInput = serde_json::from_value(serde_json::json!({
        "action": "assign_task",
        "spawn_if_needed": true
    }))
    .expect("spawn_if_needed should deserialize");
    assert_eq!(parsed.spawn_if_needed, Some(true));
}

#[test]
fn communicate_input_accepts_prefer_spawn() {
    let parsed: CommunicateInput = serde_json::from_value(serde_json::json!({
        "action": "assign_task",
        "prefer_spawn": true
    }))
    .expect("prefer_spawn should deserialize");
    assert_eq!(parsed.prefer_spawn, Some(true));
}

#[test]
fn format_tool_summary_includes_call_count() {
    let output = super::format_tool_summary(
        "session-123",
        &[
            ToolCallSummary {
                tool_name: "read".to_string(),
                brief_output: "Read 20 lines".to_string(),
                timestamp_secs: None,
            },
            ToolCallSummary {
                tool_name: "grep".to_string(),
                brief_output: "Found 3 matches".to_string(),
                timestamp_secs: None,
            },
        ],
    );

    assert!(
        output
            .output
            .contains("Tool call summary for session-123 (2 calls):")
    );
    assert!(output.output.contains("read — Read 20 lines"));
    assert!(output.output.contains("grep — Found 3 matches"));
}

#[test]
fn format_members_includes_status_and_detail() {
    let ctx = ToolContext {
        session_id: "sess-self".to_string(),
        message_id: "msg-1".to_string(),
        tool_call_id: "call-1".to_string(),
        working_dir: None,
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: ToolExecutionMode::Direct,
    };

    let output = format_members(
        &ctx,
        &[AgentInfo {
            session_id: "sess-peer".to_string(),
            friendly_name: Some("bear".to_string()),
            files_touched: vec!["src/main.rs".to_string()],
            status: Some("running".to_string()),
            detail: Some("working on tests".to_string()),
            role: Some("agent".to_string()),
            is_headless: Some(true),
            live_attachments: Some(0),
            status_age_secs: Some(12),
        }],
    );

    assert!(output.output.contains("Status: running — working on tests"));
    assert!(output.output.contains("Files: src/main.rs"));
    assert!(
        output
            .output
            .contains("Meta: headless · attachments=0 · status_age=12s")
    );
}

#[test]
fn format_members_disambiguates_duplicate_friendly_names() {
    let ctx = test_ctx(
        "session_self_1234567890_deadbeefcafebabe",
        std::path::Path::new("."),
    );
    let output = format_members(
        &ctx,
        &[
            AgentInfo {
                session_id: "session_shark_1234567890_aaaaaaaaaaaa0001".to_string(),
                friendly_name: Some("shark".to_string()),
                files_touched: vec![],
                status: Some("ready".to_string()),
                detail: None,
                role: Some("agent".to_string()),
                is_headless: None,
                live_attachments: None,
                status_age_secs: None,
            },
            AgentInfo {
                session_id: "session_shark_1234567890_bbbbbbbbbbbb0002".to_string(),
                friendly_name: Some("shark".to_string()),
                files_touched: vec![],
                status: Some("ready".to_string()),
                detail: None,
                role: Some("agent".to_string()),
                is_headless: None,
                live_attachments: None,
                status_age_secs: None,
            },
        ],
    );

    assert!(output.output.contains("shark [aa0001]"));
    assert!(output.output.contains("shark [bb0002]"));
}

#[test]
fn format_awaited_members_disambiguates_duplicate_friendly_names() {
    let output = format_awaited_members(
        true,
        "done",
        &[
            AwaitedMemberStatus {
                session_id: "session_shark_1234567890_aaaaaaaaaaaa0001".to_string(),
                friendly_name: Some("shark".to_string()),
                status: "ready".to_string(),
                done: true,
            },
            AwaitedMemberStatus {
                session_id: "session_shark_1234567890_bbbbbbbbbbbb0002".to_string(),
                friendly_name: Some("shark".to_string()),
                status: "ready".to_string(),
                done: true,
            },
        ],
    );

    assert!(output.output.contains("✓ shark [aa0001] (ready)"));
    assert!(output.output.contains("✓ shark [bb0002] (ready)"));
}

#[test]
fn format_status_snapshot_includes_activity_and_metadata() {
    let output = super::format_status_snapshot(&AgentStatusSnapshot {
        session_id: "sess-peer".to_string(),
        friendly_name: Some("bear".to_string()),
        swarm_id: Some("swarm-test".to_string()),
        status: Some("running".to_string()),
        detail: Some("working on observability".to_string()),
        role: Some("agent".to_string()),
        is_headless: Some(true),
        live_attachments: Some(0),
        status_age_secs: Some(7),
        joined_age_secs: Some(42),
        files_touched: vec!["src/server/comm_sync.rs".to_string()],
        activity: Some(SessionActivitySnapshot {
            is_processing: true,
            current_tool_name: Some("bash".to_string()),
        }),
        provider_name: None,
        provider_model: None,
    });

    assert!(
        output
            .output
            .contains("Status snapshot for bear (sess-peer)")
    );
    assert!(
        output
            .output
            .contains("Lifecycle: running — working on observability")
    );
    assert!(output.output.contains("Activity: busy (bash)"));
    assert!(output.output.contains("Swarm: swarm-test"));
    assert!(
        output
            .output
            .contains("Meta: headless · attachments=0 · status_age=7s · joined=42s")
    );
    assert!(output.output.contains("Files: src/server/comm_sync.rs"));
}

#[tokio::test]
async fn communicate_list_and_await_members_work_end_to_end() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(300),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    let socket_path = runtime_dir.path().join("jcode.sock");
    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    let mut peer = RawClient::connect(&socket_path)
        .await
        .expect("peer should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");
    peer.subscribe(&repo_dir).await.expect("peer subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let peer_session = peer.session_id().await.expect("peer session id");

    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    let list_output = tool
        .execute(json!({"action": "list"}), ctx.clone())
        .await
        .expect("communicate list should succeed");
    assert!(
        list_output.output.contains("Status: ready"),
        "expected communicate list to render member status, got: {}",
        list_output.output
    );

    let peer_message_id = peer
        .send_message("Reply with a short acknowledgement.")
        .await
        .expect("peer message request should send");

    let running_members =
        wait_for_member_status(&mut watcher, &watcher_session, &peer_session, "running")
            .await
            .expect("peer should enter running state");
    let running_peer = running_members
        .iter()
        .find(|member| member.session_id == peer_session)
        .expect("peer should be listed while running");
    assert_eq!(running_peer.status.as_deref(), Some("running"));

    let await_output = tool
        .execute(
            json!({
                "action": "await_members",
                "session_ids": [peer_session.clone()],
                "timeout_minutes": 1
            }),
            ctx.clone(),
        )
        .await
        .expect("await_members should complete");
    assert!(
        await_output.output.contains("All members done."),
        "expected completion output, got: {}",
        await_output.output
    );
    assert!(
        await_output.output.contains("(ready)"),
        "expected await_members to treat ready as done, got: {}",
        await_output.output
    );

    peer.wait_for_done(peer_message_id)
        .await
        .expect("peer message should finish");

    let ready_members =
        wait_for_member_status(&mut watcher, &watcher_session, &peer_session, "ready")
            .await
            .expect("peer should return to ready state");
    let ready_peer = ready_members
        .iter()
        .find(|member| member.session_id == peer_session)
        .expect("peer should still be listed when ready");
    assert_eq!(ready_peer.status.as_deref(), Some("ready"));

    server_task.abort();
}

#[tokio::test]
async fn communicate_status_returns_busy_snapshot_for_running_member() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(300),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    let mut peer = RawClient::connect(&socket_path)
        .await
        .expect("peer should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");
    peer.subscribe(&repo_dir).await.expect("peer subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let peer_session = peer.session_id().await.expect("peer session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    let peer_message_id = peer
        .send_message("Reply with a short acknowledgement.")
        .await
        .expect("peer message request should send");

    wait_for_member_status(&mut watcher, &watcher_session, &peer_session, "running")
        .await
        .expect("peer should enter running state");

    let snapshot = watcher
        .comm_status(&watcher_session, &peer_session)
        .await
        .expect("comm_status should succeed while peer is busy");
    assert_eq!(snapshot.session_id, peer_session);
    assert_eq!(snapshot.status.as_deref(), Some("running"));
    assert!(
        snapshot
            .activity
            .as_ref()
            .is_some_and(|activity| activity.is_processing)
    );

    let output = tool
        .execute(
            json!({
                "action": "status",
                "target_session": peer_session.clone()
            }),
            ctx,
        )
        .await
        .expect("status action should succeed");
    assert!(output.output.contains("Lifecycle: running"));
    assert!(output.output.contains("Activity: busy"));

    peer.wait_for_done(peer_message_id)
        .await
        .expect("peer message should finish");

    server_task.abort();
}

#[tokio::test]
async fn communicate_spawn_reports_completion_back_to_spawner() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    let socket_path = runtime_dir.path().join("jcode.sock");
    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    let spawn_output = tool
        .execute(
            json!({
                "action": "spawn",
                "prompt": "Reply with exactly AUTH_TEST_OK and nothing else."
            }),
            ctx,
        )
        .await
        .expect("spawn with prompt should succeed");
    let spawned_session = spawn_output
        .output
        .strip_prefix("Spawned new agent: ")
        .expect("spawn output should include session id")
        .trim()
        .to_string();

    watcher
        .read_until(Duration::from_secs(15), |event| {
            matches!(
                event,
                ServerEvent::Notification {
                    from_session,
                    notification_type: crate::protocol::NotificationType::Message {
                        scope: Some(scope),
                        channel: None,
                    },
                    message,
                    ..
                } if from_session == &spawned_session
                    && scope == "swarm"
                    && message.contains("finished their work and is ready for more")
            )
        })
        .await
        .expect("spawner should receive completion report-back notification");

    server_task.abort();
}

#[tokio::test]
async fn communicate_spawn_with_prompt_and_summary_work_end_to_end() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    let socket_path = runtime_dir.path().join("jcode.sock");
    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    let spawn_output = tool
        .execute(
            json!({
                "action": "spawn",
                "prompt": "Reply with a short acknowledgement."
            }),
            ctx.clone(),
        )
        .await
        .expect("spawn with prompt should succeed");
    let spawned_session = spawn_output
        .output
        .strip_prefix("Spawned new agent: ")
        .expect("spawn output should include session id")
        .trim()
        .to_string();
    assert!(
        !spawned_session.is_empty(),
        "spawned session id should not be empty"
    );

    wait_for_member_presence(&mut watcher, &watcher_session, &spawned_session)
        .await
        .expect("spawned member should appear in swarm list");

    let summary_output = {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match tool
                .execute(
                    json!({
                        "action": "summary",
                        "target_session": spawned_session
                    }),
                    ctx.clone(),
                )
                .await
            {
                Ok(output) => break output,
                Err(err)
                    if (err.to_string().contains("Unknown session")
                        || err.to_string().contains(" is busy;"))
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(err) => panic!("summary for spawned agent should succeed: {err}"),
            }
        }
    };
    assert!(
        summary_output.output.contains("Tool call summary for")
            || summary_output.output.contains("No tool calls found for"),
        "unexpected summary output: {}",
        summary_output.output
    );

    server_task.abort();
}

#[tokio::test]
async fn communicate_assign_task_can_spawn_fallback_agent() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    tool.execute(
        json!({
            "action": "assign_role",
            "target_session": watcher_session,
            "role": "coordinator"
        }),
        ctx.clone(),
    )
    .await
    .expect("self-promotion to coordinator should succeed");

    tool.execute(
        json!({
            "action": "propose_plan",
            "plan_items": [{
                "id": "task-a",
                "content": "Implement planner follow-up",
                "status": "queued",
                "priority": "high"
            }]
        }),
        ctx.clone(),
    )
    .await
    .expect("plan proposal should succeed");

    let assign_output = tool
        .execute(
            json!({
                "action": "assign_task",
                "spawn_if_needed": true
            }),
            ctx,
        )
        .await
        .expect("assign_task should spawn a fallback worker");

    assert!(
        assign_output.output.contains("spawned automatically"),
        "expected fallback spawn in output, got: {}",
        assign_output.output
    );
    assert!(
        assign_output.output.contains("task-a"),
        "expected selected task id in output, got: {}",
        assign_output.output
    );

    let spawned_session = assign_output
        .output
        .strip_prefix("Task 'task-a' assigned to ")
        .and_then(|rest| rest.strip_suffix(" (spawned automatically)"))
        .expect("assign output should include spawned session id")
        .trim()
        .to_string();

    assert!(
        !spawned_session.is_empty(),
        "spawned session id should not be empty"
    );

    wait_for_member_presence(&mut watcher, &watcher_session, &spawned_session)
        .await
        .expect("spawned fallback worker should appear in swarm");

    let members = watcher
        .comm_list(&watcher_session)
        .await
        .expect("comm_list should succeed");
    let spawned_member = members
        .iter()
        .find(|member| member.session_id == spawned_session)
        .expect("spawned worker should be listed");
    assert_eq!(spawned_member.role.as_deref(), Some("agent"));

    server_task.abort();
}

#[tokio::test]
async fn communicate_assign_next_assigns_next_runnable_task() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    tool.execute(
        json!({
            "action": "assign_role",
            "target_session": watcher_session,
            "role": "coordinator"
        }),
        ctx.clone(),
    )
    .await
    .expect("self-promotion to coordinator should succeed");

    let spawn_output = tool
        .execute(
            json!({
                "action": "spawn"
            }),
            ctx.clone(),
        )
        .await
        .expect("worker spawn should succeed");
    let worker_session = spawn_output
        .output
        .strip_prefix("Spawned new agent: ")
        .expect("spawn output should include session id")
        .trim()
        .to_string();

    wait_for_member_presence(&mut watcher, &watcher_session, &worker_session)
        .await
        .expect("spawned worker should appear in swarm");

    tool.execute(
        json!({
            "action": "propose_plan",
            "plan_items": [{
                "id": "setup",
                "content": "setup",
                "status": "completed",
                "priority": "high"
            }, {
                "id": "next",
                "content": "Take the next task",
                "status": "queued",
                "priority": "high",
                "blocked_by": ["setup"]
            }]
        }),
        ctx.clone(),
    )
    .await
    .expect("plan proposal should succeed");

    let assign_output = tool
        .execute(
            json!({
                "action": "assign_next",
                "target_session": worker_session
            }),
            ctx,
        )
        .await
        .expect("assign_next should succeed");

    assert!(
        assign_output.output.contains("Task 'next' assigned to"),
        "unexpected assign_next output: {}",
        assign_output.output
    );

    server_task.abort();
}

#[tokio::test]
async fn communicate_assign_next_can_prefer_fresh_spawn_server_side() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    tool.execute(
        json!({
            "action": "assign_role",
            "target_session": watcher_session,
            "role": "coordinator"
        }),
        ctx.clone(),
    )
    .await
    .expect("self-promotion to coordinator should succeed");

    let existing_output = tool
        .execute(json!({"action": "spawn"}), ctx.clone())
        .await
        .expect("existing worker spawn should succeed");
    let existing_worker = existing_output
        .output
        .strip_prefix("Spawned new agent: ")
        .expect("spawn output should include session id")
        .trim()
        .to_string();
    wait_for_member_presence(&mut watcher, &watcher_session, &existing_worker)
        .await
        .expect("existing worker should appear in swarm");

    tool.execute(
        json!({
            "action": "propose_plan",
            "plan_items": [{
                "id": "task-c",
                "content": "Use a fresh worker",
                "status": "queued",
                "priority": "high"
            }]
        }),
        ctx.clone(),
    )
    .await
    .expect("plan proposal should succeed");

    let assign_output = tool
        .execute(
            json!({
                "action": "assign_next",
                "prefer_spawn": true
            }),
            ctx,
        )
        .await
        .expect("assign_next with prefer_spawn should succeed");

    let preferred_session = assign_output
        .output
        .strip_prefix("Task 'task-c' assigned to ")
        .expect("assign_next output should include session id")
        .trim()
        .to_string();

    assert_ne!(
        preferred_session, existing_worker,
        "server-side prefer_spawn should choose a fresh worker"
    );

    wait_for_member_presence(&mut watcher, &watcher_session, &preferred_session)
        .await
        .expect("preferred spawned worker should appear in swarm");

    server_task.abort();
}

#[tokio::test]
async fn communicate_assign_next_can_spawn_if_needed_server_side() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    tool.execute(
        json!({
            "action": "assign_role",
            "target_session": watcher_session,
            "role": "coordinator"
        }),
        ctx.clone(),
    )
    .await
    .expect("self-promotion to coordinator should succeed");

    tool.execute(
        json!({
            "action": "propose_plan",
            "plan_items": [{
                "id": "task-d",
                "content": "Spawn if no worker exists",
                "status": "queued",
                "priority": "high"
            }]
        }),
        ctx.clone(),
    )
    .await
    .expect("plan proposal should succeed");

    let assign_output = tool
        .execute(
            json!({
                "action": "assign_next",
                "spawn_if_needed": true
            }),
            ctx,
        )
        .await
        .expect("assign_next with spawn_if_needed should succeed");

    let spawned_session = assign_output
        .output
        .strip_prefix("Task 'task-d' assigned to ")
        .expect("assign_next output should include session id")
        .trim()
        .to_string();
    assert!(
        !spawned_session.is_empty(),
        "server-side spawn_if_needed should assign a spawned worker"
    );

    wait_for_member_presence(&mut watcher, &watcher_session, &spawned_session)
        .await
        .expect("spawn_if_needed worker should appear in swarm");

    server_task.abort();
}

#[tokio::test]
async fn communicate_fill_slots_tops_up_to_concurrency_limit() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(300),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    tool.execute(
        json!({
            "action": "assign_role",
            "target_session": watcher_session,
            "role": "coordinator"
        }),
        ctx.clone(),
    )
    .await
    .expect("self-promotion to coordinator should succeed");

    tool.execute(
        json!({
            "action": "propose_plan",
            "plan_items": [{
                "id": "task-1",
                "content": "first task",
                "status": "queued",
                "priority": "high"
            }, {
                "id": "task-2",
                "content": "second task",
                "status": "queued",
                "priority": "high"
            }, {
                "id": "task-3",
                "content": "third task",
                "status": "queued",
                "priority": "high"
            }]
        }),
        ctx.clone(),
    )
    .await
    .expect("plan proposal should succeed");

    let output = tool
        .execute(
            json!({
                "action": "fill_slots",
                "concurrency_limit": 2,
                "spawn_if_needed": true
            }),
            ctx,
        )
        .await
        .expect("fill_slots should succeed");

    assert!(
        output.output.contains("Filled 2 slot(s):"),
        "unexpected fill_slots output: {}",
        output.output
    );

    server_task.abort();
}

#[tokio::test]
async fn communicate_assign_task_can_prefer_fresh_spawn_over_reuse() {
    let _env_lock = crate::storage::lock_test_env();
    let runtime_dir = tempfile::TempDir::new().expect("runtime tempdir");
    let repo_dir = std::env::current_dir().expect("repo cwd");
    let socket_path = runtime_dir.path().join("jcode.sock");
    let _runtime = EnvGuard::set("JCODE_RUNTIME_DIR", runtime_dir.path());
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket_path);
    let _debug = EnvGuard::set("JCODE_DEBUG_CONTROL", "1");

    let provider: Arc<dyn Provider> = Arc::new(DelayedTestProvider {
        delay: Duration::from_millis(100),
    });
    let server = Arc::new(Server::new(provider));
    let mut server_task = {
        let server = Arc::clone(&server);
        tokio::spawn(async move { server.run().await })
    };

    wait_for_server_socket(&socket_path, &mut server_task)
        .await
        .expect("server socket should be ready");

    let mut watcher = RawClient::connect(&socket_path)
        .await
        .expect("watcher should connect");
    watcher
        .subscribe(&repo_dir)
        .await
        .expect("watcher subscribe");

    let watcher_session = watcher.session_id().await.expect("watcher session id");
    let tool = CommunicateTool::new();
    let ctx = test_ctx(&watcher_session, &repo_dir);

    tool.execute(
        json!({
            "action": "assign_role",
            "target_session": watcher_session,
            "role": "coordinator"
        }),
        ctx.clone(),
    )
    .await
    .expect("self-promotion to coordinator should succeed");

    let existing_output = tool
        .execute(
            json!({
                "action": "spawn"
            }),
            ctx.clone(),
        )
        .await
        .expect("existing reusable worker should spawn");
    let existing_worker = existing_output
        .output
        .strip_prefix("Spawned new agent: ")
        .expect("spawn output should include session id")
        .trim()
        .to_string();
    wait_for_member_presence(&mut watcher, &watcher_session, &existing_worker)
        .await
        .expect("existing worker should appear in swarm");

    tool.execute(
        json!({
            "action": "propose_plan",
            "plan_items": [{
                "id": "task-b",
                "content": "Investigate a separate subsystem",
                "status": "queued",
                "priority": "high"
            }]
        }),
        ctx.clone(),
    )
    .await
    .expect("plan proposal should succeed");

    let assign_output = tool
        .execute(
            json!({
                "action": "assign_task",
                "prefer_spawn": true
            }),
            ctx,
        )
        .await
        .expect("assign_task with prefer_spawn should succeed");

    assert!(
        assign_output
            .output
            .contains("spawned by planner preference"),
        "expected planner-preference spawn in output, got: {}",
        assign_output.output
    );
    assert!(
        assign_output.output.contains("task-b"),
        "expected selected task id in output, got: {}",
        assign_output.output
    );

    let preferred_session = assign_output
        .output
        .strip_prefix("Task 'task-b' assigned to ")
        .and_then(|rest| rest.strip_suffix(" (spawned by planner preference)"))
        .expect("assign output should include preferred spawned session id")
        .trim()
        .to_string();

    assert_ne!(
        preferred_session, existing_worker,
        "prefer_spawn should choose a fresh worker instead of reusing the existing one"
    );

    wait_for_member_presence(&mut watcher, &watcher_session, &preferred_session)
        .await
        .expect("preferred spawned worker should appear in swarm");

    server_task.abort();
}
