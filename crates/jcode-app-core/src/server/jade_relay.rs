use super::client_lifecycle::process_message_streaming_mpsc;
use super::state::{
    SessionInterruptQueues, queue_soft_interrupt_for_session, session_event_fanout_sender,
};
use super::{SessionAgents, SwarmMember};
use crate::config::SafetyConfig;
use anyhow::{Context, Result};
use jcode_agent_runtime::SoftInterruptSource;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const RELAY_LONG_POLL_SECONDS: u32 = 20;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const ERROR_BACKOFF: Duration = Duration::from_secs(10);
const MAX_RESPONSE_CHARS: usize = 12_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RelayListenerConfig {
    api_base: String,
    token: String,
    token_id: Option<String>,
    user_id: Option<String>,
    session_id: String,
    device_id: String,
}

impl RelayListenerConfig {
    fn from_safety(safety: &SafetyConfig) -> Option<Self> {
        if !safety.jade_relay_enabled || !safety.jade_relay_reply_enabled {
            return None;
        }
        let api_base = non_empty(safety.jade_relay_api_base.as_deref())?;
        let token = non_empty(safety.jade_relay_token.as_deref())?;
        let session_id = non_empty(safety.jade_relay_session_id.as_deref())?;
        Some(Self {
            api_base: normalize_api_base(api_base),
            token: token.to_string(),
            token_id: non_empty(safety.jade_relay_token_id.as_deref()).map(str::to_string),
            user_id: non_empty(safety.jade_relay_user_id.as_deref()).map(str::to_string),
            session_id: session_id.to_string(),
            device_id: default_device_id(),
        })
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalize_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim();
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

fn default_device_id() -> String {
    if let Ok(value) = std::env::var("JCODE_JADE_RELAY_DEVICE_ID")
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "device".to_string());
    format!("jcode-{host}")
}

pub(super) fn spawn_if_configured(
    safety: &SafetyConfig,
    sessions: SessionAgents,
    soft_interrupt_queues: SessionInterruptQueues,
    swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
) {
    let Some(config) = RelayListenerConfig::from_safety(safety) else {
        return;
    };
    crate::logging::info(&format!(
        "Starting Jade relay listener session={} user_id={}",
        config.session_id,
        config.user_id.as_deref().unwrap_or("<token-default>")
    ));
    tokio::spawn(async move {
        let client = RelayClient::new(config);
        client
            .run(sessions, soft_interrupt_queues, swarm_members)
            .await;
    });
}

struct RelayClient {
    config: RelayListenerConfig,
    http: reqwest::Client,
}

impl RelayClient {
    fn new(config: RelayListenerConfig) -> Self {
        Self {
            config,
            http: crate::provider::shared_http_client(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.api_base, path.trim_start_matches('/'))
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut req = req.header("Authorization", format!("Bearer {}", self.config.token));
        if let Some(token_id) = &self.config.token_id {
            req = req.header("x-jade-token-id", token_id);
        }
        req
    }

    async fn run(
        &self,
        sessions: SessionAgents,
        soft_interrupt_queues: SessionInterruptQueues,
        swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    ) {
        let mut after = match self.poll_prompts(0, 0).await {
            Ok(response) => response.next_after,
            Err(error) => {
                crate::logging::warn(&format!("Jade relay initial poll failed: {error:#}"));
                0
            }
        };
        let mut last_heartbeat = Instant::now()
            .checked_sub(HEARTBEAT_INTERVAL)
            .unwrap_or_else(Instant::now);

        loop {
            if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                if let Err(error) = self.heartbeat().await {
                    crate::logging::debug(&format!("Jade relay heartbeat failed: {error:#}"));
                }
                last_heartbeat = Instant::now();
            }

            match self.poll_prompts(after, RELAY_LONG_POLL_SECONDS).await {
                Ok(response) => {
                    after = response.next_after;
                    for event in response.events {
                        if event.seq > after {
                            after = event.seq;
                        }
                        if let Err(error) = self
                            .handle_prompt(
                                event,
                                &sessions,
                                &soft_interrupt_queues,
                                Arc::clone(&swarm_members),
                            )
                            .await
                        {
                            crate::logging::warn(&format!(
                                "Jade relay prompt handling failed: {error:#}"
                            ));
                        }
                    }
                }
                Err(error) => {
                    crate::logging::warn(&format!("Jade relay poll failed: {error:#}"));
                    tokio::time::sleep(ERROR_BACKOFF).await;
                }
            }
        }
    }

    async fn heartbeat(&self) -> Result<()> {
        let mut body = serde_json::json!({
            "device_id": &self.config.device_id,
            "label": &self.config.device_id,
            "platform": std::env::consts::OS,
            "session_id": &self.config.session_id,
            "app": "jcode",
        });
        add_user_id(&mut body, self.config.user_id.as_deref());
        let response = self
            .auth(self.http.post(self.url("v1/devices")).json(&body))
            .send()
            .await?;
        ensure_success(response, "heartbeat").await.map(|_| ())
    }

    async fn poll_prompts(&self, after: i64, wait: u32) -> Result<RelayEventsResponse> {
        let session = urlencoding_encode(&self.config.session_id);
        let mut params = vec![
            format!("after={}", after.max(0)),
            "types=prompt".to_string(),
            format!("wait={wait}"),
            "limit=100".to_string(),
        ];
        if let Some(user_id) = &self.config.user_id {
            params.push(format!("user_id={}", urlencoding_encode(user_id)));
        }
        let url = self.url(&format!(
            "v1/sessions/{}/events?{}",
            session,
            params.join("&")
        ));
        let response = self.auth(self.http.get(url)).send().await?;
        let response = ensure_success(response, "poll").await?;
        response
            .json::<RelayEventsResponse>()
            .await
            .context("decode relay poll response")
    }

