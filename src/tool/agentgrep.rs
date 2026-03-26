use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct AgentGrepInput {
    mode: String,
    #[serde(default)]
    query: Option<String>,
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

pub struct AgentGrepTool {
    binary_override: Option<PathBuf>,
}

impl AgentGrepTool {
    pub fn new() -> Self {
        Self {
            binary_override: None,
        }
    }

    fn with_binary_override(path: PathBuf) -> Self {
        Self {
            binary_override: Some(path),
        }
    }
}

#[async_trait]
impl Tool for AgentGrepTool {
    fn name(&self) -> &str {
        "agentgrep"
    }

    fn description(&self) -> &str {
        "Search a codebase using agentgrep. Supports exact grep, ranked file discovery, and structured smart search. Best for replacing the agent's first burst of grep/read calls with more grouped, structure-aware results."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["mode"],
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["grep", "find", "smart"],
                    "description": "Search mode: grep for exact content search, find for ranked file discovery, smart for structured investigation"
                },
                "query": {
                    "type": "string",
                    "description": "Query string for grep and find modes"
                },
                "terms": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Structured smart DSL terms for smart mode, e.g. [\"subject:auth_status\", \"relation:rendered\"]"
                },
                "regex": {
                    "type": "boolean",
                    "description": "For grep mode: treat query as a regular expression"
                },
                "path": {
                    "type": "string",
                    "description": "Optional root path to search instead of the current working directory"
                },
                "glob": {
                    "type": "string",
                    "description": "Restrict candidate files by glob"
                },
                "type": {
                    "type": "string",
                    "description": "Restrict to a known file type"
                },
                "hidden": {
                    "type": "boolean",
                    "description": "Include hidden files"
                },
                "no_ignore": {
                    "type": "boolean",
                    "description": "Ignore .gitignore and related ignore files"
                },
                "max_files": {
                    "type": "integer",
                    "description": "For find/smart: maximum files to return"
                },
                "max_regions": {
                    "type": "integer",
                    "description": "For smart: maximum regions to return"
                },
                "full_region": {
                    "type": "string",
                    "enum": ["auto", "always", "never"],
                    "description": "For smart: region expansion mode"
                },
                "debug_plan": {
                    "type": "boolean",
                    "description": "For smart: print planner details"
                },
                "debug_score": {
                    "type": "boolean",
                    "description": "For find/smart: print score details"
                },
                "paths_only": {
                    "type": "boolean",
                    "description": "Print only matching file paths"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: AgentGrepInput = serde_json::from_value(input)?;
        let binary = match resolve_agentgrep_binary(self.binary_override.as_deref()) {
            Some(path) => path,
            None => {
                return Ok(ToolOutput::new(
                    "agentgrep is not available. Install it or set JCODE_AGENTGREP_BIN to the agentgrep binary path.\n\nSearched PATH plus:\n- /home/jeremy/agentgrep/target/debug/agentgrep\n- /home/jeremy/agentgrep/target/release/agentgrep",
                )
                .with_title("agentgrep unavailable"));
            }
        };

        let args = build_agentgrep_args(&params, &ctx)?;
        let mut command = Command::new(&binary);
        command.args(&args);
        if let Some(ref dir) = ctx.working_dir {
            command.current_dir(dir);
        }

        let output = command.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            let detail = if stderr.is_empty() {
                stdout.clone()
            } else if stdout.is_empty() {
                stderr.clone()
            } else {
                format!("{}\n\n{}", stdout, stderr)
            };
            return Err(anyhow::anyhow!(
                "agentgrep {} failed with exit code {:?}: {}",
                params.mode,
                output.status.code(),
                detail.trim()
            ));
        }

        let mut rendered = if stdout.is_empty() {
            "agentgrep completed successfully (no output)".to_string()
        } else {
            stdout
        };
        if !stderr.is_empty() {
            rendered.push_str("\n\n[stderr]\n");
            rendered.push_str(&stderr);
        }

        Ok(ToolOutput::new(rendered).with_title(format!("agentgrep {}", params.mode)))
    }
}

