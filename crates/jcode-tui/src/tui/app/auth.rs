#[path = "auth_account_commands.rs"]
mod auth_account_commands;
pub(crate) use self::auth_account_commands::{
    handle_subscription_command, save_openai_fast_setting_local,
};

use super::*;
use std::sync::Arc;

impl App {
    pub(super) fn show_jcode_subscription_status(&mut self) {
        let configured_key = crate::subscription_catalog::configured_api_key().is_some();
        let configured_base = crate::subscription_catalog::configured_api_base()
            .unwrap_or_else(|| crate::subscription_catalog::DEFAULT_JCODE_API_BASE.to_string());
        let runtime_mode = crate::subscription_catalog::is_runtime_mode_enabled();
        let cached_tier = crate::subscription_catalog::cached_tier();

        let mut message = String::from("Jcode Subscription Status\n\n");
        message.push_str(&format!(
            "  - Credentials: {}\n",
            if configured_key {
                "configured"
            } else {
                "not configured (/login jcode)"
            }
        ));
        message.push_str(&format!(
            "  - Router base: {}{}\n",
            configured_base,
            if crate::subscription_catalog::has_router_base() {
                ""
            } else {
                " (default)"
            }
        ));
        message.push_str(&format!(
            "  - Tier: {}\n",
            cached_tier
                .map(|tier| tier.display_name().to_string())
                .unwrap_or_else(|| "unknown (treated as Plus)".to_string())
        ));
        message.push_str(&format!(
            "  - Runtime mode: {}\n\n",
            if runtime_mode {
                "active for this session"
            } else {
                "inactive for this session"
            }
        ));

        message.push_str("Catalog\n\n");
        for model in crate::subscription_catalog::curated_models() {
            let default_suffix = if model.default_enabled {
                " (default)"
            } else {
                ""
            };
            let tier_suffix = if model.min_tier == crate::subscription_catalog::JcodeTier::Plus {
                String::new()
            } else {
                format!(" [{}]", model.min_tier.display_name())
            };
            message.push_str(&format!(
                "  - {} - {}{}{}\n      - {}\n      - {}\n",
                model.display_name,
                model.id,
                default_suffix,
                tier_suffix,
                crate::subscription_catalog::routing_policy_detail(model),
                model.note
            ));
        }

        message.push_str("\nTiers\n\n");
        for tier in crate::subscription_catalog::JcodeTier::ALL.iter().copied() {
            message.push_str(&format!(
                "  - {} - ${}/mo retail, about ${:.2} usable inference budget\n",
                tier.display_name(),
                tier.retail_price_usd(),
                tier.usable_budget_usd()
            ));
        }

        if configured_key {
            message.push_str("\nFetching account status...");
        } else {
            message.push_str("\nLog in with /login jcode to see account usage and tier.");
        }

        self.push_display_message(DisplayMessage::system(message));

        // With credentials present, fetch live account status (/v1/me) in the
        // background and surface it via a UiActivity card. Short timeout keeps
        // this responsive; offline failures degrade to a quiet log line.
        if configured_key {
            let session_id = self.session.id.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    match crate::subscription_api::fetch_subscription_me().await {
                        Ok(me) => {
                            let tier_label = me
                                .parsed_tier()
                                .map(|tier| tier.display_name().to_string())
                                .unwrap_or_else(|| me.tier.clone());
                            let resets = me
                                .usage
                                .resets_at
                                .as_deref()
                                .map(|at| format!(", resets {}", at))
                                .unwrap_or_default();
                            crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
                                crate::bus::UiActivity::background(
                                    Some(session_id),
                                    format!(
                                        "Jcode Subscription Account\n\n  - Email: {}\n  - Tier: {} ({})\n  - Usage: ${:.2} of ${:.2}{}",
                                        me.email,
                                        tier_label,
                                        me.status,
                                        me.usage.used_usd,
                                        me.usage.budget_usd,
                                        resets
                                    ),
                                    Some("Subscription: account status loaded"),
                                ),
                            ));
                        }
                        Err(error) => {
                            let message = if error
                                .downcast_ref::<crate::subscription_api::AccountApiError>()
                                == Some(&crate::subscription_api::AccountApiError::Unauthorized)
                            {
                                let _ = crate::subscription_catalog::clear_account_credentials();
                                "Jcode Account Status\n\nThe saved account key was revoked or expired. Local credentials were cleared. Use /account jcode login to sign in again."
                                    .to_string()
                            } else {
                                format!(
                                    "Jcode Account Status\n\nCould not load /v1/me: {}\n\nThe local credential was retained. Retry /account jcode status, open /account jcode manage, or use /account jcode logout.",
                                    error
                                )
                            };
                            crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
                                crate::bus::UiActivity::background(
                                    Some(session_id),
                                    message,
                                    Some("Jcode account status unavailable"),
                                ),
                            ));
                        }
                    }
                });
            }
        }
    }

    pub(super) fn onboarding_should_prefer_strongest_model(&self) -> bool {
        if !self.onboarding_flow_active() {
            return false;
        }

        let provider_config = &crate::config::config().provider;
        let has_explicit_default = provider_config
            .default_provider
            .as_deref()
            .is_some_and(|provider| !provider.trim().is_empty())
            || provider_config
                .default_model
                .as_deref()
                .is_some_and(|model| !model.trim().is_empty());
        let runtime_provider_explicit = std::env::var("JCODE_INITIAL_PROVIDER_EXPLICIT")
            .ok()
            .is_some_and(|value| {
                let value = value.trim().to_ascii_lowercase();
                matches!(value.as_str(), "1" | "true" | "yes" | "on")
            });

        !has_explicit_default && !runtime_provider_explicit
    }

    fn trigger_provider_auth_changed(
        &mut self,
        provider_hint: Option<&str>,
        prefer_strongest: bool,
        select_local_model: bool,
    ) {
        crate::logging::auth_event(
            "auth_changed_triggered",
            self.provider.name(),
            &[("surface", "tui")],
        );
        crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
            crate::bus::UiActivity::auth(
                Some(self.session.id.clone()),
                "",
                Some("Auth: refreshing model routes..."),
            ),
        ));
        // Remote mode forwards the auth change to the server immediately after
        // this handler returns. Refreshing the client-side provider as well used
        // to duplicate every catalog network request and could race the first
        // onboarding prompt with a second model switch.
        if self.is_remote {
            return;
        }
        let provider = Arc::clone(&self.provider);
        let provider_hint = provider_hint.map(str::to_string);
        let session_id = self.session.id.clone();
        let auto_selection_active = Arc::clone(&self.onboarding_auto_model_selection_active);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let activation = crate::auth::lifecycle::activate_auth_change(
                    &crate::auth::lifecycle::AuthActivationRequest::new(provider_hint, None),
                );
                provider.on_auth_changed();
                if select_local_model && activation.provider_id.is_some() {
                    let model_before_catalog_wait = provider.model();
                    // Hot initialization is synchronous, but provider catalogs can
                    // arrive shortly afterward. Retry briefly so a first-run import
                    // selects the strongest live route rather than validating the
                    // stale pre-import model.
                    for delay_ms in [0_u64, 150, 350, 750, 1_500] {
                        if delay_ms > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        let routes = provider.model_routes();
                        let selection = if prefer_strongest {
                            if !auto_selection_active.load(std::sync::atomic::Ordering::Acquire)
                                || provider.model() != model_before_catalog_wait
                            {
                                break;
                            }
                            let Some(route) =
                                crate::auth::lifecycle::globally_preferred_default_route(&routes)
                            else {
                                continue;
                            };
                            let exact_route = crate::provider::RouteSelection::from_model_route(&route);
                            let default_selection =
                                crate::provider::MultiProvider::default_model_selection_from_route(
                                    &route.model,
                                    &route.api_method,
                                    &route.provider,
                                );
                            Some((
                                route.model,
                                exact_route.routed_model_spec(),
                                default_selection.provider_key,
                                Some(exact_route),
                            ))
                        } else {
                            let current_model = provider.model();
                            crate::auth::lifecycle::provider_model_to_select_after_auth(
                                &activation,
                                Some(&current_model),
                                &routes,
                            )
                            .map(|model| {
                                let model_request =
                                    activation.model_switch_request(provider.name(), &model);
                                let provider_key = crate::provider::MultiProvider::session_provider_key_for_model_request(
                                    &model_request,
                                    provider.name(),
                                );
                                (model, model_request, provider_key, None)
                            })
                        };
                        let Some((model, model_request, provider_key, exact_route)) = selection else {
                            break;
                        };
                        if prefer_strongest
                            && (!auto_selection_active
                                .load(std::sync::atomic::Ordering::Acquire)
                                || provider.model() != model_before_catalog_wait)
                        {
                            break;
                        }
                        let applied = exact_route.as_ref().map_or_else(
                            || provider.set_model(&model_request),
                            |selection| provider.set_route_selection(selection),
                        );
                        if applied.is_ok() {
                            crate::bus::Bus::global().publish_models_updated();
                            crate::bus::Bus::global().publish(
                                crate::bus::BusEvent::ProviderModelActivated {
                                    session_id: session_id.clone(),
                                    model: model.clone(),
                                    provider_key,
                                    message: format!(
                                        "Login ready. Switched to the strongest available default model: {model}."
                                    ),
                                    open_picker: false,
                                },
                            );
                            break;
                        }
                    }
                }
                // Hot provider initialization is complete even if live catalog
                // prefetches are still running. Wake the picker now so it can use
                // the newly available routes instead of the pre-login snapshot.
                crate::bus::Bus::global().publish(crate::bus::BusEvent::AuthCatalogRefreshReady);
            });
        } else {
            let activation = crate::auth::lifecycle::activate_auth_change(
                &crate::auth::lifecycle::AuthActivationRequest::new(provider_hint, None),
            );
            provider.on_auth_changed();
            if select_local_model && activation.provider_id.is_some() {
                let routes = provider.model_routes();
                if prefer_strongest {
                    if let Some(route) =
                        crate::auth::lifecycle::globally_preferred_default_route(&routes)
                    {
                        let selection = crate::provider::RouteSelection::from_model_route(&route);
                        let model_request = selection.routed_model_spec();
                        if provider.set_route_selection(&selection).is_ok() {
                            self.finalize_model_switch(&model_request);
                        }
                    }
                } else {
                    let current_model = provider.model();
                    if let Some(model) = crate::auth::lifecycle::provider_model_to_select_after_auth(
                        &activation,
                        Some(&current_model),
                        &routes,
                    ) {
                        let model_request =
                            activation.model_switch_request(provider.name(), &model);
                        if provider.set_model(&model_request).is_ok() {
                            self.finalize_model_switch(&model_request);
                        }
                    }
                }
            }
            self.finish_auth_catalog_refresh();
        }
    }

    fn login_provider_is_azure(provider: &str) -> bool {
        let provider = provider.trim();
        provider.eq_ignore_ascii_case("azure")
            || provider.eq_ignore_ascii_case("azure-openai")
            || provider.eq_ignore_ascii_case("azure openai")
    }

    fn activate_azure_runtime_model_after_login(&mut self) {
        let activated_model = match crate::provider::activation::apply_azure_openai_runtime() {
            Ok(model) => model,
            Err(error) => {
                let message = error.to_string();
                crate::logging::auth_event(
                    "auth_changed_runtime_activation_failed",
                    "azure-openai",
                    &[("surface", "tui"), ("reason", message.as_str())],
                );
                self.trigger_provider_auth_changed(Some("azure-openai"), false, true);
                return;
            }
        };

        // Rebuild the OpenAI-compatible transport under the Azure runtime before
        // selecting the configured deployment. This is local-only state; it does
        // not send a prompt or resume an upstream conversation.
        self.provider.on_auth_changed();

        let Some(model) = activated_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            crate::bus::Bus::global().publish_models_updated();
            return;
        };

        let model_request = if self.provider.name().eq_ignore_ascii_case("openrouter") {
            model.to_string()
        } else {
            format!("openrouter:{}", model)
        };

        match self.provider.set_model(&model_request) {
            Ok(()) => {
                let active_model = self.finalize_model_switch(&model_request);
                crate::bus::Bus::global().publish_models_updated();
                crate::logging::auth_event(
                    "auth_changed_runtime_model_applied",
                    "azure-openai",
                    &[("surface", "tui"), ("provider_session", "reset")],
                );
                self.set_status_notice(format!("Login: Azure OpenAI ready ({})", active_model));
            }
            Err(error) => {
                let message = error.to_string();
                crate::logging::auth_event(
                    "auth_changed_runtime_model_failed",
                    "azure-openai",
                    &[("surface", "tui"), ("reason", message.as_str())],
                );
                crate::bus::Bus::global().publish_models_updated();
            }
        }
    }

    #[allow(dead_code)] // 仅被测试引用（登录流程已删，保留供测试使用）
    pub(super) fn start_openai_compatible_post_login_activation(
        &mut self,
        provider_id: String,
        provider_label: String,
    ) {
        crate::logging::event_info(
            "login_post_activation_started",
            vec![
                ("provider_id", provider_id.clone()),
                ("provider", provider_label.clone()),
                ("session_id", self.session.id.clone()),
            ],
        );
        crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
            crate::bus::UiActivity::catalog(
                Some(self.session.id.clone()),
                format!(
                    "{} Model Discovery Started\n\nSaved credentials are active. Jcode is fetching the live model catalog, will only switch to a model returned by that catalog, and will show what changed when discovery finishes.",
                    provider_label
                ),
                Some(format!("{}: fetching models...", provider_label)),
            ),
        ));
        self.set_status_notice(format!("{}: fetching models...", provider_label));
        self.invalidate_model_picker_cache();

        // Make the newly saved OpenAI-compatible credentials usable in this
        // session immediately. The normal LoginCompleted path also calls this,
        // but doing it here lets the refresh task see the hot-added provider
        // without requiring a restart or a second user action.
        let provider = Arc::clone(&self.provider);
        let session_id = self.session.id.clone();
        let before_routes = provider.model_routes();
        self.provider.on_auth_changed();

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let result = provider.refresh_model_catalog().await;
                match result {
                    Ok(_summary) => {
                        let routes = provider.model_routes();
                        let expected_api_method = format!("openai-compatible:{}", provider_id);
                        let route_matches_profile = |route: &crate::provider::ModelRoute| {
                            route.available
                                && crate::provider::is_listable_model_name(&route.model)
                                && (route.api_method.eq_ignore_ascii_case(&expected_api_method)
                                    || route.api_method.eq_ignore_ascii_case(&provider_id))
                        };
                        let before_provider_routes = before_routes
                            .into_iter()
                            .filter(route_matches_profile)
                            .collect::<Vec<_>>();
                        let provider_routes = routes
                            .iter()
                            .filter(|route| route_matches_profile(route))
                            .cloned()
                            .collect::<Vec<_>>();
                        let before_provider_models = before_provider_routes
                            .iter()
                            .map(|route| route.model.clone())
                            .collect::<Vec<_>>();
                        let after_provider_models = provider_routes
                            .iter()
                            .map(|route| route.model.clone())
                            .collect::<Vec<_>>();
                        let summary = crate::provider::summarize_model_catalog_refresh(
                            before_provider_models,
                            after_provider_models,
                            before_provider_routes,
                            provider_routes.clone(),
                        );
                        let selected = provider_routes
                            .iter()
                            .find(|route| {
                                route.available
                                    && route.api_method.eq_ignore_ascii_case(&expected_api_method)
                                    && crate::provider::is_listable_model_name(&route.model)
                            })
                            .or_else(|| {
                                provider_routes.iter().find(|route| {
                                    route.available
                                        && route.api_method.eq_ignore_ascii_case(&provider_id)
                                        && crate::provider::is_listable_model_name(&route.model)
                                })
                            })
                            .map(|route| route.model.clone());

                        if let Some(model) = selected {
                            let model_request = format!("{}:{}", provider_id, model);
                            crate::logging::event_info(
                                "login_post_activation_route_selected",
                                vec![
                                    ("provider_id", provider_id.clone()),
                                    ("model", model.clone()),
                                    ("provider_routes", provider_routes.len().to_string()),
                                    ("models_added", summary.models_added.to_string()),
                                    ("routes_added", summary.routes_added.to_string()),
                                ],
                            );
                            match provider.set_model(&model_request) {
                                Ok(()) => {
                                    let provider_key = crate::provider::MultiProvider::session_provider_key_for_model_request(
                                        &model_request,
                                        provider.name(),
                                    );
                                    crate::logging::event_info(
                                        "login_post_activation_model_applied",
                                        vec![
                                            ("provider_id", provider_id.clone()),
                                            ("model", model.clone()),
                                            (
                                                "session_provider",
                                                provider_key.clone().unwrap_or_default(),
                                            ),
                                        ],
                                    );
                                    crate::bus::Bus::global().publish_models_updated();
                                    crate::bus::Bus::global().publish(
                                        crate::bus::BusEvent::ProviderModelActivated {
                                            session_id,
                                            model: model.clone(),
                                            provider_key,
                                            message: format!(
                                                "{} is ready.\n\nFetched model catalog: +{} models, +{} routes, ~{} changed.{}\n\nSwitched to {}. Use /model if you want to choose a different accessible model.\n\nIf the model list ever looks stale, run /refresh-model-list.",
                                                provider_label,
                                                summary.models_added,
                                                summary.routes_added,
                                                summary.routes_changed,
                                                {
                                                    let mut details = String::new();
                                                    super::model_context::append_model_name_diff(&mut details, &summary);
                                                    if details.is_empty() { String::new() } else { format!("\n{}", details) }
                                                },
                                                model
                                            ),
                                            open_picker: false,
                                        },
                                    );
                                }
                                Err(error) => {
                                    crate::logging::event_error(
                                        "login_post_activation_model_failed",
                                        vec![
                                            ("provider_id", provider_id.clone()),
                                            ("model", model.clone()),
                                            ("error", error.to_string()),
                                        ],
                                    );
                                    crate::bus::Bus::global().publish(
                                        crate::bus::BusEvent::LoginCompleted(
                                            crate::bus::LoginCompleted {
                                                provider: provider_label,
                                                success: false,
                                                message: format!(
                                                    "Fetched models, but failed to switch to {}: {}\n\nYou can run /refresh-model-list to retry model discovery.",
                                                    model, error
                                                ),
                                            },
                                        ),
                                    );
                                }
                            }
                        } else {
                            crate::logging::event_warn(
                                "login_post_activation_no_route",
                                vec![
                                    ("provider_id", provider_id.clone()),
                                    ("provider_routes", provider_routes.len().to_string()),
                                ],
                            );
                            crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
                                crate::bus::UiActivity::catalog(
                                    Some(session_id),
                                    format!(
                                        "{} Model Discovery Still Updating\n\nSaved credentials are active, but this local refresh pass did not find a selectable {} route yet. Jcode is still processing the auth-change catalog refresh and will switch once provider routes are available. If the model list still looks stale after the auth catalog update, run /refresh-model-list.",
                                        provider_label, provider_label
                                    ),
                                    Some(format!(
                                        "{}: waiting for model routes...",
                                        provider_label
                                    )),
                                ),
                            ));
                        }
                    }
                    Err(error) => {
                        crate::logging::event_error(
                            "login_post_activation_refresh_failed",
                            vec![
                                ("provider_id", provider_id.clone()),
                                ("error", error.to_string()),
                            ],
                        );
                        crate::bus::Bus::global().publish(crate::bus::BusEvent::UiActivity(
                            crate::bus::UiActivity::catalog(
                                Some(session_id),
                                format!(
                                    "{} Model Discovery Still Updating\n\nSaved credentials are active, but this local refresh pass failed before the server auth-change catalog refresh finished. Jcode is still processing the auth-change catalog refresh and will switch once provider routes are available. If the model list still looks stale after the auth catalog update, run /refresh-model-list.\n\nLocal refresh error: {}",
                                    provider_label, error
                                ),
                                Some(format!(
                                    "{}: waiting for model routes...",
                                    provider_label
                                )),
                            ),
                        ));
                    }
                }
            });
        }
    }

    pub(super) fn handle_login_completed(&mut self, login: LoginCompleted) {
        if login.provider == "copilot_code" {
            self.push_display_message(DisplayMessage::system(login.message.clone()));
            if let Some(code) = login
                .message
                .split("Your code: ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
            {
                self.set_status_notice(format!("Login: enter {} at GitHub", code));
            }
            return;
        }
        crate::auth::AuthStatus::invalidate_cache();
        crate::logging::event_info(
            "login_completed",
            vec![
                ("provider", login.provider.clone()),
                ("success", login.success.to_string()),
            ],
        );
        if login.success {
            self.recent_authenticated_provider = Some((login.provider.clone(), Instant::now()));
            // A fresh login is exactly what the credential-failure breaker is
            // waiting for: give automatic retries a fresh budget.
            self.reset_credential_failure_breaker();
            self.auth_catalog_refresh_pending = true;
            self.invalidate_model_picker_cache();
            let suppress_first_run_login_noise =
                self.onboarding_flow_active() && !matches!(login.provider.as_str(), "copilot_code");
            if !suppress_first_run_login_noise {
                self.push_display_message(DisplayMessage::system(login.message));
            }
            self.set_status_notice(format!("Login: {} ready", login.provider));
            if Self::login_provider_is_azure(&login.provider) {
                self.activate_azure_runtime_model_after_login();
            } else {
                let prefer_strongest = self.onboarding_should_prefer_strongest_model();
                if prefer_strongest {
                    self.onboarding_auto_model_selection_active
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                // Direct OpenAI-compatible logins already launched the
                // profile-specific catalog refresh and model activation before
                // publishing LoginCompleted. The generic auth refresh still
                // needs to rebuild routes and release the picker loading state,
                // but must not race it with a second model selection.
                let profile_activation_owns_selection =
                    crate::provider_catalog::resolve_openai_compatible_profile_selection(
                        &login.provider,
                    )
                    .is_some();
                self.trigger_provider_auth_changed(
                    Some(&login.provider),
                    prefer_strongest,
                    !profile_activation_owns_selection,
                );
            }
            // First-run onboarding: once the user has authenticated on a fresh
            // install, walk them through model selection -> continue/suggestions.
            self.maybe_begin_onboarding_flow_after_login();
        } else {
            let message = crate::auth::login_diagnostics::augment_auth_error_message(
                &login.provider,
                &login.message,
            );
            // During onboarding we route the failure to the recovery screen
            // (which explains next steps) instead of dumping a raw error message
            // and a status notice the user can miss.
            if self.onboarding_flow_active() {
                self.onboarding_handle_login_failed(Some(message));
            } else {
                self.push_display_message(DisplayMessage::error(message));
                self.set_status_notice(format!("Login: {} failed", login.provider));
                self.onboarding_handle_login_failed(None);
            }
        }
    }

            }
#[cfg(test)]
fn save_tui_openai_compatible_api_base(
    api_base: &str,
) -> anyhow::Result<crate::provider_catalog::ResolvedOpenAiCompatibleProfile> {
    let trimmed = api_base.trim();
    if !trimmed.is_empty() {
        let normalized = crate::provider_catalog::normalize_api_base(trimmed).ok_or_else(|| {
            anyhow::anyhow!("OpenAI-compatible API base must be https://... or http://localhost.")
        })?;
        crate::provider_catalog::save_env_value_to_env_file(
            "JCODE_OPENAI_COMPAT_API_BASE",
            crate::provider_catalog::OPENAI_COMPAT_PROFILE.env_file,
            Some(&normalized),
        )?;
    }
    Ok(crate::provider_catalog::resolve_openai_compatible_profile(
        crate::provider_catalog::OPENAI_COMPAT_PROFILE,
    ))
}

#[cfg(test)]
fn save_tui_openai_compatible_key(
    profile: crate::provider_catalog::OpenAiCompatibleProfile,
    key: &str,
) -> anyhow::Result<crate::provider_catalog::ResolvedOpenAiCompatibleProfile> {
    let resolved = crate::provider_catalog::resolve_openai_compatible_profile(profile);
    if resolved.requires_api_key {
        crate::provider_catalog::save_env_value_to_env_file(
            crate::provider_catalog::OPENAI_COMPAT_LOCAL_ENABLED_ENV,
            &resolved.env_file,
            None,
        )?;
        crate::provider_catalog::save_env_value_to_env_file(
            &resolved.api_key_env,
            &resolved.env_file,
            Some(key.trim()),
        )?;
    } else {
        crate::provider_catalog::save_env_value_to_env_file(
            crate::provider_catalog::OPENAI_COMPAT_LOCAL_ENABLED_ENV,
            &resolved.env_file,
            Some("1"),
        )?;
        crate::provider_catalog::save_env_value_to_env_file(
            &resolved.api_key_env,
            &resolved.env_file,
            if key.trim().is_empty() {
                None
            } else {
                Some(key.trim())
            },
        )?;
    }
    Ok(resolved)
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;


