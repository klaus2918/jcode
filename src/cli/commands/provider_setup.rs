use anyhow::{Context, Result};
use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

use crate::cli::args::ProviderAuthArg;
use crate::config::{
    Config, NamedProviderAuth, NamedProviderConfig, NamedProviderModelConfig, NamedProviderType,
    ProviderApiFormat,
};
use crate::provider_catalog::{
    api_base_uses_localhost, is_safe_env_file_name, is_safe_env_key_name,
    normalize_api_base_relaxed, resolve_login_provider, save_env_value_to_env_file,
};

#[derive(Debug)]
pub(crate) struct ProviderAddOptions {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub context_window: Option<usize>,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub api_key_stdin: bool,
    pub no_api_key: bool,
    pub auth: Option<ProviderAuthArg>,
    pub auth_header: Option<String>,
    pub env_file: Option<String>,
    pub set_default: bool,
    pub overwrite: bool,
    pub provider_routing: bool,
    pub model_catalog: bool,
    pub api: Option<crate::cli::args::ProviderApiFormatArg>,
    pub proxy: Option<String>,
    pub json: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderSetupReport {
    status: &'static str,
    profile: String,
    config_path: String,
    api_base: String,
    model: String,
    api_key_env: Option<String>,
    env_file: Option<String>,
    env_file_path: Option<String>,
    api_key_stored: bool,
    auth: String,
    default_set: bool,
    run_command: String,
    auth_test_command: String,
}

pub(crate) fn run_provider_add_command(options: ProviderAddOptions) -> Result<()> {
    let emit_json = options.json;
    let report = configure_provider_profile(options)?;

    if emit_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Added provider profile '{}'", report.profile);
        println!("  config: {}", report.config_path);
        println!("  base:   {}", report.api_base);
        println!("  model:  {}", report.model);
        println!("  auth:   {}", report.auth);
        if let Some(env_file_path) = &report.env_file_path {
            if report.api_key_stored {
                println!(
                    "  key:    {} in {}",
                    report.api_key_env.as_deref().unwrap_or("API key"),
                    env_file_path
                );
            } else {
                println!(
                    "  key:    {} (also reads {})",
                    report.api_key_env.as_deref().unwrap_or("API key"),
                    env_file_path
                );
            }
        } else if let Some(api_key_env) = &report.api_key_env {
            println!("  key:    environment variable {}", api_key_env);
        }
        if report.default_set {
            println!("  default: yes");
        }
        println!();
        println!("Run:       {}", report.run_command);
        println!("Validate:  {}", report.auth_test_command);
    }

    Ok(())
}

