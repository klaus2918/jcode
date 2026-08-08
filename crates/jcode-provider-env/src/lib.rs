use std::sync::{LazyLock, RwLock};

use jcode_provider_metadata::{is_safe_env_file_name, is_safe_env_key_name};

/// Fallback resolvers consulted by [`load_api_key_from_env_or_config`] after the
/// environment and config-file lookups fail. Higher-level crates register
/// resolvers at startup so this leaf crate does not need to depend on auth.
type ApiKeyFallbackResolver = fn(&str) -> Option<String>;

static API_KEY_FALLBACK_RESOLVERS: LazyLock<RwLock<Vec<ApiKeyFallbackResolver>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

/// Register a fallback API-key resolver consulted when env/config lookups miss.
pub fn register_api_key_fallback_resolver(resolver: ApiKeyFallbackResolver) {
    API_KEY_FALLBACK_RESOLVERS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(resolver);
}

fn resolve_api_key_fallback(env_key: &str) -> Option<String> {
    let resolvers = API_KEY_FALLBACK_RESOLVERS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for resolver in resolvers.iter() {
        if let Some(key) = resolver(env_key) {
            return Some(key);
        }
    }
    None
}

/// Characters that editors, terminals, and `cat` render invisibly but that
/// corrupt a credential when embedded in it. Rust's [`str::trim`] only removes
/// ASCII whitespace, so these survive a plain trim and silently break auth
/// (see GitHub issue #376). [`char::is_whitespace`] covers Unicode White_Space
/// (NBSP U+00A0, the en/em spaces U+2002-U+200A, line/paragraph separators,
/// etc.); the explicit cases below are zero-width characters and the BOM, which
/// are not classified as whitespace.
fn is_invisible_boundary_char(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\u{200B}' // zero-width space
                | '\u{200C}' // zero-width non-joiner
                | '\u{200D}' // zero-width joiner
                | '\u{2060}' // word joiner
                | '\u{FEFF}' // BOM / zero-width no-break space
        )
}

/// Strip leading/trailing invisible (Unicode whitespace and zero-width)
/// characters and one optional layer of surrounding quotes from a loaded
/// secret or config value.
///
/// Exposed so other credential loaders (e.g. the Cursor key reader) can apply
/// the same sanitizing as [`load_api_key_from_env_or_config`].
pub fn sanitize_secret_value(raw: &str) -> &str {
    raw.trim_matches(is_invisible_boundary_char)
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches(is_invisible_boundary_char)
}

/// Sanitize a loaded value and surface a warning when Unicode invisible
/// characters were present, so the failure mode in issue #376 is no longer
/// silent. Returns `None` for values that are empty after sanitizing.
fn clean_loaded_value(raw: &str, env_key: &str) -> Option<String> {
    let cleaned = sanitize_secret_value(raw);
    if cleaned.is_empty() {
        return None;
    }
    // A plain ASCII trim is what we previously did; if it leaves a different
    // result than the Unicode-aware sanitize, hidden characters were stripped.
    let ascii_only = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if ascii_only != cleaned {
        jcode_logging::warn(&format!(
            "Stripped Unicode invisible or non-ASCII whitespace characters from '{}' while loading credentials; verify the value contains no hidden characters",
            env_key
        ));
    }
    Some(cleaned.to_string())
}

pub fn load_api_key_from_env_or_config(env_key: &str, file_name: &str) -> Option<String> {
    if !is_safe_env_key_name(env_key) {
        jcode_logging::warn(&format!(
            "Ignoring invalid API key variable name '{}' while loading credentials",
            env_key
        ));
        return None;
    }
    if !is_safe_env_file_name(file_name) {
        jcode_logging::warn(&format!(
            "Ignoring invalid env file name '{}' while loading credentials",
            file_name
        ));
        return None;
    }

    // resonix 对齐：先查统一 `<jcode home>/.env`，再回退旧的分散文件
    // （openai-compatible.env / ollama.env 等），保证迁移期向后兼容。
    // 统一 .env 是密钥的权威来源：用户直接编辑 .env 替换 key 后立即生效，
    // 不会被继承的旧进程环境变量（jcode 保存 key 时会 set_var）覆盖。
    if let Some(key) = load_from_unified_env_file(env_key) {
        return Some(key);
    }

    if let Ok(key) = std::env::var(env_key)
        && let Some(key) = clean_loaded_value(&key, env_key)
    {
        return Some(key);
    }

    // 旧分散文件可能不存在（凭证已迁到统一 .env）：缺失时不提前返回，
    // 继续后面的 ZHIPU 特判与 fallback 解析。
    let config_path = jcode_storage::app_config_dir().ok()?.join(file_name);
    jcode_storage::harden_secret_file_permissions(&config_path);
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let prefix = format!("{}=", env_key);

    for line in content.lines() {
        if let Some(key) = line.strip_prefix(&prefix)
            && let Some(key) = clean_loaded_value(key, env_key)
        {
            return Some(key);
        }
    }

    if env_key == "ZHIPU_API_KEY" {
        if let Ok(key) = std::env::var("ZAI_API_KEY")
            && let Some(key) = clean_loaded_value(&key, "ZAI_API_KEY")
        {
            return Some(key);
        }

        // 统一 .env 里的 ZAI_API_KEY（resonix 单文件）优先于进程 env。
        if let Some(key) = load_from_unified_env_file("ZAI_API_KEY") {
            return Some(key);
        }

        let legacy_prefix = "ZAI_API_KEY=";
        for line in content.lines() {
            if let Some(key) = line.strip_prefix(legacy_prefix)
                && let Some(key) = clean_loaded_value(key, "ZAI_API_KEY")
            {
                return Some(key);
            }
        }
    }

    if let Some(key) = resolve_api_key_fallback(env_key) {
        return Some(key);
    }

    None
}

