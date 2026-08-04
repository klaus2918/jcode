use super::*;

fn set_or_clear_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        crate::env::set_var(key, value);
    } else {
        crate::env::remove_var(key);
    }
}

#[test]
fn scriptable_resume_command_matches_input_kind() {
    assert_eq!(
        scriptable_resume_command("openai", "callback_url"),
        "jcode login --provider openai --callback-url '<url-or-query>'"
    );
    assert_eq!(
        scriptable_resume_command("gemini", "auth_code"),
        "jcode login --provider gemini --auth-code '<code>'"
    );
    assert_eq!(
        scriptable_resume_command("copilot", "complete"),
        "jcode login --provider copilot --complete"
    );
}

#[test]
fn load_pending_login_removes_expired_record() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::TempDir::new().expect("temp dir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let path = pending_login_path("openai").expect("pending path");
    let record = PendingScriptableLoginRecord {
        expires_at_ms: current_time_ms() - 1,
        login: PendingScriptableLogin::Openai {
            account_label: "default".to_string(),
            verifier: "verifier".to_string(),
            state: "state".to_string(),
            redirect_uri: "http://localhost:1455/auth/callback".to_string(),
        },
    };
    crate::storage::write_json_secret(&path, &record).expect("write pending login");

    let err = load_pending_login(&path, "openai").expect_err("expected expired state");
    assert!(err.to_string().contains("expired"));
    assert!(!path.exists(), "expired pending login should be removed");

    set_or_clear_env("JCODE_HOME", prev_home);
}

#[test]
fn uses_scriptable_flow_detects_dash_input_without_consuming_stdin() {
    let options = LoginOptions {
        callback_url: Some("-".to_string()),
        ..LoginOptions::default()
    };
    assert!(
        options
            .uses_scriptable_flow()
            .expect("uses scriptable flow")
    );
    assert!(options.has_provided_input());
}

