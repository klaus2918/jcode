use super::*;

pub(super) fn auto_scriptable_flow_reason(
    provider: LoginProviderDescriptor,
    options: &LoginOptions,
    stdin_is_terminal: bool,
) -> Option<&'static str> {
    if options.print_auth_url || options.complete || options.has_provided_input() {
        return None;
    }

    let supports_scriptable = matches!(
        provider.target,
        LoginProviderTarget::Claude | LoginProviderTarget::OpenAi | LoginProviderTarget::Google
    );
    if !supports_scriptable {
        return None;
    }

    if !stdin_is_terminal {
        Some("non_interactive_terminal")
    } else if auth::browser_suppressed(options.no_browser) {
        Some("no_browser_requested")
    } else {
        None
    }
}

pub(super) async fn run_scriptable_login_provider(
    provider: LoginProviderDescriptor,
    account_label: Option<&str>,
    options: &LoginOptions,
) -> Result<LoginFlowOutcome> {
    if options.print_auth_url {
        return start_scriptable_login(provider, account_label, options).await;
    }

    let input = options.resolve_provided_input()?;
    if options.complete && input.is_some() {
        anyhow::bail!(
            "Use either --complete or an explicit --callback-url / --auth-code input, not both."
        );
    }
    complete_scriptable_login(provider, account_label, options, input).await
}

/// Run the normal Google OAuth flow when the caller is noninteractive but a
/// browser is available. The scriptable flow is normally used in that
/// environment to avoid prompts, but Google needs a live localhost callback
/// listener or the browser lands on a confusing connection error page.
pub(super) async fn run_automatic_google_login(
    provider_id: &str,
    options: &LoginOptions,
) -> Result<LoginFlowOutcome> {
    let tier = options
        .google_access_tier
        .unwrap_or(auth::google::GmailAccessTier::Full);
    let tokens = auth::google::login(tier, options.no_browser).await?;
    let credentials_path = auth::google::credentials_path()?;
    let tokens_path = auth::google::tokens_path()?;

    emit_scriptable_auth_success(
        options.json,
        ScriptableAuthSuccess {
            status: "authenticated",
            provider: provider_id.to_string(),
            account_label: None,
            credentials_path: Some(credentials_path.display().to_string()),
            email: tokens.email.clone(),
        },
    )?;
    if !options.json {
        eprintln!("\nGmail setup complete!");
        if let Some(email) = tokens.email {
            eprintln!("Account: {}", email);
        }
        eprintln!("Access tier: {}", tokens.tier.label());
        eprintln!("Tokens saved to {}", tokens_path.display());
    }

    Ok(LoginFlowOutcome::Completed)
}