    async fn post_relay_event(&self, event_type: &str, text: &str, request_seq: i64) -> Result<()> {
        let session = urlencoding_encode(&self.config.session_id);
        let mut body = serde_json::json!({
            "type": event_type,
            "text": truncate_chars(text, MAX_RESPONSE_CHARS),
            "request_seq": request_seq,
            "origin": &self.config.device_id,
        });
        add_user_id(&mut body, self.config.user_id.as_deref());
        let response = self
            .auth(
                self.http
                    .post(self.url(&format!("v1/sessions/{}/events", session)))
                    .json(&body),
            )
            .send()
            .await?;
        ensure_success(response, "post relay event")
            .await
            .map(|_| ())
    }

    async fn handle_prompt(
        &self,
        event: RelayEvent,
        sessions: &SessionAgents,
        soft_interrupt_queues: &SessionInterruptQueues,
        swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    ) -> Result<()> {
        let text = event.text.unwrap_or_default();
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        crate::logging::info(&format!(
            "Jade relay delivering prompt seq={} session={} chars={}",
            event.seq,
            self.config.session_id,
            text.chars().count()
        ));

        match deliver_to_session(
            &self.config.session_id,
            text,
            sessions,
            soft_interrupt_queues,
            swarm_members,
        )
        .await
        {
            Ok(reply) => self.post_relay_event("response", &reply, event.seq).await,
            Err(error) => {
                let message = format!("delivery failed: {error:#}");
                let _ = self.post_relay_event("error", &message, event.seq).await;
                Err(error)
            }
        }
    }
}

async fn ensure_success(response: reqwest::Response, action: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    anyhow::bail!("jade relay {action} failed ({status}): {body}")
}

fn add_user_id(body: &mut serde_json::Value, user_id: Option<&str>) {
    if let Some(user_id) = user_id
        && let Some(obj) = body.as_object_mut()
    {
        obj.insert(
            "user_id".to_string(),
            serde_json::Value::String(user_id.to_string()),
        );
    }
}

async fn deliver_to_session(
    session_id: &str,
    text: &str,
    sessions: &SessionAgents,
    soft_interrupt_queues: &SessionInterruptQueues,
    swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> Result<String> {
    let agent = {
        let guard = sessions.read().await;
        guard.get(session_id).cloned()
    };
    let Some(agent) = agent else {
        anyhow::bail!("session '{session_id}' is not live in this Jcode server")
    };

    if agent.try_lock().is_err() {
        let queued = queue_soft_interrupt_for_session(
            session_id,
            format!("[jade relay message from user]\n{text}"),
            false,
            SoftInterruptSource::User,
            soft_interrupt_queues,
            sessions,
        )
        .await;
        if queued {
            return Ok("Message queued for the running session.".to_string());
        }
        anyhow::bail!("session '{session_id}' is busy and could not accept a queued interrupt")
    }

    let start_message_index = {
        let agent_guard = agent.lock().await;
        agent_guard.message_count()
    };
    let event_tx = session_event_fanout_sender(session_id.to_string(), swarm_members);
    process_message_streaming_mpsc(Arc::clone(&agent), text, Vec::new(), None, event_tx).await?;
    let reply = {
        let agent_guard = agent.lock().await;
        agent_guard.latest_assistant_text_after(start_message_index)
    };
    Ok(reply.unwrap_or_else(|| "Message processed; no assistant text was produced.".to_string()))
}

#[derive(Debug, serde::Deserialize)]
struct RelayEventsResponse {
    #[serde(default)]
    events: Vec<RelayEvent>,
    #[serde(default)]
    next_after: i64,
}

#[derive(Debug, serde::Deserialize)]
struct RelayEvent {
    #[serde(default)]
    seq: i64,
    #[serde(default)]
    text: Option<String>,
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

/// Minimal percent-encoding for path/query segments (alnum and -_.~ pass through).
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_listener_config_is_opt_in_and_requires_credentials() {
        let cfg = SafetyConfig::default();
        assert!(RelayListenerConfig::from_safety(&cfg).is_none());

        let cfg = SafetyConfig {
            jade_relay_enabled: true,
            jade_relay_reply_enabled: true,
            ..SafetyConfig::default()
        };
        assert!(RelayListenerConfig::from_safety(&cfg).is_none());
    }

    #[test]
    fn relay_listener_config_accepts_complete_opt_in_config() {
        let cfg = SafetyConfig {
            jade_relay_enabled: true,
            jade_relay_reply_enabled: true,
            jade_relay_api_base: Some("https://example.com/api".to_string()),
            jade_relay_token: Some("tok".to_string()),
            jade_relay_token_id: Some("alice-token".to_string()),
            jade_relay_user_id: Some("alice".to_string()),
            jade_relay_session_id: Some("sess-1".to_string()),
            ..SafetyConfig::default()
        };
        let parsed = RelayListenerConfig::from_safety(&cfg).expect("complete config");
        assert_eq!(parsed.api_base, "https://example.com/api/");
        assert_eq!(parsed.token, "tok");
        assert_eq!(parsed.token_id.as_deref(), Some("alice-token"));
        assert_eq!(parsed.user_id.as_deref(), Some("alice"));
        assert_eq!(parsed.session_id, "sess-1");
    }

    #[test]
    fn relay_url_encoding_matches_jade_api_expectations() {
        assert_eq!(urlencoding_encode("sess-relay-test"), "sess-relay-test");
        assert_eq!(urlencoding_encode("a/b c"), "a%2Fb%20c");
        assert_eq!(urlencoding_encode("user.name~1_2"), "user.name~1_2");
    }

    #[test]
    fn truncation_preserves_short_text_and_marks_long_text() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
    }
}
