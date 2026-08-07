//! First-run onboarding flow state machine.
//!
//! After the user configures model access on a fresh install, we walk them
//! through a short guided flow:
//!
//!   1. `ConfigureProvider` - if we boot without working credentials, ask the
//!      user inside the TUI whether to configure a model provider now
//!      (the fresh install no longer runs a blocking CLI login, and
//!      external-login import walkthroughs are not part of live
//!      onboarding). Skipped entirely when credentials already exist.
//!   2. `StartChoice` - show two stacked actions: run a suggested Git-based bug
//!      and architecture review, or start a blank new session.
//!   3. `Suggestions` - the existing prompt-suggestion cards. Reached when
//!      they choose "Start a new session" or as the terminal resting state.
//!
//! Session history is intentionally excluded from onboarding and remains
//! available later through `/resume`.

use std::time::Instant;

/// The current phase of the onboarding flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OnboardingPhase {
    /// Ask the user whether to configure a model provider now. Shown on a
    /// fresh install when no working credentials exist. A highlightable
    /// Yes/No selector (default "Yes"): Yes points at `jcode provider add`
    /// (model access is config-driven), No exits onboarding to the normal
    /// new-session screen with a system message telling the user how to
    /// connect a provider when ready. There is no auto-timeout: connecting a
    /// provider is a meaningful first step, so we wait for the user rather
    /// than auto-selecting.
    ConfigureProvider {
        /// Which option is highlighted (true = "Yes, configure now").
        yes_highlighted: bool,
    },
    /// Legacy phase kept for compatibility with older replay/test fixtures.
    /// New onboarding skips explicit model selection and uses the default route;
    /// users can still run `/model` later.
    ModelSelect,
    /// Action-only picker offering the suggested review or a blank new session.
    StartChoice { shown_at: Instant },
    /// Existing prompt-suggestion cards (resting / "No" state).
    Suggestions,
    /// Flow finished; nothing onboarding-specific to render.
    Done,
}

/// A first-run new-session model-validation request that is waiting for a
/// concrete default-model id to be known before it fires. In remote/client
/// mode the live model is reported by the server asynchronously, so the
/// onboarding tick polls until a real id (not "unknown") is available, then
/// runs the lightweight validation ping.
///
/// When the validation is requested right after a login (remote mode), the
/// server also pushes a fresh model catalog a moment later (e.g. switching the
/// route to gpt-5.5 after an OpenAI login). We capture the catalog "generation"
/// at request time and wait for it to advance so the readiness line reports the
/// freshly-selected model rather than the stale pre-login default.
#[derive(Clone, Debug)]
pub(crate) struct OnboardingPendingValidation {
    /// Session the validation belongs to; stale requests are ignored.
    pub(crate) session_id: String,
    /// When the request was created, so we can give up after a short wait
    /// (and validate whatever default we have) rather than spinning forever.
    pub(crate) requested_at: Instant,
    /// Whether to wait for the server's post-login catalog refresh to land
    /// before firing (remote mode after a login).
    pub(crate) await_catalog_refresh: bool,
    /// Remote catalog generation observed when the request was created. The
    /// post-login refresh has landed once the live generation moves past this.
    pub(crate) catalog_generation_at_request: u64,
}

impl OnboardingPendingValidation {
    /// How long we will wait for the server to report a concrete model id
    /// before validating with the best default we currently have.
    const RESOLVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

    pub(crate) fn new(session_id: String) -> Self {
        Self {
            session_id,
            requested_at: Instant::now(),
            await_catalog_refresh: false,
            catalog_generation_at_request: 0,
        }
    }

    /// Variant that also waits for the remote catalog generation to advance
    /// past `catalog_generation` (the post-login refresh) before firing.
    pub(crate) fn awaiting_catalog_refresh(session_id: String, catalog_generation: u64) -> Self {
        Self {
            session_id,
            requested_at: Instant::now(),
            await_catalog_refresh: true,
            catalog_generation_at_request: catalog_generation,
        }
    }

    /// Whether we have waited long enough that we should validate now even if
    /// the model id has not been reported yet.
    pub(crate) fn resolve_timed_out(&self) -> bool {
        self.requested_at.elapsed() >= Self::RESOLVE_TIMEOUT
    }
}

/// Runtime state for the onboarding flow. `None`/`Done` means inactive.
#[derive(Clone, Debug)]
pub(crate) struct OnboardingFlow {
    pub(crate) phase: OnboardingPhase,
}

impl OnboardingFlow {
    /// Start the post-login flow. The app immediately advances this legacy
    /// phase to continue/suggestions so first-run onboarding no longer blocks on
    /// choosing a model.
    pub(crate) fn begin() -> Self {
        Self {
            phase: OnboardingPhase::ModelSelect,
        }
    }

    /// Start the flow at the "configure a model provider?" phase (no working
    /// credentials yet). Model access is config-driven, so the welcome screen
    /// asks a simple "Configure a model provider?" Yes/No and points at
    /// `jcode provider add`; no external-login import walkthrough is offered.
    pub(crate) fn begin_at_configure() -> Self {
        Self {
            phase: OnboardingPhase::ConfigureProvider {
                yes_highlighted: true,
            },
        }
    }

    /// Whether the flow is actively driving the UI.
    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.phase, OnboardingPhase::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_starts_at_model_select_and_is_active() {
        let flow = OnboardingFlow::begin();
        assert_eq!(flow.phase, OnboardingPhase::ModelSelect);
        assert!(flow.is_active());
    }

    #[test]
    fn done_phase_is_inactive() {
        let flow = OnboardingFlow {
            phase: OnboardingPhase::Done,
        };
        assert!(!flow.is_active());
    }

    #[test]
    fn begin_at_configure_offers_config_first() {
        let flow = OnboardingFlow::begin_at_configure();
        assert_eq!(
            flow.phase,
            OnboardingPhase::ConfigureProvider {
                yes_highlighted: true
            }
        );
        assert!(flow.is_active());
    }
}