pub(super) async fn start_scriptable_login(
    provider: LoginProviderDescriptor,
    account_label: Option<&str>,
    options: &LoginOptions,
) -> Result<LoginFlowOutcome> {
    let (pending, auth_url, input_kind, user_code, expires_at_ms) = match provider.target {
        LoginProviderTarget::Claude => {
            let label = auth::claude::login_target_label(account_label)?;
            let (verifier, challenge) = auth::oauth::generate_pkce_public();
            let redirect_uri = auth::oauth::claude::REDIRECT_URI.to_string();
            let auth_url = auth::oauth::claude_auth_url(&redirect_uri, &challenge, &verifier);
            (
                PendingScriptableLogin::Claude {
                    account_label: label,
                    verifier,
                    redirect_uri,
                },
                auth_url,
                "auth_code_or_callback_url",
                None::<String>,
                PendingScriptableLogin::Claude {
                    account_label: String::new(),
                    verifier: String::new(),
                    redirect_uri: String::new(),
                }
                .default_expires_at_ms(),
            )
        }
        LoginProviderTarget::OpenAi => {
            let label = auth::codex::login_target_label(account_label)?;
            let (verifier, challenge) = auth::oauth::generate_pkce_public();
            let state = auth::oauth::generate_state_public();
            let redirect_uri = auth::oauth::openai::default_redirect_uri();
            let auth_url = auth::oauth::openai_auth_url_with_prompt(
                &redirect_uri,
                &challenge,
                &state,
                Some("login"),
            );
            (
                PendingScriptableLogin::Openai {
                    account_label: label,
                    verifier,
                    state,
                    redirect_uri,
                },
                auth_url,
                "callback_url",
                None::<String>,
                PendingScriptableLogin::Openai {
                    account_label: String::new(),
                    verifier: String::new(),
                    state: String::new(),
                    redirect_uri: String::new(),
                }
                .default_expires_at_ms(),
            )
        }
        LoginProviderTarget::Google => {
            let creds = auth::google::load_credentials().context(
                "Google/Gmail scriptable auth requires saved OAuth credentials first. Run `jcode login --provider google` once or save google credentials manually.",
            )?;
            let tier = options
                .google_access_tier
                .unwrap_or(auth::google::GmailAccessTier::Full);
            let (verifier, challenge) = auth::oauth::generate_pkce_public();
            let state = auth::oauth::generate_state_public();
            let redirect_uri = format!("http://127.0.0.1:{}", auth::google::DEFAULT_PORT);
            let auth_url =
                auth::google::build_auth_url(&creds, tier, &redirect_uri, &challenge, &state);
            (
                PendingScriptableLogin::Google {
                    verifier,
                    state,
                    redirect_uri,
                    tier,
                },
                auth_url,
                "callback_url",
                None::<String>,
                PendingScriptableLogin::Google {
                    verifier: String::new(),
                    state: String::new(),
                    redirect_uri: String::new(),
                    tier,
                }
                .default_expires_at_ms(),
            )
        }
        _ => {
            anyhow::bail!("`--print-auth-url` is currently supported for: claude, openai, google.")
        }
    };

    let pending_path = pending.pending_path()?;
    cleanup_stale_pending_login_files()?;
    let record = PendingScriptableLoginRecord {
        expires_at_ms,
        login: pending,
    };
    crate::storage::write_json_secret(&pending_path, &record)?;
    emit_scriptable_auth_prompt(
        provider.id,
        &auth_url,
        input_kind,
        &pending_path,
        user_code.as_deref(),
        expires_at_ms,
        options.json,
    )?;
    Ok(LoginFlowOutcome::Deferred)
}

pub(super) async fn complete_scriptable_login(
    provider: LoginProviderDescriptor,
    account_label: Option<&str>,
    options: &LoginOptions,
    input: Option<ProvidedAuthInput>,
) -> Result<LoginFlowOutcome> {
    if account_label.is_some() {
        anyhow::bail!(
            "Do not pass --account when completing a scriptable login. The pending login already stores the target account."
        );
    }

    match provider.target {
        LoginProviderTarget::Claude => {
            complete_scriptable_claude_login(provider.id, options, require_scriptable_input(input)?)
                .await
        }
        LoginProviderTarget::OpenAi => {
            complete_scriptable_openai_login(provider.id, options, require_scriptable_input(input)?)
                .await
        }
        LoginProviderTarget::Google => {
            complete_scriptable_google_login(provider.id, options, require_scriptable_input(input)?)
                .await
        }
        _ => anyhow::bail!(
            "Scriptable completion is currently supported for: claude, openai, google."
        ),
    }
}

pub(super) async fn complete_scriptable_claude_login(
    provider_id: &str,
    options: &LoginOptions,
    input: ProvidedAuthInput,
) -> Result<LoginFlowOutcome> {
    let pending_path = pending_login_path("claude")?;
    let PendingScriptableLogin::Claude {
        account_label,
        verifier,
        redirect_uri,
    } = load_pending_login(&pending_path, "claude")?
    else {
        anyhow::bail!("Pending Claude login state is invalid.");
    };

    let raw_input = match input {
        ProvidedAuthInput::CallbackUrl(value) | ProvidedAuthInput::AuthCode(value) => value,
    };
    let selected_redirect_uri =
        auth::oauth::claude_redirect_uri_for_input(&raw_input, &redirect_uri);
    let tokens =
        auth::oauth::exchange_claude_code(&verifier, &raw_input, &selected_redirect_uri).await?;
    auth::oauth::save_claude_tokens_for_account(&tokens, &account_label)?;
    let profile_email =
        auth::oauth::update_claude_account_profile(&account_label, &tokens.access_token)
            .await
            .unwrap_or(None);
    clear_pending_login(&pending_path);

    emit_scriptable_auth_success(
        options.json,
        ScriptableAuthSuccess {
            status: "authenticated",
            provider: provider_id.to_string(),
            account_label: Some(account_label.clone()),
            credentials_path: Some(auth::claude::jcode_path()?.display().to_string()),
            email: profile_email.clone(),
        },
    )?;
    if !options.json {
        eprintln!("Successfully logged in to Claude!");
        eprintln!(
            "Account '{}' stored at {}",
            account_label,
            auth::claude::jcode_path()?.display()
        );
        if let Some(email) = profile_email {
            eprintln!("Profile email: {}", email);
        }
    }
    Ok(LoginFlowOutcome::Completed)
}