pub(crate) fn configure_provider_profile(
    options: ProviderAddOptions,
) -> Result<ProviderSetupReport> {
    let name = validate_profile_name(&options.name)?;
    ensure_profile_name_not_reserved(&name)?;

    let api_base = normalize_api_base_relaxed(&options.base_url).ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid --base-url '{}'. Use https://... or http://... (http is allowed for explicit gateway endpoints).",
            options.base_url
        )
    })?;
    let model = options.model.trim().to_string();
    if model.is_empty() {
        anyhow::bail!("--model cannot be empty");
    }
    if matches!(options.context_window, Some(0)) {
        anyhow::bail!("--context-window must be greater than 0");
    }

    let api_key = read_api_key(&options)?;
    let auth = resolve_auth_mode(&options, api_key.as_deref(), &api_base)?;
    let uses_auth = !matches!(auth, NamedProviderAuth::None);

    if !uses_auth && options.auth_header.is_some() {
        anyhow::bail!("--auth-header can only be used with --auth api-key");
    }
    if !matches!(auth, NamedProviderAuth::Header) && options.auth_header.is_some() {
        anyhow::bail!("--auth-header requires --auth api-key");
    }

    let api_key_env = if uses_auth {
        Some(resolve_api_key_env(&name, options.api_key_env.as_deref())?)
    } else {
        None
    };

    let api_format = options.api.map(|format| match format {
        crate::cli::args::ProviderApiFormatArg::OpenaiCompatible => {
            ProviderApiFormat::OpenAiCompatible
        }
        crate::cli::args::ProviderApiFormatArg::Anthropic => ProviderApiFormat::Anthropic,
    });
    let proxy = options
        .proxy
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let env_file = if uses_auth && (api_key.is_some() || options.env_file.is_some()) {
        Some(resolve_env_file(&name, options.env_file.as_deref())?)
    } else {
        None
    };

    if uses_auth
        && api_key.is_none()
        && options.api_key_env.is_none()
        && options.env_file.is_none()
        && !api_base_uses_localhost(&api_base)
    {
        anyhow::bail!(
            "Remote provider '{}' needs an API key source. Use --api-key-env NAME, --api-key-stdin, --api-key VALUE, or --auth none if this endpoint truly needs no auth.",
            name
        );
    }

    if let (Some(key), Some(env_key), Some(file_name)) = (
        api_key.as_deref(),
        api_key_env.as_deref(),
        env_file.as_deref(),
    ) {
        save_env_value_to_env_file(env_key, file_name, Some(key))?;
    }
    let api_key_stored = api_key.is_some() && env_file.is_some();

    let profile = NamedProviderConfig {
        provider_type: NamedProviderType::OpenAiCompatible,
        base_url: api_base.clone(),
        api_format,
        proxy,
        auth: auth.clone(),
        auth_header: match auth {
            NamedProviderAuth::Header => options
                .auth_header
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            _ => None,
        },
        api_key_env: api_key_env.clone(),
        api_key: None,
        env_file: env_file.clone(),
        default_model: Some(model.clone()),
        requires_api_key: Some(uses_auth),
        provider_routing: options.provider_routing,
        model_catalog: options.model_catalog,
        allow_provider_pinning: options.provider_routing,
        models: vec![NamedProviderModelConfig {
            id: model.clone(),
            context_window: options.context_window,
            input: Vec::new(),
            vision: None,
            tools: None,
            reasoning_protocol: None,
            supported_efforts: None,
            default_effort: None,
            output_window: None,
            temperature_supported: None,
            fixed_sampling: None,
            output_limit_field: None,
        }],
        extra_body: None,
        supports_reasoning_effort: None,
        ..NamedProviderConfig::default()
    };

    let config_path = Config::path().ok_or_else(|| anyhow::anyhow!("No config path"))?;
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let existing = parse_config_or_default(&content).with_context(|| {
        format!(
            "failed to parse existing config at {}",
            config_path.display()
        )
    })?;
    if existing.providers.contains_key(&name) && !options.overwrite {
        anyhow::bail!(
            "Provider profile '{}' already exists. Re-run with --overwrite to replace it.",
            name
        );
    }

    let mut updated = if options.overwrite {
        remove_named_provider_sections(&content, &name)
    } else {
        content
    };
    if options.set_default {
        updated = upsert_provider_defaults(updated, &name, &model);
    }
    updated = append_profile_section(updated, &name, &profile);

    toml::from_str::<Config>(&updated).with_context(|| {
        format!(
            "generated provider config for '{}' was not valid TOML",
            name
        )
    })?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, updated)?;

    let env_file_path = env_file
        .as_deref()
        .map(|file| crate::storage::app_config_dir().map(|dir| dir.join(file)))
        .transpose()?
        .map(path_to_string);

    Ok(ProviderSetupReport {
        status: "ok",
        profile: name.clone(),
        config_path: path_to_string(config_path),
        api_base,
        model: model.clone(),
        api_key_env,
        env_file,
        env_file_path,
        api_key_stored,
        auth: auth_label(&auth).to_string(),
        default_set: options.set_default,
        run_command: format!(
            "jcode --provider-profile {} --model {} run 'hello'",
            shell_quote(&name),
            shell_quote(&model)
        ),
        auth_test_command: format!(
            "jcode --provider-profile {} auth-test --prompt {}",
            shell_quote(&name),
            shell_quote("Reply exactly JCODE_PROVIDER_SETUP_OK")
        ),
    })
}

