use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub(super) struct DebugSnapshot {
    state: serde_json::Value,
    frame: Option<crate::tui::visual_debug::FrameCapture>,
    recent_messages: Vec<DebugMessage>,
    queued_messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DebugMessage {
    role: String,
    content: String,
    tool_calls: Vec<String>,
    duration_secs: Option<f32>,
    title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DebugAssertion {
    field: String,
    op: String,
    value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DebugAssertResult {
    ok: bool,
    field: String,
    op: String,
    expected: serde_json::Value,
    actual: serde_json::Value,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DebugStepResult {
    step: String,
    ok: bool,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DebugScript {
    steps: Vec<String>,
    assertions: Vec<DebugAssertion>,
    wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DebugRunReport {
    ok: bool,
    steps: Vec<DebugStepResult>,
    assertions: Vec<DebugAssertResult>,
}

fn estimate_display_message_bytes(message: &DisplayMessage) -> usize {
    message.role.len()
        + message.content.len()
        + message
            .tool_calls
            .iter()
            .map(|call| call.len())
            .sum::<usize>()
        + message.title.as_ref().map(|title| title.len()).unwrap_or(0)
        + message
            .tool_data
            .as_ref()
            .map(crate::process_memory::estimate_json_bytes)
            .unwrap_or(0)
}

fn estimate_string_vec_bytes(values: &[String]) -> usize {
    values.iter().map(|value| value.capacity()).sum()
}

fn estimate_pair_vec_bytes(values: &[(String, usize)]) -> usize {
    values.iter().map(|(name, _)| name.capacity()).sum()
}

fn estimate_pending_images_bytes(values: &[(String, String)]) -> usize {
    values
        .iter()
        .map(|(media_type, data)| media_type.capacity() + data.capacity())
        .sum()
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DebugEvent {
    at_ms: u64,
    kind: String,
    detail: String,
}

pub(super) struct DebugTrace {
    pub(super) enabled: bool,
    pub(super) started_at: Instant,
    pub(super) events: Vec<DebugEvent>,
}

impl DebugTrace {
    pub(super) fn new() -> Self {
        Self {
            enabled: false,
            started_at: Instant::now(),
            events: Vec::new(),
        }
    }

    pub(super) fn record(&mut self, kind: &str, detail: String) {
        if !self.enabled {
            return;
        }
        let at_ms = self.started_at.elapsed().as_millis() as u64;
        self.events.push(DebugEvent {
            at_ms,
            kind: kind.to_string(),
            detail,
        });
    }
}

const LARGE_DISPLAY_BLOB_THRESHOLD_BYTES: usize = 16 * 1024;

#[derive(Default)]
struct ProviderMessageMemoryStats {
    content_blocks: usize,
    text_bytes: usize,
    reasoning_bytes: usize,
    tool_use_input_json_bytes: usize,
    tool_result_bytes: usize,
    image_data_bytes: usize,
    openai_compaction_bytes: usize,
    large_blob_count: usize,
    large_blob_bytes: usize,
    large_tool_result_count: usize,
    large_tool_result_bytes: usize,
    max_block_bytes: usize,
}

impl ProviderMessageMemoryStats {
    fn record_bytes(&mut self, bytes: usize) {
        self.max_block_bytes = self.max_block_bytes.max(bytes);
        if bytes >= LARGE_DISPLAY_BLOB_THRESHOLD_BYTES {
            self.large_blob_count += 1;
            self.large_blob_bytes += bytes;
        }
    }

    fn record_message(&mut self, message: &crate::message::Message) {
        for block in &message.content {
            self.content_blocks += 1;
            match block {
                crate::message::ContentBlock::Text { text, .. } => {
                    self.text_bytes += text.len();
                    self.record_bytes(text.len());
                }
                crate::message::ContentBlock::Reasoning { text }
                | crate::message::ContentBlock::ReasoningTrace { text } => {
                    self.reasoning_bytes += text.len();
                    self.record_bytes(text.len());
                }
                crate::message::ContentBlock::AnthropicThinking {
                    thinking,
                    signature,
                } => {
                    let bytes = thinking.len() + signature.len();
                    self.reasoning_bytes += bytes;
                    self.record_bytes(bytes);
                }
                crate::message::ContentBlock::OpenAIReasoning {
                    id,
                    summary,
                    encrypted_content,
                    status,
                } => {
                    let bytes = id.len()
                        + summary.iter().map(String::len).sum::<usize>()
                        + encrypted_content.as_ref().map(String::len).unwrap_or(0)
                        + status.as_ref().map(String::len).unwrap_or(0);
                    self.reasoning_bytes += bytes;
                    self.record_bytes(bytes);
                }
                crate::message::ContentBlock::ToolUse { input, .. } => {
                    let bytes = crate::process_memory::estimate_json_bytes(input);
                    self.tool_use_input_json_bytes += bytes;
                    self.record_bytes(bytes);
                }
                crate::message::ContentBlock::ToolResult { content, .. } => {
                    self.tool_result_bytes += content.len();
                    if content.len() >= LARGE_DISPLAY_BLOB_THRESHOLD_BYTES {
                        self.large_tool_result_count += 1;
                        self.large_tool_result_bytes += content.len();
                    }
                    self.record_bytes(content.len());
                }
                crate::message::ContentBlock::Image { data, .. } => {
                    self.image_data_bytes += data.len();
                    self.record_bytes(data.len());
                }
                crate::message::ContentBlock::OpenAICompaction { encrypted_content } => {
                    self.openai_compaction_bytes += encrypted_content.len();
                    self.record_bytes(encrypted_content.len());
                }
            }
        }
    }

    fn payload_text_bytes(&self) -> usize {
        self.text_bytes
            + self.reasoning_bytes
            + self.tool_result_bytes
            + self.image_data_bytes
            + self.openai_compaction_bytes
    }
}

#[derive(Default)]
struct DisplayMessageMemoryStats {
    role_bytes: usize,
    content_bytes: usize,
    tool_call_text_bytes: usize,
    title_bytes: usize,
    tool_data_json_bytes: usize,
    large_content_count: usize,
    large_content_bytes: usize,
    max_content_bytes: usize,
}

impl DisplayMessageMemoryStats {
    fn record_message(&mut self, message: &DisplayMessage) {
        self.role_bytes += message.role.len();
        self.content_bytes += message.content.len();
        self.tool_call_text_bytes += message
            .tool_calls
            .iter()
            .map(|call| call.len())
            .sum::<usize>();
        self.title_bytes += message.title.as_ref().map(|title| title.len()).unwrap_or(0);
        self.tool_data_json_bytes += message
            .tool_data
            .as_ref()
            .map(crate::process_memory::estimate_json_bytes)
            .unwrap_or(0);
        self.max_content_bytes = self.max_content_bytes.max(message.content.len());
        if message.content.len() >= LARGE_DISPLAY_BLOB_THRESHOLD_BYTES {
            self.large_content_count += 1;
            self.large_content_bytes += message.content.len();
        }
    }
}

#[path = "debug_bench.rs"]
mod debug_bench;
#[path = "debug_cmds.rs"]
mod debug_cmds;
#[path = "debug_profile.rs"]
mod debug_profile;
#[path = "debug_script.rs"]
mod debug_script;

pub(super) fn handle_debug_command(app: &mut App, trimmed: &str) -> bool {
    if trimmed == "/debug-visual" || trimmed == "/debug-visual on" {
        use crate::tui::visual_debug;
        visual_debug::enable();
        app.push_display_message(DisplayMessage {
            role: "system".to_string(),
            content: "Visual debugging enabled. Frames are being captured.\n\
                     Use `/debug-visual dump` to write captured frames to file.\n\
                     Use `/debug-visual off` to disable."
                .to_string(),
            tool_calls: vec![],
            duration_secs: None,
            title: None,
            tool_data: None,
        });
        app.set_status_notice("Visual debug: ON");
        return true;
    }

    if trimmed == "/debug-visual off" {
        use crate::tui::visual_debug;
        visual_debug::disable();
        app.push_display_message(DisplayMessage {
            role: "system".to_string(),
            content: "Visual debugging disabled.".to_string(),
            tool_calls: vec![],
            duration_secs: None,
            title: None,
            tool_data: None,
        });
        app.set_status_notice("Visual debug: OFF");
        return true;
    }

    if trimmed == "/debug-visual dump" {
        use crate::tui::visual_debug;
        match visual_debug::dump_to_file() {
            Ok(path) => {
                app.push_display_message(DisplayMessage {
                    role: "system".to_string(),
                    content: format!(
                        "Visual debug dump written to:\n`{}`\n\n\
                         This file contains frame captures with:\n\
                         - Layout computations\n\
                         - State snapshots\n\
                         - Rendered text content\n\
                         - Any detected anomalies",
                        path.display()
                    ),
                    tool_calls: vec![],
                    duration_secs: None,
                    title: None,
                    tool_data: None,
                });
            }
            Err(e) => {
                app.push_display_message(DisplayMessage {
                    role: "error".to_string(),
                    content: format!("Failed to write visual debug dump: {}", e),
                    tool_calls: vec![],
                    duration_secs: None,
                    title: None,
                    tool_data: None,
                });
            }
        }
        return true;
    }

    if trimmed.starts_with("/debug-visual ") {
        app.push_display_message(DisplayMessage::error(
            "Usage: `/debug-visual` (on), `/debug-visual off`, `/debug-visual dump`".to_string(),
        ));
        return true;
    }

    false
}
