use super::*;

/// 处理 `/subscription` 与 `/subscribe` 两个订阅命令。
///
/// resonix 化后 `/login` `/logout` `/account` `/auth` 已被移除：模型接入只走
/// `[[providers]]` 配置 + 统一 `.env`，没有交互式登录/登出/账户管理可提供。
pub(crate) fn handle_subscription_command(app: &mut App, trimmed: &str) -> bool {
    if trimmed == "/subscription" || trimmed == "/subscription status" {
        app.show_jcode_subscription_status();
        return true;
    }

    if trimmed == "/subscribe" {
        app.show_subscribe_pitch();
        return true;
    }

    false
}

pub(crate) fn save_openai_fast_setting_local(app: &mut App, enabled: bool) {
    // Persist an explicit "off" instead of clearing the key. `None` serializes
    // by removing `openai_service_tier` from config.toml entirely, which made
    // "/fast default off" look like it never saved anything (issue #506). The
    // OpenAI runtime already treats "off" as disabling the tier.
    let value = if enabled { "priority" } else { "off" };
    match crate::config::Config::set_openai_service_tier(Some(value)) {
        Ok(()) => {
            let _ = app.provider.set_service_tier(value);
            let label = if enabled { "on" } else { "off" };
            app.set_status_notice(format!("Fast mode: {}", label));
            app.push_display_message(DisplayMessage::system(format!(
                "Saved OpenAI fast mode: {}.",
                label
            )));
        }
        Err(err) => app.push_display_message(DisplayMessage::error(format!(
            "Failed to save OpenAI fast mode: {}",
            err
        ))),
    }
}