fn parse_config_or_default(content: &str) -> Result<Config> {
    if content.trim().is_empty() {
        Ok(Config::default())
    } else {
        Ok(toml::from_str::<Config>(content)?)
    }
}

fn validate_profile_name(raw: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        anyhow::bail!("provider profile name cannot be empty");
    }
    if name.len() > 64 {
        anyhow::bail!("provider profile name must be at most 64 characters");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        anyhow::bail!("provider profile name cannot be empty");
    };
    if !first.is_ascii_alphanumeric() {
        anyhow::bail!("provider profile name must start with a letter or number");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        anyhow::bail!("provider profile name may only contain ASCII letters, numbers, '-' and '_'");
    }
    Ok(name.to_string())
}

fn ensure_profile_name_not_reserved(name: &str) -> Result<()> {
    const RESERVED_PROVIDER_NAMES: &[&str] = &[
        "auto",
        "claude-subprocess",
        "compat",
        "custom",
        "azure-openai",
        "aoai",
    ];
    if resolve_login_provider(name).is_some()
        || RESERVED_PROVIDER_NAMES
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        anyhow::bail!(
            "'{}' is a built-in provider id or alias. Choose a non-reserved profile name such as '{}-api'.",
            name,
            name
        );
    }
    Ok(())
}

fn read_api_key(options: &ProviderAddOptions) -> Result<Option<String>> {
    if options.api_key_stdin {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let key = input.trim().to_string();
        if key.is_empty() {
            anyhow::bail!("--api-key-stdin was set, but stdin was empty");
        }
        Ok(Some(key))
    } else {
        Ok(options
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(ToString::to_string))
    }
}

fn resolve_auth_mode(
    options: &ProviderAddOptions,
    api_key: Option<&str>,
    api_base: &str,
) -> Result<NamedProviderAuth> {
    if options.no_api_key {
        return Ok(NamedProviderAuth::None);
    }
    if matches!(options.auth, Some(ProviderAuthArg::None)) {
        if api_key.is_some() || options.api_key_env.is_some() || options.env_file.is_some() {
            anyhow::bail!("--auth none cannot be combined with API key options");
        }
        return Ok(NamedProviderAuth::None);
    }
    if options.auth.is_none()
        && api_key.is_none()
        && options.api_key_env.is_none()
        && options.env_file.is_none()
        && api_base_uses_localhost(api_base)
    {
        return Ok(NamedProviderAuth::None);
    }

    Ok(match options.auth.unwrap_or(ProviderAuthArg::Bearer) {
        ProviderAuthArg::Bearer => NamedProviderAuth::Bearer,
        ProviderAuthArg::ApiKey => NamedProviderAuth::Header,
        ProviderAuthArg::None => NamedProviderAuth::None,
    })
}

fn resolve_api_key_env(name: &str, configured: Option<&str>) -> Result<String> {
    let env_name = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| derived_api_key_env(name));
    if !is_safe_env_key_name(&env_name) {
        anyhow::bail!(
            "Invalid --api-key-env '{}'. Use uppercase letters, numbers, and underscores only.",
            env_name
        );
    }
    Ok(env_name)
}

fn resolve_env_file(name: &str, configured: Option<&str>) -> Result<String> {
    let file = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("provider-{}.env", name));
    if !is_safe_env_file_name(&file) {
        anyhow::bail!(
            "Invalid --env-file '{}'. Use a file name only, with letters, numbers, '.', '_' or '-'.",
            file
        );
    }
    Ok(file)
}

fn derived_api_key_env(name: &str) -> String {
    let suffix = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("JCODE_PROVIDER_{}_API_KEY", suffix)
}

