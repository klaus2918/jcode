//! Regression: the user's CC Switch + cch config uses named provider tables
//! (`[providers.<name>]`) with per-provider model arrays
//! (`[[providers.<name>.models]]`). Resonix-style top-level `[[providers]]`
//! arrays are not supported by this setup, so `Config::save()` must keep the
//! named-table style and must not drop per-model `api_key_env` / `auth`.

use jcode_base::config::{CompactionMode, Config, NamedProviderAuth, ProviderApiFormat};

const USER_CONFIG: &str = r#"
[provider]
default_provider = "cch"
default_model = "deepseek-v4-flash"

[providers.cc-switch]
type = "openai-compatible"
base_url = "http://127.0.0.1:15721"
api = "anthropic"
auth = "none"

[[providers.cc-switch.models]]
id = "deepseek-v4-flash"
context_window = 1000000
auth = "none"

[providers.cch]
type = "openai-compatible"
base_url    = "http://cch.skytech.io"
api = "anthropic"
auth = "header"
auth_header = "x-api-key"
api_key_env = "DEEPSEEK_API_KEY"
default_model = "deepseek-v4-flash"
fill_missing_reasoning = true

[[providers.cch.models]]
id = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "MiniMax-M3"
api_key_env = "MINIMAX_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "glm-5.2"
api_key_env = "GLM_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "kimi-k3"
api_key_env = "KIMI_API_KEY"
context_window = 1000000

[[providers.cch.models]]
id = "mimo-v2.5-pro"
api_key_env = "XIAOMI_MIMO_API_KEY"
context_window = 1000000

[compaction]
mode = "proactive"
threshold = 0.27
proactive_floor = 0.10
min_turns_between_compactions = 10
"#;

#[test]
fn user_cc_switch_and_cch_named_style_config_round_trips() {
    let config: Config = toml::from_str(USER_CONFIG).expect("user config must parse");
    assert_eq!(config.provider.default_provider.as_deref(), Some("cch"));
    assert_eq!(
        config.provider.default_model.as_deref(),
        Some("deepseek-v4-flash")
    );
    assert_eq!(config.compaction.mode, CompactionMode::Proactive);
    assert_eq!(config.compaction.threshold, 0.27);
    assert_eq!(config.compaction.proactive_floor, 0.10);
    assert_eq!(config.compaction.min_turns_between_compactions, 10);

    let cc = config
        .providers
        .get("cc-switch")
        .expect("cc-switch provider");
    assert_eq!(cc.base_url, "http://127.0.0.1:15721");
    assert_eq!(cc.api_format, Some(ProviderApiFormat::Anthropic));
    assert_eq!(cc.auth, NamedProviderAuth::None);
    assert_eq!(cc.models.len(), 1);
    assert_eq!(cc.models[0].id, "deepseek-v4-flash");
    assert_eq!(cc.models[0].context_window, Some(1000000));
    assert_eq!(cc.models[0].auth, Some(NamedProviderAuth::None));

    let cch = config.providers.get("cch").expect("cch provider");
    assert_eq!(cch.base_url, "http://cch.skytech.io");
    assert_eq!(cch.api_format, Some(ProviderApiFormat::Anthropic));
    assert_eq!(cch.auth, NamedProviderAuth::Header);
    assert_eq!(cch.auth_header.as_deref(), Some("x-api-key"));
    assert_eq!(cch.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
    assert_eq!(cch.default_model.as_deref(), Some("deepseek-v4-flash"));
    assert_eq!(cch.fill_missing_reasoning, Some(true));
    assert_eq!(cch.models.len(), 5);
    let minimax = cch
        .models
        .iter()
        .find(|m| m.id == "MiniMax-M3")
        .expect("MiniMax-M3 model");
    assert_eq!(minimax.api_key_env.as_deref(), Some("MINIMAX_API_KEY"));
    assert_eq!(minimax.context_window, Some(1000000));

    // Saving must keep the named-table style (never rewrite the user's file
    // into resonix-style top-level [[providers]] arrays), and per-model
    // api_key_env / auth must survive.
    let rendered = toml::to_string_pretty(&config).expect("serialize");
    println!("=== RENDERED ===\n{rendered}\n=== END RENDERED ===");
    assert!(
        rendered.contains("[providers.cc-switch]"),
        "save must keep named table style for cc-switch:\n{rendered}"
    );
    assert!(
        rendered.contains("[providers.cch]"),
        "save must keep named table style for cch:\n{rendered}"
    );
    assert!(
        !rendered.contains("[[providers]]\n"),
        "save must not emit resonix-style top-level [[providers]] arrays:\n{rendered}"
    );
    assert!(
        rendered.contains("[[providers.cch.models]]"),
        "save must keep per-provider model arrays:\n{rendered}"
    );
    assert!(
        rendered.contains("api_key_env = \"MINIMAX_API_KEY\""),
        "per-model api_key_env must survive round-trip:\n{rendered}"
    );

    let reparsed: Config = toml::from_str(&rendered).expect("rendered config must reparse");
    assert_eq!(reparsed.providers, config.providers);
    assert_eq!(
        reparsed.provider.default_provider,
        config.provider.default_provider
    );
    assert_eq!(
        reparsed.provider.default_model,
        config.provider.default_model
    );
    assert_eq!(reparsed.compaction.mode, config.compaction.mode);
    assert_eq!(reparsed.compaction.threshold, config.compaction.threshold);
}