fn build_agentgrep_args(params: &AgentGrepInput, ctx: &ToolContext) -> Result<Vec<OsString>> {
    let mut args = Vec::new();
    let mode = params.mode.as_str();
    match mode {
        "grep" | "find" | "smart" => args.push(OsString::from(mode)),
        _ => {
            return Err(anyhow::anyhow!(
                "Unsupported agentgrep mode: {}. Use grep, find, or smart.",
                params.mode
            ));
        }
    }

    match mode {
        "grep" => {
            let query = params
                .query
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("agentgrep grep requires 'query'"))?;
            if params.regex.unwrap_or(false) {
                args.push(OsString::from("--regex"));
            }
            push_common_flags(&mut args, params, ctx);
            args.push(OsString::from(query));
        }
        "find" => {
            let query = params
                .query
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("agentgrep find requires 'query'"))?;
            if params.debug_score.unwrap_or(false) {
                args.push(OsString::from("--debug-score"));
            }
            if let Some(max_files) = params.max_files {
                args.push(OsString::from("--max-files"));
                args.push(OsString::from(max_files.to_string()));
            }
            push_common_flags(&mut args, params, ctx);
            for part in query.split_whitespace() {
                args.push(OsString::from(part));
            }
        }
        "smart" => {
            let terms = params
                .terms
                .as_ref()
                .filter(|terms| !terms.is_empty())
                .ok_or_else(|| anyhow::anyhow!("agentgrep smart requires non-empty 'terms'"))?;
            if let Some(max_files) = params.max_files {
                args.push(OsString::from("--max-files"));
                args.push(OsString::from(max_files.to_string()));
            }
            if let Some(max_regions) = params.max_regions {
                args.push(OsString::from("--max-regions"));
                args.push(OsString::from(max_regions.to_string()));
            }
            if let Some(full_region) = params.full_region.as_deref() {
                args.push(OsString::from("--full-region"));
                args.push(OsString::from(full_region));
            }
            if params.debug_plan.unwrap_or(false) {
                args.push(OsString::from("--debug-plan"));
            }
            if params.debug_score.unwrap_or(false) {
                args.push(OsString::from("--debug-score"));
            }
            push_common_flags(&mut args, params, ctx);
            for term in terms {
                args.push(OsString::from(term));
            }
        }
        _ => unreachable!(),
    }

    Ok(args)
}

fn push_common_flags(args: &mut Vec<OsString>, params: &AgentGrepInput, ctx: &ToolContext) {
    if params.paths_only.unwrap_or(false) {
        args.push(OsString::from("--paths-only"));
    }
    if params.hidden.unwrap_or(false) {
        args.push(OsString::from("--hidden"));
    }
    if params.no_ignore.unwrap_or(false) {
        args.push(OsString::from("--no-ignore"));
    }
    if let Some(file_type) = params.file_type.as_deref() {
        args.push(OsString::from("--type"));
        args.push(OsString::from(file_type));
    }
    if let Some(glob) = params.glob.as_deref() {
        args.push(OsString::from("--glob"));
        args.push(OsString::from(glob));
    }
    if let Some(path) = params.path.as_deref() {
        args.push(OsString::from("--path"));
        args.push(resolve_path_arg(ctx, path).into_os_string());
    }
}

fn resolve_path_arg(ctx: &ToolContext, path: &str) -> PathBuf {
    ctx.resolve_path(Path::new(path))
}

fn resolve_agentgrep_binary(override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = override_path {
        if path.exists() {
            return Some(path.to_path_buf());
        }
    }

    if let Some(path) = std::env::var_os("JCODE_AGENTGREP_BIN") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    if let Some(path) = find_in_path(binary_name()) {
        return Some(path);
    }

    default_agentgrep_candidates()
        .into_iter()
        .find(|path| path.exists())
}

fn binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "agentgrep.exe"
    }
    #[cfg(not(windows))]
    {
        "agentgrep"
    }
}