fn append_profile_section(
    mut content: String,
    name: &str,
    profile: &NamedProviderConfig,
) -> String {
    if !content.trim().is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if !content.is_empty() && !content.ends_with("\n\n") {
        content.push('\n');
    }

    content.push_str(&format!("[providers.{name}]\n"));
    content.push_str("type = \"openai-compatible\"\n");
    content.push_str(&format!("base_url = {}\n", toml_quote(&profile.base_url)));
    if let Some(api_format) = profile.api_format {
        let wire = match api_format {
            ProviderApiFormat::OpenAiCompatible => "openai-compatible",
            ProviderApiFormat::Anthropic => "anthropic",
        };
        content.push_str(&format!("api = {}\n", toml_quote(wire)));
    }
    if let Some(proxy) = profile.proxy.as_deref() {
        content.push_str(&format!("proxy = {}\n", toml_quote(proxy)));
    }
    content.push_str(&format!(
        "auth = {}\n",
        toml_quote(auth_label(&profile.auth))
    ));
    if let Some(header) = profile.auth_header.as_deref() {
        content.push_str(&format!("auth_header = {}\n", toml_quote(header)));
    }
    if let Some(api_key_env) = profile.api_key_env.as_deref() {
        content.push_str(&format!("api_key_env = {}\n", toml_quote(api_key_env)));
    }
    if let Some(env_file) = profile.env_file.as_deref() {
        content.push_str(&format!("env_file = {}\n", toml_quote(env_file)));
    }
    if let Some(default_model) = profile.default_model.as_deref() {
        content.push_str(&format!("default_model = {}\n", toml_quote(default_model)));
    }
    if let Some(requires_api_key) = profile.requires_api_key {
        content.push_str(&format!("requires_api_key = {requires_api_key}\n"));
    }
    if profile.provider_routing {
        content.push_str("provider_routing = true\nallow_provider_pinning = true\n");
    }
    if profile.model_catalog {
        content.push_str("model_catalog = true\n");
    }

    for model in &profile.models {
        content.push_str(&format!("\n[[providers.{name}.models]]\n"));
        content.push_str(&format!("id = {}\n", toml_quote(&model.id)));
        if let Some(limit) = model.context_window {
            content.push_str(&format!("context_window = {limit}\n"));
        }
    }

    content
}

fn upsert_provider_defaults(content: String, profile: &str, model: &str) -> String {
    let mut lines = split_lines_lossy(&content);
    let provider_idx = lines.iter().position(|line| line.trim() == "[provider]");

    let Some(idx) = provider_idx else {
        let mut prefix = String::from("[provider]\n");
        prefix.push_str(&format!("default_provider = {}\n", toml_quote(profile)));
        prefix.push_str(&format!("default_model = {}\n\n", toml_quote(model)));
        if content.trim().is_empty() {
            return prefix;
        }
        return format!("{prefix}{}", content.trim_start_matches('\n'));
    };

    let end = lines
        .iter()
        .enumerate()
        .skip(idx + 1)
        .find(|(_, line)| is_toml_header(line))
        .map(|(line_idx, _)| line_idx)
        .unwrap_or(lines.len());

    upsert_key_in_range(
        &mut lines,
        idx + 1,
        end,
        "default_provider",
        &toml_quote(profile),
    );
    let end = lines
        .iter()
        .enumerate()
        .skip(idx + 1)
        .find(|(_, line)| is_toml_header(line))
        .map(|(line_idx, _)| line_idx)
        .unwrap_or(lines.len());
    upsert_key_in_range(
        &mut lines,
        idx + 1,
        end,
        "default_model",
        &toml_quote(model),
    );

    join_lines(lines)
}