pub(super) async fn complete_scriptable_openai_login(
    provider_id: &str,
    options: &LoginOptions,
    input: ProvidedAuthInput,
) -> Result<LoginFlowOutcome> {
    let pending_path = pending_login_path("openai")?;
    let PendingScriptableLogin::Openai {
        account_label,
        verifier,
        state,
        redirect_uri,
    } = load_pending_login(&pending_path, "openai")?
    else {
        anyhow::bail!("Pending OpenAI login state is invalid.");
    };

    let callback_input = match input {
        ProvidedAuthInput::CallbackUrl(value) => value,
        ProvidedAuthInput::AuthCode(_) => {
            anyhow::bail!(
                "OpenAI completion requires --callback-url because state validation is required."
            )
        }
    };
    let tokens = auth::oauth::exchange_openai_callback_input(
        &verifier,
        &callback_input,
        &state,
        &redirect_uri,
    )
    .await?;
    auth::oauth::save_openai_tokens_for_account(&tokens, &account_label)?;
    clear_pending_login(&pending_path);

    let credentials_path = crate::storage::jcode_dir()?.join("openai-auth.json");
    emit_scriptable_auth_success(
        options.json,
        ScriptableAuthSuccess {
            status: "authenticated",
            provider: provider_id.to_string(),
            account_label: Some(account_label.clone()),
            credentials_path: Some(credentials_path.display().to_string()),
            email: None,
        },
    )?;
    if !options.json {
        eprintln!(
            "Successfully logged in to OpenAI! Account '{}' saved to {}",
            account_label,
            credentials_path.display()
        );
    }
    Ok(LoginFlowOutcome::Completed)
}

pub(super) async fn complete_scriptable_google_login(
    provider_id: &str,
    options: &LoginOptions,
    input: ProvidedAuthInput,
) -> Result<LoginFlowOutcome> {
    let pending_path = pending_login_path("google")?;
    let PendingScriptableLogin::Google {
        verifier,
        state,
        redirect_uri,
        tier,
    } = load_pending_login(&pending_path, "google")?
    else {
        anyhow::bail!("Pending Google login state is invalid.");
    };

    let callback_input = match input {
        ProvidedAuthInput::CallbackUrl(value) => value,
        ProvidedAuthInput::AuthCode(_) => {
            anyhow::bail!("Google completion requires --callback-url.")
        }
    };
    let creds = auth::google::load_credentials().context(
        "Google/Gmail completion requires saved OAuth credentials first. Run `jcode login --provider google` once or save google credentials manually.",
    )?;
    let tokens = auth::google::exchange_callback_input(
        &creds,
        &verifier,
        &callback_input,
        &state,
        &redirect_uri,
        tier,
    )
    .await?;
    clear_pending_login(&pending_path);

    emit_scriptable_auth_success(
        options.json,
        ScriptableAuthSuccess {
            status: "authenticated",
            provider: provider_id.to_string(),
            account_label: None,
            credentials_path: Some(auth::google::tokens_path()?.display().to_string()),
            email: tokens.email.clone(),
        },
    )?;
    if !options.json {
        eprintln!("Successfully logged in to Google/Gmail!");
        if let Some(email) = tokens.email.as_deref() {
            eprintln!("Account: {}", email);
        }
        eprintln!("Access tier: {}", tokens.tier.label());
        eprintln!("Tokens saved to {}", auth::google::tokens_path()?.display());
    }
    Ok(LoginFlowOutcome::Completed)
}

