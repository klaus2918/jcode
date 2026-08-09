fn generic_credential_paths_for_provider(
    provider: crate::provider_catalog::LoginProviderDescriptor,
) -> Vec<String> {
    let Ok(config_dir) = crate::storage::app_config_dir() else {
        return Vec::new();
    };

    match provider.target {
        crate::provider_catalog::LoginProviderTarget::Jcode => {
            vec![config_dir.join(crate::subscription_catalog::JCODE_ENV_FILE)]
        }
        crate::provider_catalog::LoginProviderTarget::OpenRouter => {
            vec![config_dir.join("openrouter.env")]
        }
        crate::provider_catalog::LoginProviderTarget::OpenAiApiKey => {
            vec![config_dir.join("openai.env")]
        }
        crate::provider_catalog::LoginProviderTarget::Azure => {
            vec![config_dir.join(crate::auth::azure::ENV_FILE)]
        }
        crate::provider_catalog::LoginProviderTarget::OpenAiCompatible(profile) => {
            // When a named config profile is active (selected via
            // `--provider-profile`), its credentials come from the profile's
            // configured `api_key_env`/`env_file`, not the built-in
            // `openai-compatible.env`. Report that path so the audit is accurate
            // (#402).
            if let Some((_key_env, env_file)) =
                crate::provider_catalog::active_named_provider_profile_credential_source()
            {
                vec![config_dir.join(env_file)]
            } else {
                let resolved =
                    crate::provider_catalog::resolve_openai_compatible_profile(profile);
                vec![config_dir.join(resolved.env_file)]
            }
        }
        _ => Vec::new(),
    }
    .into_iter()
    .map(|path| path.display().to_string())
    .collect()
}

fn auth_state_label(state: crate::auth::AuthState) -> &'static str {
    match state {
        crate::auth::AuthState::Available => "available",
        crate::auth::AuthState::Expired => "expired",
        crate::auth::AuthState::NotConfigured => "not_configured",
    }
}

fn probe_generic_provider_auth(
    provider: crate::provider_catalog::LoginProviderDescriptor,
    report: &mut AuthTestProviderReport,
) {
    // Keep generic provider probes provider-local. A DeepSeek/Z.AI/OpenRouter
    // auth-test should never be delayed or wedged by an unrelated Cursor/Gemini
    // external auth probe.
    let status = crate::auth::AuthStatus::check_fast();
    let assessment = status.assessment_for_provider(provider);
    report.push_step(
        "credential_probe",
        assessment.is_available(),
        format!(
            "{} auth status is {} ({}).",
            provider.display_name,
            auth_state_label(assessment.state),
            assessment.method_detail,
        ),
    );
    report.push_step(
        "refresh_probe",
        true,
        "Skipped: provider does not expose a dedicated refresh probe in jcode today.".to_string(),
    );
}

async fn probe_claude_auth(report: &mut AuthTestProviderReport) {
    if let Some(creds) = push_result_step(
        report,
        "credential_probe",
        crate::auth::claude::load_credentials(),
        |creds| {
            format!(
                "Loaded Claude credentials (expires_at={}).",
                creds.expires_at
            )
        },
    ) {
        push_result_step(
            report,
            "refresh_probe",
            crate::auth::oauth::refresh_claude_tokens(&creds.refresh_token).await,
            |tokens| {
                format!(
                    "Claude token refresh succeeded (new_expires_at={}).",
                    tokens.expires_at
                )
            },
        );
    }
}

async fn probe_openai_auth(report: &mut AuthTestProviderReport) {
    if let Some(creds) = push_result_step(
        report,
        "credential_probe",
        crate::auth::codex::load_credentials(),
        |creds| {
            if creds.refresh_token.trim().is_empty() {
                "Loaded OpenAI API key credentials (no refresh token present).".to_string()
            } else {
                format!(
                    "Loaded OpenAI OAuth credentials (expires_at={:?}).",
                    creds.expires_at
                )
            }
        },
    ) {
        if creds.refresh_token.trim().is_empty() {
            report.push_step(
                "refresh_probe",
                true,
                "Skipped: OpenAI is using API key auth, not OAuth.",
            );
        } else {
            push_result_step(
                report,
                "refresh_probe",
                crate::auth::oauth::refresh_openai_tokens(&creds.refresh_token).await,
                |tokens| {
                    format!(
                        "OpenAI token refresh succeeded (new_expires_at={}).",
                        tokens.expires_at
                    )
                },
            );
        }
    }
}

