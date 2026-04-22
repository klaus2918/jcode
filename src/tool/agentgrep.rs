use super::{Tool, ToolContext, ToolOutput};
use crate::message::{ContentBlock, ToolCall};
use crate::session::Session;
use crate::storage;
use crate::{logging, util};
use ::agentgrep::cli::{FindArgs, FullRegionMode, GrepArgs, OutlineArgs, SmartArgs};
use ::agentgrep::find::{FindFile, FindResult, run_find};
use ::agentgrep::outline::{OutlineResult, run_outline};
use ::agentgrep::search::{FileMatches, GrepResult, run_grep};
use ::agentgrep::smart_dsl::{Relation, SmartQuery, parse_smart_query};
use ::agentgrep::smart_engine::{SmartFile, SmartRegion, SmartResult, run_smart};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod args;
mod context;
mod render;

#[cfg(test)]
use self::args::trace_or_smart_terms_owned;
use self::args::{
    build_find_args, build_grep_args, build_outline_args, build_smart_args_and_query,
    resolve_search_root, summarize_agentgrep_request,
};
use self::context::maybe_write_context_json;
#[cfg(test)]
use self::context::{
    collect_bash_exposure, collect_trace_exposure, tune_known_file, tune_known_region,
};
use self::render::{
    render_find_output, render_grep_output, render_outline_output, render_smart_output,
};

#[derive(Debug, Deserialize)]
struct AgentGrepInput {
    mode: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    terms: Option<Vec<String>>,
    #[serde(default)]
    regex: Option<bool>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(rename = "type", default)]
    file_type: Option<String>,
    #[serde(default)]
    hidden: Option<bool>,
    #[serde(default)]
    no_ignore: Option<bool>,
    #[serde(default)]
    max_files: Option<usize>,
    #[serde(default)]
    max_regions: Option<usize>,
    #[serde(default)]
    full_region: Option<String>,
    #[serde(default)]
    debug_plan: Option<bool>,
    #[serde(default)]
    debug_score: Option<bool>,
    #[serde(default)]
    paths_only: Option<bool>,
}

