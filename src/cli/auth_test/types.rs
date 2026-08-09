#[expect(
    clippy::large_enum_variant,
    reason = "Generic auth-test targets carry provider descriptors until this CLI path is refactored"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedAuthTestTarget {
    Detailed(AuthTestTarget),
    Generic {
        provider: crate::provider_catalog::LoginProviderDescriptor,
        choice: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthTestTarget {
    Claude,
    Openai,
}

impl AuthTestTarget {
    fn provider_id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Openai => "openai",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Openai => "openai",
        }
    }

    fn from_provider_id(choice: &str) -> Option<Self> {
        match choice.trim() {
            "claude" | crate::cli::provider_init::CLAUDE_SUBPROCESS_ID => Some(Self::Claude),
            "openai" => Some(Self::Openai),
            _ => None,
        }
    }

    fn credential_paths(self) -> Result<Vec<String>> {
        match self {
            Self::Claude => Ok(vec![
                crate::auth::claude::jcode_path()?.display().to_string(),
                crate::storage::user_home_path(".claude/.credentials.json")?
                    .display()
                    .to_string(),
                crate::storage::user_home_path(".local/share/opencode/auth.json")?
                    .display()
                    .to_string(),
                crate::storage::user_home_path(".pi/agent/auth.json")?
                    .display()
                    .to_string(),
            ]),
            Self::Openai => Ok(vec![
                crate::storage::jcode_dir()?
                    .join("openai-auth.json")
                    .display()
                    .to_string(),
                crate::storage::user_home_path(".codex/auth.json")?
                    .display()
                    .to_string(),
                crate::storage::user_home_path(".local/share/opencode/auth.json")?
                    .display()
                    .to_string(),
                crate::storage::user_home_path(".pi/agent/auth.json")?
                    .display()
                    .to_string(),
            ]),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuthTestStepReport {
    name: String,
    ok: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct AuthTestProviderReport {
    provider: String,
    credential_paths: Vec<String>,
    steps: Vec<AuthTestStepReport>,
    smoke_output: Option<String>,
    tool_smoke_output: Option<String>,
    success: bool,
}

#[derive(Debug, Serialize)]
struct AuthTestContextModelReport {
    model: String,
    catalog_context_window: usize,
    resolved_context_window: usize,
    ok: bool,
}

#[derive(Debug, Serialize)]
struct AuthTestContextAuditReport {
    provider: String,
    display_name: String,
    checked_models: usize,
    skipped_models_without_context: usize,
    mismatches: Vec<AuthTestContextModelReport>,
    success: bool,
    detail: String,
}

impl AuthTestProviderReport {
    fn new(target: AuthTestTarget) -> Self {
        Self {
            provider: target.label().to_string(),
            credential_paths: target.credential_paths().unwrap_or_default(),
            steps: Vec::new(),
            smoke_output: None,
            tool_smoke_output: None,
            success: true,
        }
    }

    fn new_generic(provider_id: String, credential_paths: Vec<String>) -> Self {
        Self {
            provider: provider_id,
            credential_paths,
            steps: Vec::new(),
            smoke_output: None,
            tool_smoke_output: None,
            success: true,
        }
    }

    fn push_step(&mut self, name: impl Into<String>, ok: bool, detail: impl Into<String>) {
        if !ok {
            self.success = false;
        }
        self.steps.push(AuthTestStepReport {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
    }
}

impl ResolvedAuthTestTarget {
    fn from_choice(choice: &str) -> Option<Self> {
        let trimmed = choice.trim();
        if trimmed.eq_ignore_ascii_case(super::provider_init::CLAUDE_SUBPROCESS_ID) {
            return Some(Self::Detailed(AuthTestTarget::Claude));
        }
        let provider = crate::provider_catalog::resolve_login_provider(trimmed)?;
        Some(match AuthTestTarget::from_provider_id(provider.id) {
            Some(target) => Self::Detailed(target),
            None => Self::Generic {
                provider,
                choice: trimmed.to_string(),
            },
        })
    }

    fn from_provider(provider: crate::provider_catalog::LoginProviderDescriptor) -> Option<Self> {
        Some(match AuthTestTarget::from_provider_id(provider.id) {
            Some(target) => Self::Detailed(target),
            None => Self::Generic {
                provider,
                choice: provider.id.to_string(),
            },
        })
    }
}

#[derive(Clone, Copy)]
enum AuthTestSmokeKind {
    Provider,
    Tool,
}

impl AuthTestSmokeKind {
    fn step_name(self) -> &'static str {
        match self {
            Self::Provider => "provider_smoke",
            Self::Tool => "tool_smoke",
        }
    }

    fn skipped_by_flag_detail(self) -> &'static str {
        match self {
            Self::Provider => "Skipped by --no-smoke.",
            Self::Tool => "Skipped by --no-tool-smoke.",
        }
    }

    fn success_detail(self) -> &'static str {
        match self {
            Self::Provider => "Provider returned AUTH_TEST_OK.",
            Self::Tool => {
                "Tool-enabled provider request returned AUTH_TEST_OK after one validated real Jcode bash tool call, successful registry execution, and tool-result followup."
            }
        }
    }

    fn failure_detail(self, output: &str) -> String {
        match self {
            Self::Provider => {
                format!("Provider response did not contain AUTH_TEST_OK: {}", output)
            }
            Self::Tool => format!(
                "Tool-enabled provider response did not contain AUTH_TEST_OK: {}",
                output
            ),
        }
    }

    async fn run(
        self,
        target: AuthTestTarget,
        model: Option<&str>,
        prompt: &str,
    ) -> Result<String> {
        self.run_for_choice(target.provider_id(), model, prompt)
            .await
    }

    async fn run_for_choice(
        self,
        choice: &str,
        model: Option<&str>,
        prompt: &str,
    ) -> Result<String> {
        match self {
            Self::Provider => run_provider_smoke_for_choice(choice, model, prompt).await,
            Self::Tool => run_provider_tool_smoke_for_choice(choice, model, prompt).await,
        }
    }

    fn set_output(self, report: &mut AuthTestProviderReport, output: String) {
        match self {
            Self::Provider => report.smoke_output = Some(output),
            Self::Tool => report.tool_smoke_output = Some(output),
        }
    }
}

fn push_result_step<T, E, F>(
    report: &mut AuthTestProviderReport,
    name: &'static str,
    result: std::result::Result<T, E>,
    detail: F,
) -> Option<T>
where
    E: std::fmt::Display,
    F: FnOnce(&T) -> String,
{
    match result {
        Ok(value) => {
            report.push_step(name, true, detail(&value));
            Some(value)
        }
        Err(err) => {
            report.push_step(name, false, err.to_string());
            None
        }
    }
}