fn upsert_key_in_range(lines: &mut Vec<String>, start: usize, end: usize, key: &str, value: &str) {
    for line in lines.iter_mut().take(end).skip(start) {
        if line_has_toml_key(line, key) {
            *line = format!("{key} = {value}");
            return;
        }
    }
    lines.insert(end, format!("{key} = {value}"));
}

fn remove_named_provider_sections(content: &str, name: &str) -> String {
    let lines = split_lines_lossy(content);
    let mut kept: Vec<String> = Vec::with_capacity(lines.len());
    // Buffer a top-level `[[providers]]` array block until its `name` is known
    // (resonix entries declare `name = "..."` inside the block). Nested
    // `[[providers.models]]` subtables belong to the same array element and stay
    // in the buffer.
    let mut providers_block: Vec<String> = Vec::new();
    let mut in_providers_block = false;

    for line in lines {
        let trimmed = line.trim();
        if is_toml_header(&line) {
            if trimmed == "[[providers]]" {
                // Start of a new resonix array entry: flush the previous block.
                if in_providers_block {
                    flush_providers_block(&mut kept, &providers_block, name);
                }
                in_providers_block = true;
                providers_block.clear();
                providers_block.push(line.clone());
                continue;
            }
            if trimmed.starts_with("[[providers.") {
                // Nested sub-table of the current `[[providers]]` element
                // (e.g. `[[providers.models]]`): still part of the block.
                if in_providers_block {
                    providers_block.push(line.clone());
                    continue;
                }
                // Legacy standalone `[[providers.<name>.models]]` table style.
                if !is_named_provider_header(&line, name) {
                    kept.push(line);
                }
                continue;
            }
            if in_providers_block {
                // Any other section header (e.g. `[providers.x]` table or a
                // top-level `[provider]` section) ends the resonix block.
                flush_providers_block(&mut kept, &providers_block, name);
                in_providers_block = false;
            }
        }

        if in_providers_block {
            providers_block.push(line.clone());
        } else if !is_named_provider_header(&line, name) {
            kept.push(line);
        }
    }
    if in_providers_block {
        flush_providers_block(&mut kept, &providers_block, name);
    }

    join_lines(kept)
}

/// Emit a buffered `[[providers]]` block unless it declares `name = "<target>"`.
fn flush_providers_block(kept: &mut Vec<String>, block: &[String], target: &str) {
    let block_name = block
        .iter()
        .find_map(|line| providers_block_name(line))
        .unwrap_or_default();
    if block_name == target {
        // Matching resonix entry: drop the whole block (including any nested
        // `[[providers.models]]` subtables).
        return;
    }
    kept.extend(block.iter().cloned());
}

/// The `name = "..."` value of a line inside a `[[providers]]` block.
fn providers_block_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("name")?;
    if !rest.trim_start().starts_with('=') {
        return None;
    }
    let value = rest.trim_start().trim_start_matches('=').trim();
    let unquoted = value.strip_prefix('"')?.strip_suffix('"')?;
    Some(unquoted.to_string())
}

fn is_named_provider_header(line: &str, name: &str) -> bool {
    let trimmed = line.trim();
    let inner = if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
        &trimmed[2..trimmed.len() - 2]
    } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        return false;
    };
    let inner = inner.trim();
    let plain = format!("providers.{name}");
    let double_quoted = format!("providers.{}", toml_quote(name));
    let single_quoted = format!("providers.'{name}'");
    inner == plain
        || inner == format!("{plain}.models")
        || inner == double_quoted
        || inner == format!("{double_quoted}.models")
        || inner == single_quoted
        || inner == format!("{single_quoted}.models")
}

fn is_toml_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('[') && trimmed.ends_with(']')
}

fn line_has_toml_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix(key) else {
        return false;
    };
    rest.trim_start().starts_with('=')
}

fn split_lines_lossy(content: &str) -> Vec<String> {
    content.lines().map(ToString::to_string).collect()
}

fn join_lines(lines: Vec<String>) -> String {
    let mut joined = lines.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    joined
}