#[derive(Debug, Serialize, Default)]
struct AgentGrepHarnessContext {
    version: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    known_regions: Vec<AgentGrepKnownRegion>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    known_files: Vec<AgentGrepKnownFile>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    known_symbols: Vec<AgentGrepKnownSymbol>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    focus_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentGrepKnownRegion {
    path: String,
    start_line: usize,
    end_line: usize,
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
    reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AgentGrepKnownFile {
    path: String,
    structure_confidence: f32,
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
    reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AgentGrepKnownSymbol {
    path: String,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    structure_confidence: f32,
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
    reasons: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy)]
struct RegionConfidenceProfile {
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
}

#[derive(Debug, Clone)]
struct PendingTraceRegion {
    path: String,
    kind: Option<&'static str>,
    start_line: usize,
    end_line: usize,
}

#[derive(Debug, Clone)]
struct ToolExposureObservation {
    tool: ToolCall,
    content: String,
    timestamp: Option<DateTime<Utc>>,
    message_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct ExposureDescriptor {
    timestamp: Option<DateTime<Utc>>,
    message_index: usize,
    total_messages: usize,
    compaction_cutoff: Option<usize>,
}

pub struct AgentGrepTool;

impl AgentGrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AgentGrepTool {
    fn name(&self) -> &str {
        "agentgrep"
    }

    fn description(&self) -> &str {
        "Search code."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["mode"],
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["grep", "find", "outline", "trace"],
                    "description": "Mode."
                },
                "query": {
                    "type": "string"
                },
                "file": {
                    "type": "string"
                },
                "terms": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Terms."
                },
                "regex": {
                    "type": "boolean",
                    "description": "Regex."
                },
                "path": {
                    "type": "string",
                    "description": "Root path."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob."
                },
                "type": {
                    "type": "string",
                    "description": "File type."
                },
                "max_files": {
                    "type": "integer",
                    "description": "Max files."
                },
                "max_regions": {
                    "type": "integer",
                    "description": "Max regions."
                },
                "paths_only": {
                    "type": "boolean",
                    "description": "Paths only."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: AgentGrepInput = serde_json::from_value(input)?;
        let context_path = maybe_write_context_json(&params, &ctx)?;
        let request = summarize_agentgrep_request(&params, &ctx, context_path.as_deref());
        let started_at = std::time::Instant::now();
        let outcome = execute_linked_agentgrep(&params, &ctx, context_path.as_deref());
        let elapsed_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

        if let Some(path) = context_path {
            let _ = std::fs::remove_file(path);
        }

        match outcome {
            Ok(output) => {
                if elapsed_ms >= 2_000 {
                    logging::warn(&format!(
                        "agentgrep slow mode={} elapsed_ms={} request={}",
                        params.mode, elapsed_ms, request
                    ));
                }
                Ok(output)
            }
            Err(err) => {
                let detail = err.to_string();
                let detail = util::truncate_str(detail.trim(), 600);
                logging::warn(&format!(
                    "agentgrep failure mode={} elapsed_ms={} request={} error={}",
                    params.mode, elapsed_ms, request, detail
                ));
                Err(anyhow::anyhow!(
                    "agentgrep {} failed after {}ms: {}",
                    params.mode,
                    elapsed_ms,
                    err
                ))
            }
        }
    }
}

fn execute_linked_agentgrep(
    params: &AgentGrepInput,
    ctx: &ToolContext,
    context_json_path: Option<&Path>,
) -> Result<ToolOutput> {
    match params.mode.as_str() {
        "grep" => {
            let args = build_grep_args(params, ctx)?;
            let root = resolve_search_root(ctx, args.path.as_deref());
            let result = run_grep(&root, &args).map_err(anyhow::Error::msg)?;
            Ok(ToolOutput::new(render_grep_output(&result, &args)).with_title("agentgrep grep"))
        }
        "find" => {
            let args = build_find_args(params, ctx)?;
            let root = resolve_search_root(ctx, args.path.as_deref());
            let result = run_find(&root, &args);
            Ok(ToolOutput::new(render_find_output(&result, &args)).with_title("agentgrep find"))
        }
        "outline" => {
            let args = build_outline_args(params, ctx, context_json_path)?;
            let root = resolve_search_root(ctx, args.path.as_deref());
            let result = run_outline(&root, &args).map_err(anyhow::Error::msg)?;
            Ok(ToolOutput::new(render_outline_output(&result)).with_title("agentgrep outline"))
        }
        "trace" | "smart" => {
            let (args, query) = build_smart_args_and_query(params, ctx, context_json_path)?;
            let root = resolve_search_root(ctx, args.path.as_deref());
            let result = run_smart(&root, &query, &args).map_err(anyhow::Error::msg)?;
            Ok(ToolOutput::new(render_smart_output(&result, &args))
                .with_title(format!("agentgrep {}", params.mode)))
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported agentgrep mode: {}. Use grep, find, outline, or trace.",
            params.mode
        )),
    }
}

fn resolve_path_arg(ctx: &ToolContext, path: &str) -> PathBuf {
    ctx.resolve_path(Path::new(path))
}

fn normalized_agentgrep_glob(glob: Option<&str>) -> Option<&str> {
    let glob = glob?.trim();
    if glob.is_empty() {
        return None;
    }

    if is_match_all_glob(glob) {
        return None;
    }

    Some(glob)
}

fn normalized_agentgrep_glob_owned(glob: Option<&str>) -> Option<String> {
    normalized_agentgrep_glob(glob).map(ToOwned::to_owned)
}

fn is_match_all_glob(glob: &str) -> bool {
    matches!(glob, "*" | "**" | "**/*" | "./*" | "./**" | "./**/*")
}

#[cfg(test)]
#[path = "agentgrep_tests.rs"]
mod tests;