pub(super) fn pending_login_path(key: &str) -> Result<PathBuf> {
    Ok(crate::storage::jcode_dir()?
        .join("pending-login")
        .join(format!("{key}.json")))
}

pub(super) fn pending_login_dir() -> Result<PathBuf> {
    Ok(crate::storage::jcode_dir()?.join("pending-login"))
}

pub(super) fn require_scriptable_input(
    input: Option<ProvidedAuthInput>,
) -> Result<ProvidedAuthInput> {
    input.ok_or_else(|| anyhow::anyhow!("No scriptable auth input was provided."))
}

pub(super) fn load_pending_login(path: &PathBuf, provider: &str) -> Result<PendingScriptableLogin> {
    if !path.exists() {
        anyhow::bail!(
            "No pending {} login state found. Run `jcode login --provider {} --print-auth-url` first.",
            provider,
            provider
        );
    }
    crate::storage::harden_secret_file_permissions(path);
    let data = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Failed to read pending {} login state from {}",
            provider,
            path.display()
        )
    })?;
    if let Ok(record) = serde_json::from_str::<PendingScriptableLoginRecord>(&data) {
        if record.expires_at_ms <= current_time_ms() {
            clear_pending_login(path);
            anyhow::bail!(
                "Pending {} login state expired. Run `jcode login --provider {} --print-auth-url` again.",
                provider,
                provider
            );
        }
        cleanup_stale_pending_login_files()?;
        return Ok(record.login);
    }
    let login = serde_json::from_str::<PendingScriptableLogin>(&data).with_context(|| {
        format!(
            "Failed to load pending {} login state from {}",
            provider,
            path.display()
        )
    })?;
    cleanup_stale_pending_login_files()?;
    Ok(login)
}

pub(super) fn clear_pending_login(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

pub(super) fn cleanup_stale_pending_login_files() -> Result<()> {
    let dir = pending_login_dir()?;
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(data) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<PendingScriptableLoginRecord>(&data) else {
            continue;
        };
        if record.expires_at_ms <= current_time_ms() {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

pub(super) fn current_time_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(super) fn resolve_auth_input(value: &str) -> Result<String> {
    if value != "-" {
        return Ok(value.to_string());
    }

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read auth input from stdin")?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("No auth input was provided on stdin.");
    }
    Ok(trimmed.to_string())
}

pub(super) fn emit_scriptable_auth_prompt(
    provider: &str,
    auth_url: &str,
    input_kind: &str,
    pending_path: &Path,
    user_code: Option<&str>,
    expires_at_ms: i64,
    json: bool,
) -> Result<()> {
    let resume_command = scriptable_resume_command(provider, input_kind);
    let prompt = ScriptableAuthPrompt {
        status: "pending",
        provider: provider.to_string(),
        auth_url: auth_url.to_string(),
        input_kind: input_kind.to_string(),
        pending_path: pending_path.display().to_string(),
        user_code: user_code.map(str::to_string),
        expires_at_ms,
        resume_command: resume_command.clone(),
    };
    if json {
        println!("{}", serde_json::to_string(&prompt)?);
    } else {
        println!("{}", auth_url);
        if let Some(user_code) = user_code {
            eprintln!("User code: {}", user_code);
        }
        eprintln!("Auth URL printed to stdout.");
        eprintln!("Complete this login later with `{}`.", resume_command);
        eprintln!(
            "This pending login expires at {} ms since epoch.",
            expires_at_ms
        );
        eprintln!("Pending login state saved at {}", pending_path.display());
    }
    Ok(())
}

pub(super) fn scriptable_resume_command(provider: &str, input_kind: &str) -> String {
    match input_kind {
        "callback_url" => {
            format!(
                "jcode login --provider {} --callback-url '<url-or-query>'",
                provider
            )
        }
        "auth_code" => format!("jcode login --provider {} --auth-code '<code>'", provider),
        "complete" => format!("jcode login --provider {} --complete", provider),
        _ => format!(
            "jcode login --provider {} --callback-url '<url>'  # or --auth-code '<code>'",
            provider
        ),
    }
}

pub(super) fn emit_scriptable_auth_success(
    json: bool,
    success: ScriptableAuthSuccess,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(&success)?);
    }
    Ok(())
}