pub fn load_env_value_from_env_or_config(env_key: &str, file_name: &str) -> Option<String> {
    if !is_safe_env_key_name(env_key) {
        jcode_logging::warn(&format!(
            "Ignoring invalid variable name '{}' while loading config value",
            env_key
        ));
        return None;
    }
    if !is_safe_env_file_name(file_name) {
        jcode_logging::warn(&format!(
            "Ignoring invalid env file name '{}' while loading config value",
            file_name
        ));
        return None;
    }

    // 统一 .env 优先于进程环境变量（resonix 语义：.env 是配置值/密钥的
    // 权威来源），旧分散文件作为最后回退。
    if let Some(value) = load_from_unified_env_file(env_key) {
        return Some(value);
    }

    if let Ok(value) = std::env::var(env_key)
        && let Some(value) = clean_loaded_value(&value, env_key)
    {
        return Some(value);
    }

    load_env_value_from_config_file(env_key, file_name)
}

/// Load a value only from the saved env file under the jcode config dir,
/// ignoring the process environment.
///
/// [`load_env_value_from_env_or_config`] prefers the process env var, which is
/// correct for ambient configuration but wrong right after an explicit
/// `/login`: a stale env var inherited by a long-lived server process would
/// silently win over the credential the user just saved (issue #453). This
/// reader lets the auth-change path resolve what the file actually contains.
pub fn load_env_value_from_config_file(env_key: &str, file_name: &str) -> Option<String> {
    if !is_safe_env_key_name(env_key) {
        jcode_logging::warn(&format!(
            "Ignoring invalid variable name '{}' while loading config value",
            env_key
        ));
        return None;
    }
    if !is_safe_env_file_name(file_name) {
        jcode_logging::warn(&format!(
            "Ignoring invalid env file name '{}' while loading config value",
            file_name
        ));
        return None;
    }

    // resonix 对齐：统一 `.env` 优先，旧分散文件回退。
    if let Some(value) = load_from_unified_env_file(env_key) {
        return Some(value);
    }

    let config_path = jcode_storage::app_config_dir().ok()?.join(file_name);
    jcode_storage::harden_secret_file_permissions(&config_path);
    let content = std::fs::read_to_string(config_path).ok()?;
    let prefix = format!("{}=", env_key);

    for line in content.lines() {
        if let Some(value) = line.strip_prefix(&prefix)
            && let Some(value) = clean_loaded_value(value, env_key)
        {
            return Some(value);
        }
    }

    None
}

/// 统一凭证文件路径（resonix 对齐）：`<jcode home>/.env`。
///
/// 所有 API key / 配置值现在集中写入这一个文件，取代旧的分散
/// `openai-compatible.env` / `ollama.env` / `lmstudio.env` 等文件。
/// 旧文件仍可读取（向后兼容），新写入一律进统一文件。
pub fn unified_env_file_path() -> Option<std::path::PathBuf> {
    jcode_storage::jcode_dir().ok().map(|dir| dir.join(".env"))
}

/// 读取统一 `.env` 中 `env_key` 的值（无进程 env 参与，供迁移/校验用）。
fn load_from_unified_env_file(env_key: &str) -> Option<String> {
    if !is_safe_env_key_name(env_key) {
        return None;
    }
    let path = unified_env_file_path()?;
    jcode_storage::harden_secret_file_permissions(&path);
    let content = std::fs::read_to_string(path).ok()?;
    let prefix = format!("{}=", env_key);
    for line in content.lines() {
        if let Some(value) = line.strip_prefix(&prefix)
            && let Some(value) = clean_loaded_value(value, env_key)
        {
            return Some(value);
        }
    }
    None
}