fn default_agentgrep_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from(format!(
            "/home/jeremy/agentgrep/target/debug/{}",
            binary_name()
        )),
        PathBuf::from(format!(
            "/home/jeremy/agentgrep/target/release/{}",
            binary_name()
        )),
    ]
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_ctx(root: &Path) -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            tool_call_id: "test".to_string(),
            working_dir: Some(root.to_path_buf()),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: super::super::ToolExecutionMode::Direct,
        }
    }

    #[test]
    fn build_args_for_grep_includes_scope_flags() {
        let ctx = test_ctx(Path::new("/tmp/root"));
        let params = AgentGrepInput {
            mode: "grep".to_string(),
            query: Some("auth_status".to_string()),
            terms: None,
            regex: Some(true),
            path: Some("src".to_string()),
            glob: Some("src/**/*.rs".to_string()),
            file_type: Some("rs".to_string()),
            hidden: Some(true),
            no_ignore: Some(true),
            max_files: None,
            max_regions: None,
            full_region: None,
            debug_plan: None,
            debug_score: None,
            paths_only: Some(true),
        };

        let args = build_agentgrep_args(&params, &ctx).unwrap();
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            rendered,
            vec![
                "grep",
                "--regex",
                "--paths-only",
                "--hidden",
                "--no-ignore",
                "--type",
                "rs",
                "--glob",
                "src/**/*.rs",
                "--path",
                "/tmp/root/src",
                "auth_status"
            ]
        );
    }

    #[test]
    fn build_args_for_smart_uses_terms() {
        let ctx = test_ctx(Path::new("/workspace"));
        let params = AgentGrepInput {
            mode: "smart".to_string(),
            query: None,
            terms: Some(vec![
                "subject:auth_status".to_string(),
                "relation:rendered".to_string(),
                "path:src/tui".to_string(),
            ]),
            regex: None,
            path: Some("repo".to_string()),
            glob: None,
            file_type: Some("rs".to_string()),
            hidden: None,
            no_ignore: None,
            max_files: Some(3),
            max_regions: Some(4),
            full_region: Some("auto".to_string()),
            debug_plan: Some(true),
            debug_score: Some(true),
            paths_only: None,
        };

        let args = build_agentgrep_args(&params, &ctx).unwrap();
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            rendered,
            vec![
                "smart",
                "--max-files",
                "3",
                "--max-regions",
                "4",
                "--full-region",
                "auto",
                "--debug-plan",
                "--debug-score",
                "--type",
                "rs",
                "--path",
                "/workspace/repo",
                "subject:auth_status",
                "relation:rendered",
                "path:src/tui"
            ]
        );
    }

    #[tokio::test]
    async fn missing_binary_returns_helpful_output() {
        let temp = tempfile::tempdir().expect("tempdir");
        let tool = AgentGrepTool::with_binary_override(temp.path().join("missing-agentgrep"));
        let ctx = test_ctx(temp.path());
        let output = tool
            .execute(json!({"mode": "grep", "query": "lsp"}), ctx)
            .await
            .expect("tool output");
        assert!(output.output.contains("agentgrep is not available"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn execute_runs_configured_binary() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let script = temp.path().join("fake-agentgrep");
        fs::write(&script, "#!/usr/bin/env bash\nprintf 'args:%s\n' \"$*\"\n")
            .expect("write script");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod");

        let tool = AgentGrepTool::with_binary_override(script);
        let ctx = test_ctx(temp.path());
        let output = tool
            .execute(
                json!({
                    "mode": "smart",
                    "terms": ["subject:lsp", "relation:implementation"],
                    "path": "repo",
                    "max_files": 2,
                    "max_regions": 3,
                    "debug_plan": true
                }),
                ctx,
            )
            .await
            .expect("agentgrep execution");
        assert!(
            output
                .output
                .contains("args:smart --max-files 2 --max-regions 3 --debug-plan --path")
        );
        assert!(
            output
                .output
                .contains("subject:lsp relation:implementation")
        );
    }
}