fn auth_label(auth: &NamedProviderAuth) -> &'static str {
    match auth {
        NamedProviderAuth::Bearer => "bearer",
        NamedProviderAuth::Header => "header",
        NamedProviderAuth::None => "none",
    }
}

fn toml_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| {
        crate::logging::warn(&format!(
            "failed to quote provider config string with serde_json: {error}"
        ));
        format!("\"{}\"", value.escape_default())
    })
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn path_to_string(path: PathBuf) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            crate::env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            crate::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                crate::env::set_var(self.key, previous);
            } else {
                crate::env::remove_var(self.key);
            }
        }
    }

    fn base_options() -> ProviderAddOptions {
        ProviderAddOptions {
            name: "my-api".to_string(),
            base_url: "https://llm.example.com/v1".to_string(),
            model: "model-a".to_string(),
            context_window: Some(128_000),
            api_key_env: None,
            api_key: Some("secret-test-key".to_string()),
            api_key_stdin: false,
            no_api_key: false,
            auth: None,
            auth_header: None,
            env_file: None,
            set_default: true,
            overwrite: false,
            provider_routing: false,
            model_catalog: false,
            api: None,
            proxy: None,
            json: false,
        }
    }

    #[test]
    fn provider_add_writes_anthropic_format_and_proxy() {
        let _lock = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().expect("temp dir");
        let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
        let _key = EnvVarGuard::remove("JCODE_PROVIDER_ANTH_API_KEY");
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "").expect("write config");

        let mut options = base_options();
        options.name = "anth".to_string();
        options.api_key = Some("sk-anth".to_string());
        options.api = Some(crate::cli::args::ProviderApiFormatArg::Anthropic);
        options.proxy = Some("http://127.0.0.1:7890".to_string());

        configure_provider_profile(options).expect("configure provider");
        let config = std::fs::read_to_string(&config_path).expect("read config");
        assert!(config.contains("[providers.anth]"));
        assert!(config.contains("api = \"anthropic\""));
        assert!(config.contains("proxy = \"http://127.0.0.1:7890\""));

        let parsed: Config = toml::from_str(&config).expect("valid config");
        let profile = parsed.providers.get("anth").expect("profile");
        assert_eq!(
            profile.api_format,
            Some(crate::config::ProviderApiFormat::Anthropic)
        );
        assert_eq!(profile.proxy.as_deref(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn provider_add_writes_named_profile_env_file_and_default() {
        let _lock = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().expect("temp dir");
        let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
        let _key = EnvVarGuard::remove("JCODE_PROVIDER_MY_API_API_KEY");
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            "# keep this comment\n[provider]\nopenai_reasoning_effort = \"low\"\n",
        )
        .expect("write config");

        let report = configure_provider_profile(base_options()).expect("configure provider");

        assert_eq!(report.profile, "my-api");
        let config = std::fs::read_to_string(&config_path).expect("read config");
        assert!(config.contains("# keep this comment"));
        assert!(config.contains("default_provider = \"my-api\""));
        assert!(config.contains("default_model = \"model-a\""));
        assert!(config.contains("[providers.my-api]"));
        assert!(!config.contains("secret-test-key"));

        let parsed: Config = toml::from_str(&config).expect("valid config");
        assert_eq!(parsed.provider.default_provider.as_deref(), Some("my-api"));
        assert_eq!(parsed.provider.default_model.as_deref(), Some("model-a"));
        let profile = parsed.providers.get("my-api").expect("profile");
        assert_eq!(profile.base_url, "https://llm.example.com/v1");
        assert_eq!(profile.default_model.as_deref(), Some("model-a"));
        assert_eq!(
            profile.api_key_env.as_deref(),
            Some("JCODE_PROVIDER_MY_API_API_KEY")
        );
        assert_eq!(profile.env_file.as_deref(), Some("provider-my-api.env"));
        assert_eq!(profile.models[0].context_window, Some(128_000));

        // resonix 对齐：密钥写入统一 `<jcode home>/.env`。
        let env_file = temp.path().join(".env");
        let env_content = std::fs::read_to_string(env_file).expect("env file");
        assert!(env_content.contains("JCODE_PROVIDER_MY_API_API_KEY=secret-test-key"));
    }

    #[test]
    fn provider_add_rejects_remote_without_api_key_source() {
        let _lock = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().expect("temp dir");
        let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
        let mut options = base_options();
        options.api_key = None;
        options.set_default = false;

        let err = configure_provider_profile(options).expect_err("should require key source");
        assert!(err.to_string().contains("needs an API key source"));
    }

    #[test]
    fn provider_add_allows_localhost_without_api_key() {
        let _lock = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().expect("temp dir");
        let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
        let mut options = base_options();
        options.base_url = "http://localhost:8000/v1".to_string();
        options.api_key = None;
        options.set_default = false;

        configure_provider_profile(options).expect("localhost no-auth should work");
        let config = std::fs::read_to_string(temp.path().join("config.toml")).expect("config");
        assert!(config.contains("auth = \"none\""));
        assert!(config.contains("requires_api_key = false"));
    }

    #[test]
    fn provider_add_allows_public_http_gateway_base_url() {
        let _lock = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().expect("temp dir");
        let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
        let _key = EnvVarGuard::remove("JCODE_PROVIDER_GW_HTTP_API_KEY");
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "").expect("write config");

        let mut options = base_options();
        options.name = "gw-http".to_string();
        options.base_url = "http://gateway.example.com".to_string();
        options.api_key = Some("sk-http-gw".to_string());

        configure_provider_profile(options).expect("public http gateway should be accepted");
        let config = std::fs::read_to_string(&config_path).expect("read config");
        let parsed: Config = toml::from_str(&config).expect("valid config");
        let profile = parsed.providers.get("gw-http").expect("profile");
        assert_eq!(profile.base_url, "http://gateway.example.com");
    }

    #[test]
    fn remove_named_provider_sections_removes_resonix_array_entry() {
        // `--overwrite` against a provider that exists as a resonix-style
        // top-level `[[providers]]` entry must drop that block (including its
        // nested `[[providers.models]]` subtables) and keep unrelated entries.
        let content = r#"[[providers]]
name = "keep"
type = "openai-compatible"
kind = "openai"
base_url = "https://api.keep.com"
model = "m1"

[[providers]]
name = "testgw"
type = "openai-compatible"
base_url = "https://api.testgw.com"
model = "m1"
api_key_env = "TESTGW_KEY"

[[providers.models]]
id = "m1"
context_window = 1000

[display]
centered = true
"#;
        let result = remove_named_provider_sections(content, "testgw");
        assert!(
            result.contains("name = \"keep\""),
            "keep entry must survive"
        );
        assert!(!result.contains("testgw"), "target entry must be removed");
        assert!(!result.contains("api_key_env = \"TESTGW_KEY\""));
        assert!(
            result.contains("[display]"),
            "unrelated section must survive"
        );
        // Result must still parse as valid TOML with only `keep`.
        let parsed: Config = toml::from_str(&result).expect("valid config");
        assert!(parsed.providers.contains_key("keep"));
        assert!(!parsed.providers.contains_key("testgw"));
    }

    #[test]
    fn remove_named_provider_sections_keeps_unrelated_resonix_entries() {
        let content = r#"[[providers]]
name = "one"
type = "openai-compatible"
base_url = "https://api.one.com"

[[providers]]
name = "two"
type = "openai-compatible"
base_url = "https://api.two.com"
"#;
        let result = remove_named_provider_sections(content, "missing");
        let parsed: Config = toml::from_str(&result).expect("valid config");
        assert!(parsed.providers.contains_key("one"));
        assert!(parsed.providers.contains_key("two"));
    }
}