pub fn save_env_value_to_env_file(
    env_key: &str,
    file_name: &str,
    value: Option<&str>,
) -> anyhow::Result<()> {
    if !is_safe_env_key_name(env_key) {
        anyhow::bail!("Invalid variable name: {}", env_key);
    }
    if !is_safe_env_file_name(file_name) {
        anyhow::bail!("Invalid env file name: {}", file_name);
    }

    // resonix 对齐：新写入一律进统一 `<jcode home>/.env`。file_name 参数
    // 保留仅用于向后兼容的读取路径（旧分散文件回退）。
    let file_path = unified_env_file_path()
        .ok_or_else(|| anyhow::anyhow!("No jcode home directory for unified .env"))?;
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    jcode_storage::upsert_env_file_value(&file_path, env_key, value)?;

    if let Some(value) = value {
        jcode_core::env::set_var(env_key, value);
    } else {
        jcode_core::env::remove_var(env_key);
    }

    Ok(())
}

/// 把旧的分散 env 文件（`openai-compatible.env` / `ollama.env` / `lmstudio.env`
/// 等）合并进统一 `<jcode home>/.env`，然后删除分散文件。
///
/// resonix 对齐：凭证只存一个文件。迁移是幂等的：统一文件已存在的 key
/// 以统一文件为准（旧文件不覆盖），避免覆盖用户在新文件中改过的值。
pub fn maybe_migrate_legacy_env_files() -> anyhow::Result<()> {
    let config_dir = jcode_storage::app_config_dir()?;
    let unified = match unified_env_file_path() {
        Some(path) => path,
        None => return Ok(()),
    };

    // 收集旧分散文件中的全部 key=value，跳过统一文件本身（不在同一目录）。
    let mut merged: Vec<(String, String)> = Vec::new();
    let mut migrated_any = false;
    for entry in std::fs::read_dir(&config_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if !name.ends_with(".env") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim();
                let value = line[eq + 1..].trim();
                if is_safe_env_key_name(key) && !value.is_empty() {
                    merged.push((key.to_string(), value.to_string()));
                }
            }
        }
        // 迁移后删除分散文件（保留一个 .env 本身，防误删）。
        std::fs::remove_file(&path)?;
        migrated_any = true;
        jcode_logging::info(&format!(
            "Migrated legacy env file {} into unified ~/.jcode/.env",
            name
        ));
    }

    if !migrated_any {
        return Ok(());
    }

    if let Some(parent) = unified.parent() {
        std::fs::create_dir_all(parent)?;
    }
    jcode_storage::harden_secret_file_permissions(&unified);
    // 合并：统一文件已存在的 key 保持统一文件的值。
    let mut existing: Vec<String> = Vec::new();
    if let Ok(content) = std::fs::read_to_string(&unified) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            existing.push(line.to_string());
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim();
                merged.retain(|(k, _)| k != key);
            }
        }
    }
    let mut all = existing;
    for (key, value) in merged {
        all.push(format!("{}={}", key, value));
    }
    let mut content = all.join("\n");
    content.push('\n');
    std::fs::write(&unified, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let saved = keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            for key in keys {
                jcode_core::env::remove_var(key);
            }
            Self { _lock: lock, saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                match value {
                    Some(value) => jcode_core::env::set_var(key, value),
                    None => jcode_core::env::remove_var(key),
                }
            }
        }
    }

    #[test]
    fn loads_api_key_from_unified_env_before_process_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME", "JCODE_PROVIDER_ENV_TEST_KEY"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        save_env_value_to_env_file(
            "JCODE_PROVIDER_ENV_TEST_KEY",
            "provider-env-test.env",
            Some("file-key"),
        )
        .expect("save file key");
        jcode_core::env::set_var("JCODE_PROVIDER_ENV_TEST_KEY", "env-key");

        // 统一 `.env` 是密钥权威来源：用户替换 .env 后立即生效，进程里的
        // 旧环境变量（jcode 保存 key 时会 set_var）不再覆盖文件值。
        assert_eq!(
            load_api_key_from_env_or_config("JCODE_PROVIDER_ENV_TEST_KEY", "provider-env-test.env")
                .as_deref(),
            Some("file-key")
        );
    }

    #[test]
    fn loads_and_removes_values_from_sandboxed_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME", "JCODE_PROVIDER_ENV_TEST_VALUE"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        save_env_value_to_env_file(
            "JCODE_PROVIDER_ENV_TEST_VALUE",
            "provider-env-test.env",
            Some("file-value"),
        )
        .expect("save file value");

        jcode_core::env::remove_var("JCODE_PROVIDER_ENV_TEST_VALUE");
        assert_eq!(
            load_env_value_from_env_or_config(
                "JCODE_PROVIDER_ENV_TEST_VALUE",
                "provider-env-test.env"
            )
            .as_deref(),
            Some("file-value")
        );

        save_env_value_to_env_file(
            "JCODE_PROVIDER_ENV_TEST_VALUE",
            "provider-env-test.env",
            None,
        )
        .expect("remove file value");
        assert_eq!(
            load_env_value_from_env_or_config(
                "JCODE_PROVIDER_ENV_TEST_VALUE",
                "provider-env-test.env"
            ),
            None
        );
    }

    #[test]
    fn accepts_legacy_zai_key_for_zhipu() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME", "ZHIPU_API_KEY", "ZAI_API_KEY"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        save_env_value_to_env_file("ZAI_API_KEY", "zai.env", Some("legacy-zai-key"))
            .expect("save legacy key");
        jcode_core::env::remove_var("ZAI_API_KEY");

        assert_eq!(
            load_api_key_from_env_or_config("ZHIPU_API_KEY", "zai.env").as_deref(),
            Some("legacy-zai-key")
        );
    }

    #[test]
    fn migrate_legacy_env_files_merges_and_deletes_legacy_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        // 旧分散文件
        let config_dir = jcode_storage::app_config_dir().expect("config dir");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("openai-compatible.env"),
            "OPENAI_COMPAT_API_KEY=legacy-key\n",
        )
        .expect("write legacy env");

        maybe_migrate_legacy_env_files().expect("migrate");

        // 统一 .env 里有旧值，分散文件已删除
        assert_eq!(
            load_api_key_from_env_or_config("OPENAI_COMPAT_API_KEY", "openai-compatible.env")
                .as_deref(),
            Some("legacy-key")
        );
        assert!(!config_dir.join("openai-compatible.env").exists());
        // 幂等：再跑一次不报错
        maybe_migrate_legacy_env_files().expect("second migrate");
    }

    #[test]
    fn migrate_keeps_unified_value_when_key_already_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        // 统一文件已有值
        save_env_value_to_env_file(
            "OPENAI_COMPAT_API_KEY",
            "openai-compatible.env",
            Some("new-key"),
        )
        .expect("save unified");

        // 旧文件也想写同一 key
        let config_dir = jcode_storage::app_config_dir().expect("config dir");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("openai-compatible.env"),
            "OPENAI_COMPAT_API_KEY=old-key\n",
        )
        .expect("write legacy env");

        maybe_migrate_legacy_env_files().expect("migrate");

        // 统一文件值优先（旧文件不覆盖）
        assert_eq!(
            load_api_key_from_env_or_config("OPENAI_COMPAT_API_KEY", "openai-compatible.env")
                .as_deref(),
            Some("new-key")
        );
    }
    #[test]
    fn sanitize_strips_unicode_invisible_characters() {
        // Zero-width space, BOM, NBSP, en space around the value.
        assert_eq!(
            sanitize_secret_value("\u{200B}sk-key123\u{FEFF}"),
            "sk-key123"
        );
        assert_eq!(sanitize_secret_value("\u{00A0}sk-key\u{2002}"), "sk-key");
        // Quotes plus invisible padding both stripped.
        assert_eq!(
            sanitize_secret_value("\u{FEFF}\"sk-quoted\"\u{200B}"),
            "sk-quoted"
        );
        // Interior characters are preserved.
        assert_eq!(
            sanitize_secret_value("sk-mid\u{200B}dle"),
            "sk-mid\u{200B}dle"
        );
        // Empty after sanitize.
        assert_eq!(sanitize_secret_value("\u{200B}\u{FEFF}"), "");
    }

    #[test]
    fn loads_api_key_with_zero_width_space_from_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME", "JCODE_PROVIDER_FOO_API_KEY"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());

        // Write an env file with a U+200B zero-width space prefixed onto the key,
        // mirroring issue #376's reproduction.
        let config_dir = jcode_storage::app_config_dir().expect("config dir");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("provider-foo.env"),
            "JCODE_PROVIDER_FOO_API_KEY=\u{200B}sk-mykey123\n",
        )
        .expect("write env file");

        assert_eq!(
            load_api_key_from_env_or_config("JCODE_PROVIDER_FOO_API_KEY", "provider-foo.env")
                .as_deref(),
            Some("sk-mykey123")
        );
    }

    #[test]
    fn loads_api_key_with_invisible_chars_from_env_var() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::new(&["JCODE_HOME", "JCODE_PROVIDER_BAR_API_KEY"]);
        jcode_core::env::set_var("JCODE_HOME", temp.path());
        // NBSP + BOM padding around the env-provided key.
        jcode_core::env::set_var("JCODE_PROVIDER_BAR_API_KEY", "\u{00A0}sk-env-key\u{FEFF}");

        assert_eq!(
            load_api_key_from_env_or_config("JCODE_PROVIDER_BAR_API_KEY", "provider-bar.env")
                .as_deref(),
            Some("sk-env-key")
        );
    }
}
