//! Control logic / phase transitions for the first-run onboarding flow.
//!
//! See [`super::onboarding_flow`] for the phase definitions. This module hangs
//! the driving methods off `App` so the rest of the TUI can advance the flow in
//! response to login, model selection, key presses, and the auto-advance timer.

use super::onboarding_flow::{OnboardingFlow, OnboardingPendingValidation, OnboardingPhase};
use super::{App, DisplayMessage, SessionPickerMode};
use crate::import::repo_ranking::{self, SessionLocation};
use crate::tui::session_picker::{SessionPicker, load_sessions};
use crossterm::event::KeyCode;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

impl App {
    /// Whether the guided onboarding flow is currently driving the UI.
    pub(super) fn onboarding_flow_active(&self) -> bool {
        self.onboarding_flow
            .as_ref()
            .map(OnboardingFlow::is_active)
            .unwrap_or(false)
    }

    /// The current onboarding phase, if the flow is active.
    pub(super) fn onboarding_phase(&self) -> Option<&OnboardingPhase> {
        self.onboarding_flow
            .as_ref()
            .filter(|flow| flow.is_active())
            .map(|flow| &flow.phase)
    }

    /// Gate + start the flow after a successful auth change (login/import or
    /// provider setup). Only fires for brand-new users (no prior onboarding
    /// flow this session) so returning users who re-auth aren't dragged through
    /// onboarding.
    pub(super) fn maybe_begin_onboarding_flow_after_auth(&mut self) {
        // If the flow is already running, a successful auth change means we
        // should leave the in-TUI configure phase and continue into model
        // selection.
        if self.onboarding_flow.is_some() {
            self.onboarding_advance_from_configure();
            return;
        }
        if !self.onboarding_preview_mode
            && (self.is_selfdev_canary_session() || !self.is_new_user_for_onboarding())
        {
            return;
        }
        self.begin_onboarding_flow();
    }

    /// One-shot startup check: the fresh-install path logs the user in at the CLI
    /// *before* the TUI launches, so no in-TUI login event ever fires. If we boot
    /// already authenticated as a brand-new user, kick the guided flow here.
    ///
    /// Returns without committing the one-shot guard until auth is actually
    /// resolved (the server may still be bootstrapping on the first ticks), so a
    /// momentary "not yet authenticated" reading doesn't permanently skip the
    /// flow. Once we either start the flow or conclude it shouldn't run, the
    /// guard is set and this becomes a no-op for the rest of the session.
    pub(super) fn maybe_begin_onboarding_flow_on_startup(&mut self) {
        if self.onboarding_startup_checked {
            return;
        }
        if self.onboarding_flow.is_some() {
            self.onboarding_startup_checked = true;
            return;
        }
        // Don't hijack a session that already has real activity (resume,
        // restored input, or a genuine conversation already on screen). These
        // are settled states, so we can commit the guard.
        //
        // A brand-new session still carries one synthetic `<system-reminder>`
        // "Session Context" message (role=user) plus assorted system scaffolding.
        // Those are not real activity, so we ignore them when deciding whether
        // the session is already in use.
        let has_real_conversation = self.display_messages.iter().any(|m| {
            let role = m.role.as_str();
            let is_system_reminder =
                role == "user" && m.content.trim_start().starts_with("<system-reminder>");
            let is_scaffolding = matches!(
                role,
                "system" | "usage" | "overnight" | "todos" | "background_task"
            );
            !is_system_reminder && !is_scaffolding
        });
        if has_real_conversation || self.is_processing || !self.input.is_empty() {
            self.onboarding_startup_checked = true;
            return;
        }
        // Self-dev / canary sessions are explicitly not first-run users: they are
        // spawned by developers (e.g. the niri `jcode self-dev` hotkey) and that
        // launch path never increments `launch_count`, so the new-user heuristic
        // would otherwise re-onboard on every spawn. Skip onboarding for them.
        if self.is_selfdev_canary_session() {
            self.onboarding_startup_checked = true;
            return;
        }
        if !self.is_new_user_for_onboarding() {
            self.onboarding_startup_checked = true;
            return;
        }
        // Fresh installs no longer log in at the CLI before the TUI launches.
        // If we boot without working credentials, start the flow at the in-TUI
        // "configure a model provider?" phase. If credentials already exist,
        // start the post-auth onboarding path directly; we no longer ask
        // first-run users to choose a model before they can get started.
        self.onboarding_startup_checked = true;
        if crate::auth::AuthStatus::check_fast().has_any_available() {
            self.begin_onboarding_flow();
        } else {
            self.begin_onboarding_flow_at_configure();
        }
    }

    /// Whether this install looks like a brand-new user.
    ///
    /// Looks for independent evidence of an established install: a meaningful
    /// number of persisted native sessions. A user with a long session
    /// history must never be dragged through first-run onboarding.
    fn is_new_user_for_onboarding(&self) -> bool {
        Self::is_new_user_install()
    }

    /// Shared "does this install look brand-new?" check (see
    /// [`Self::is_new_user_for_onboarding`] for the rationale). Also used by
    /// the welcome-screen suggestion prompts.
    pub(super) fn is_new_user_install() -> bool {
        let Ok(dir) = crate::storage::jcode_dir() else {
            return true;
        };
        !Self::has_established_native_session_history(&dir)
    }

    /// Independent "experienced user" evidence: enough persisted native
    /// sessions on disk. Imported transcripts (`imported_*.json`) don't count;
    /// they exist on fresh installs that imported Codex/Claude history.
    fn has_established_native_session_history(jcode_dir: &std::path::Path) -> bool {
        const ESTABLISHED_SESSION_THRESHOLD: usize = 10;
        let Ok(entries) = std::fs::read_dir(jcode_dir.join("sessions")) else {
            return false;
        };
        let mut native_sessions = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with("session_") && name.ends_with(".json") {
                native_sessions += 1;
                if native_sessions >= ESTABLISHED_SESSION_THRESHOLD {
                    return true;
                }
            }
        }
        false
    }

    /// Whether this is a self-dev / canary session.
    ///
    /// These are launched by developers working on jcode itself. They should
    /// never auto-start the guided onboarding flow.
    fn is_selfdev_canary_session(&self) -> bool {
        if self.is_remote {
            self.remote_is_canary.unwrap_or(self.session.is_canary)
        } else {
            self.session.is_canary
        }
    }

    /// Begin the guided post-login flow. Called once auth becomes available on a
    /// fresh install (login/import completes). New users are not forced through a
    /// model picker; the default route is used and `/model` remains available.
    ///
    /// No-op if a flow is already running or the user is experienced.
    pub(super) fn begin_onboarding_flow(&mut self) {
        if self.onboarding_flow.is_some() {
            return;
        }
        self.onboarding_flow = Some(OnboardingFlow::begin());
        self.onboarding_after_model_select();
    }

    /// Begin the guided flow at the "configure a model provider?" prompt. Used
    /// on a fresh install that booted without working credentials (the CLI no
    /// longer logs in before the TUI launches). Model access is config-driven:
    /// the welcome screen asks whether to configure a provider now and points at
    /// `jcode provider add`. No external-login (Anthropic/Codex/...) import
    /// walkthrough is offered.
    ///
    /// No-op if a flow is already running.
    pub(super) fn begin_onboarding_flow_at_configure(&mut self) {
        if self.onboarding_flow.is_some() {
            return;
        }
        self.onboarding_flow = Some(OnboardingFlow::begin_at_configure());
        // The login prompt is rendered by the onboarding welcome screen
        // (`onboarding_welcome_kind`) so it survives in remote mode.
        self.set_status_notice(
            "Configure a model provider now? Yes/No - hl to move, Enter to choose (No skips for now)",
        );
    }

    /// Start the default first-run provider setup.
    /// Config-driven (Reasonix-aligned) mode: models are connected via
    /// `[[providers]]` config entries + a unified `~/.jcode/.env`, so the
    /// guided flow points at `jcode provider add` instead of interactive login.
    pub(super) fn onboarding_start_provider_setup(&mut self) {
        self.push_display_message(DisplayMessage::system(
            "Model access is configured, not logged in. Run `jcode provider add <name> --base-url <url> --api-key-env <ENV_VAR>` to connect a provider, then pick it in /model."
                .to_string(),
        ));
        self.set_status_notice("Configure a provider: jcode provider add");
        self.onboarding_advance_from_configure();
    }

    /// Advance out of the "configure a model provider?" phase once provider
    /// setup is done. Advance straight to model selection.
    /// No-op unless the flow is in the configure phase.
    pub(super) fn onboarding_advance_from_configure(&mut self) {
        if !matches!(
            self.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider { .. })
        ) {
            return;
        }
        if let Some(flow) = self.onboarding_flow.as_mut() {
            flow.phase = OnboardingPhase::ModelSelect;
        }
        self.onboarding_after_model_select();
    }

    /// Advance out of model selection into the simple first-run choice: run the
    /// suggested Git-based bug review or start with a blank new session.
    pub(super) fn onboarding_after_model_select(&mut self) {
        if !matches!(self.onboarding_phase(), Some(OnboardingPhase::ModelSelect)) {
            return;
        }
        self.onboarding_open_start_choice();
    }

    /// Intercept keys for the guided onboarding welcome phases:
    /// - `ModelSelect`: we tell the user to run /model; Enter is also a
    ///   shortcut that opens the model picker from the welcome screen.
    /// - `ConfigureProvider`: Left/h -> Yes, Right/l -> No, toggle with
    ///   Up/Down/k/j/Tab; y/n commit directly, Enter/Space commit the
    ///   highlighted default (Yes -> configure a provider now, No -> finish
    ///   onboarding).
    ///
    /// Returns true if the key was consumed.
    pub(super) fn handle_onboarding_continue_prompt_key(&mut self, code: KeyCode) -> bool {
        // While a provider login is awaiting typed input (an API key, env var
        // value, endpoint, etc.) the onboarding flow is still parked in a
        // pre-ready phase, but the user is now typing into the pending prompt
        // rather than driving the welcome-screen Yes/No. If we kept intercepting
        // keys here, Enter would re-open the picker and characters like h/l/j/
        // k/y/n would be eaten as navigation. Let the normal input path handle
        // everything until the pending entry resolves.
        if self.pending_ssh_remote_name.is_some() {
            return false;
        }
        // Universal escape hatch. From any guided pre-ready phase, Esc always
        // leaves onboarding to the normal new-session screen. This is the last
        // line of the liveness guarantee: no matter what state the flow is in,
        // one key the user always has gets them out. We only handle it on the
        // welcome card itself; when an inline overlay (picker / sign-in) is
        // open we let Esc close that first.
        if code == KeyCode::Esc
            && self.inline_interactive_state.is_none()
            && self.session_picker_overlay.is_none()
            && matches!(
                self.onboarding_phase(),
                Some(OnboardingPhase::ConfigureProvider { .. } | OnboardingPhase::ModelSelect)
            )
        {
            self.onboarding_finish();
            let login = Self::onboarding_provider_add_hint();
            self.set_status_notice(format!(
                "Onboarding skipped - run {login} when you're ready"
            ));
            return true;
        }
        match self.onboarding_phase() {
            Some(OnboardingPhase::ConfigureProvider { .. }) => {
                // Don't intercept once an inline overlay (the provider picker)
                // is already open.
                if self.inline_interactive_state.is_some() {
                    return false;
                }
                self.handle_onboarding_configure_key(code)
            }
            Some(OnboardingPhase::ModelSelect) => match code {
                // Enter opens the model picker, but only from the welcome
                // screen. If a picker (or any inline overlay) is already open,
                // let it handle Enter so the selection can commit.
                KeyCode::Enter if self.inline_interactive_state.is_none() => {
                    self.open_model_picker();
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Handle a key while the "Configure a model provider?" prompt is up.
    /// Yes/No sit side by side (default highlight is "Yes"):
    ///   - Left / h  -> highlight "Yes"
    ///   - Right / l -> highlight "No"
    ///   - Up / Down / k / j / Tab -> toggle
    ///   - y / Y -> configure a provider now;  n / N -> skip and finish onboarding
    ///   - Enter / Space -> commit the highlighted choice
    fn handle_onboarding_configure_key(&mut self, code: KeyCode) -> bool {
        let Some(flow) = self.onboarding_flow.as_mut() else {
            return false;
        };
        let OnboardingPhase::ConfigureProvider { yes_highlighted } = &mut flow.phase else {
            return false;
        };
        match code {
            KeyCode::Left | KeyCode::Char('h') => {
                *yes_highlighted = true;
                self.update_onboarding_configure_status();
                true
            }
            KeyCode::Right | KeyCode::Char('l') => {
                *yes_highlighted = false;
                self.update_onboarding_configure_status();
                true
            }
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Char('k')
            | KeyCode::Char('j')
            | KeyCode::Tab => {
                *yes_highlighted = !*yes_highlighted;
                self.update_onboarding_configure_status();
                true
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.onboarding_answer_configure(true);
                true
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.onboarding_answer_configure(false);
                true
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let wants_openai = *yes_highlighted;
                self.onboarding_answer_configure(wants_openai);
                true
            }
            _ => false,
        }
    }

    /// Answer the "Configure a provider?" prompt. Yes starts the config-guided
    /// setup; No exits onboarding and drops the user on the normal new-session
    /// screen with a system message telling them how to connect a provider.
    pub(super) fn onboarding_answer_configure(&mut self, wants_openai: bool) {
        if !matches!(
            self.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider { .. })
        ) {
            return;
        }
        if wants_openai {
            self.onboarding_start_provider_setup();
        } else {
            self.onboarding_finish();
            self.push_display_message(DisplayMessage::system(
                "No problem. When you're ready to connect a model provider, run                  `jcode provider add <name> --base-url <url> --api-key-env <ENV_VAR>`                  and pick it in /model."
                    .to_string(),
            ));
            self.set_status_notice("Configure a provider when you're ready");
        }
    }

    /// Refresh the status notice for the "Configure a model provider?" prompt.
    fn update_onboarding_configure_status(&mut self) {
        let yes = matches!(
            self.onboarding_phase(),
            Some(OnboardingPhase::ConfigureProvider {
                yes_highlighted: true
            })
        );
        let choice = if yes { "Yes" } else { "No" };
        self.set_status_notice(format!(
            "Configure a model provider now? [{choice}] - hl to move, Enter to choose (No skips for now)"
        ));
    }

    /// Open the action-only onboarding choice. Session history remains available
    /// later through `/resume`, but first run stays focused on two clear paths.
    pub(super) fn onboarding_open_start_choice(&mut self) {
        let mut picker = SessionPicker::new(Vec::new());
        picker.activate_onboarding_banner(Self::onboarding_start_choice_banner_lines());
        self.session_picker_overlay = Some(RefCell::new(picker));
        self.session_picker_mode = SessionPickerMode::Onboarding;
        if let Some(flow) = self.onboarding_flow.as_mut() {
            flow.phase = OnboardingPhase::StartChoice {
                shown_at: Instant::now(),
            };
        }
        self.onboarding_prefetch_recent_project();
        self.set_status_notice("Choose a suggested review or start a new session (↑↓, Enter)");
    }

    /// Formatted copy shown above the two first-run actions.
    fn onboarding_start_choice_banner_lines() -> Vec<ratatui::text::Line<'static>> {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        let accent = crate::tui::color_support::rgb(186, 139, 255);
        vec![
            Line::from(vec![Span::styled(
                "Welcome to jcode 🎉",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "How would you like to begin?",
                Style::default().fg(Color::White),
            )]),
        ]
    }

    /// Warm the recent-project lookup while the user is still reading the start
    /// choice screen.
    ///
    /// Resolving the newest known Git repository requires a full session-list
    /// scan, which is a cold multi-hundred-millisecond disk walk on machines
    /// with a large `~/.jcode/sessions` directory. Doing it inline on Enter made
    /// the "Find bugs in what I've been working on" action feel laggy, so run it
    /// off-thread as soon as the choice is displayed and have the key handler
    /// consume the cached answer.
    fn onboarding_prefetch_recent_project(&mut self) {
        if self.is_remote || self.onboarding_recent_project_prefetch.is_some() {
            return;
        }
        let slot: Arc<Mutex<Option<Option<PathBuf>>>> = Arc::new(Mutex::new(None));
        self.onboarding_recent_project_prefetch = Some(slot.clone());
        let session_id = self.session.id.clone();
        // A plain OS thread (not `tokio::spawn`) keeps this blocking filesystem
        // scan off the async runtime and works in tests without a reactor.
        std::thread::spawn(move || {
            let resolved = Self::recent_project_path_from_sessions(&session_id);
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(resolved);
            }
        });
    }

    /// Resolve the project to review before the agent turn starts. The active
    /// session directory wins when it is already inside a Git repository;
    /// otherwise recent native and external session metadata supplies the newest
    /// known repository. This keeps repository discovery out of the model prompt.
    pub(super) fn onboarding_recent_project_path(&self) -> Option<PathBuf> {
        let home = dirs::home_dir();
        let excluded: Vec<PathBuf> = home.iter().cloned().collect();

        if let Some(working_dir) = self.session.working_dir.as_deref() {
            let working_dir = Path::new(working_dir);
            if self.is_remote {
                // The path belongs to the remote runtime and cannot generally be
                // statted by the attached client. Trust the server-provided
                // session directory, but never turn a home-directory launch into
                // a broad home review.
                if !working_dir.as_os_str().is_empty()
                    && home.as_deref().is_none_or(|home| home != working_dir)
                {
                    return Some(working_dir.to_path_buf());
                }
            } else if let Some(root) = repo_ranking::resolve_git_root(working_dir)
                && !excluded.iter().any(|excluded| excluded == &root)
            {
                return Some(root);
            }
        }

        if self.is_remote {
            return None;
        }

        // Prefer the prefetched answer so the action responds immediately.
        if let Some(prefetched) = self
            .onboarding_recent_project_prefetch
            .as_ref()
            .and_then(|slot| slot.lock().ok().and_then(|slot| slot.clone()))
        {
            return prefetched;
        }

        Self::recent_project_path_from_sessions(&self.session.id)
    }

    /// Newest known Git repository across recorded sessions, excluding the
    /// current session and the bare home directory. Blocking: this walks the
    /// session list on disk.
    fn recent_project_path_from_sessions(current_session_id: &str) -> Option<PathBuf> {
        let excluded: Vec<PathBuf> = dirs::home_dir().into_iter().collect();
        let sessions = load_sessions().unwrap_or_default();
        let locations: Vec<SessionLocation> = sessions
            .into_iter()
            .filter(|session| {
                session.id != current_session_id && !session.is_debug && !session.is_canary
            })
            .filter_map(|session| {
                let working_dir = session.working_dir?;
                Some(SessionLocation::new(
                    working_dir,
                    session.last_active_at.or(Some(session.last_message_time)),
                ))
            })
            .collect();
        repo_ranking::most_recent_repository(&locations, &excluded)
    }

    /// First-turn prompt launched by the onboarding recent-project review action.
    /// Repository discovery is deliberately absent: the path has already been
    /// selected programmatically before this prompt is built.
    pub(super) fn onboarding_recent_project_review_prompt(repository: &Path) -> String {
        let repository = format!("{:?}", repository.to_string_lossy());
        format!(
            "Find the most critical architecture problems in the repository at {repository}. Do not fix them yet, and ask me whether I want them fixed once you find them."
        )
    }

    pub(super) fn onboarding_prepare_recent_project_review(&mut self) -> bool {
        let Some(repository) = self.onboarding_recent_project_path() else {
            self.onboarding_show_suggestions();
            self.set_status_notice(
                "No recent Git repository found. Start jcode inside a project to review it.",
            );
            return false;
        };
        self.onboarding_finish();
        self.input = Self::onboarding_recent_project_review_prompt(&repository);
        self.cursor_pos = self.input.len();
        true
    }

    /// Start the proactive recent-project review on the active runtime.
    ///
    /// Local TUIs consume `pending_turn` in their run loop, while remote-attached
    /// TUIs send queued messages from the remote tick loop. Calling
    /// [`App::submit_input`] in remote mode leaves the client permanently parked
    /// in `Sending` because no local run loop exists to consume that flag.
    pub(super) fn onboarding_start_recent_project_review(&mut self) {
        if !self.onboarding_prepare_recent_project_review() {
            return;
        }
        self.follow_chat_bottom_for_typing();
        if self.is_remote {
            super::input::queue_message(self);
            self.set_status_notice("Architecture review queued");
        } else {
            self.submit_input();
        }
    }

    /// Drop into the suggestion-card state (the "No" / no-OAuth path). Prints
    /// the same starter prompts the empty-screen welcome offers, as an inline
    /// numbered list the user can pick by typing the number or anything else.
    ///
    /// This is also the "Start a new session" landing screen on first run. We
    /// intentionally keep it clean: the usual login/import system chatter is
    /// suppressed while onboarding drives the UI, and instead of that noise we
    /// kick off a single lightweight live validation of the auto-selected
    /// default model and report it as one tidy "ready"/"failed" line.
    pub(super) fn onboarding_show_suggestions(&mut self) {
        if let Some(flow) = self.onboarding_flow.as_mut() {
            flow.phase = OnboardingPhase::Suggestions;
        }
        let suggestions = self.suggestion_prompts();
        if suggestions.is_empty() {
            self.onboarding_finish();
            self.set_status_notice("You're all set, type anything to start");
            self.onboarding_validate_default_model();
            return;
        }
        let mut body = String::from("Here are a few things you can try:\n");
        for (i, (label, _prompt)) in suggestions.iter().enumerate() {
            body.push_str(&format!("  [{}] {}\n", i + 1, label));
        }
        body.push_str(&format!(
            "Press 1-{} to use one, or just type anything to start.",
            suggestions.len()
        ));
        self.push_display_message(DisplayMessage::system(body));
        self.set_status_notice("Try a suggestion, or type anything to start");
        self.onboarding_validate_default_model();
    }

    /// Friendly label for the active default model, including the reasoning
    /// effort tier when one applies (e.g. "GPT-5.5 (low)"). Used by the
    /// onboarding new-session validation line.
    fn onboarding_default_model_label(&self) -> String {
        let model = self.onboarding_default_model_id();
        let pretty = super::model_names::pretty_model_display_name(&model);
        match self.provider.reasoning_effort() {
            Some(effort) if !effort.trim().is_empty() && effort != "none" => {
                let effort_label = super::helpers::effort_display_label(&effort);
                format!("{} ({})", pretty, effort_label.to_ascii_lowercase())
            }
            _ => pretty,
        }
    }

    /// Resolve the raw id of the default model the new-session screen is about
    /// to use. In remote/client mode the live model is reported by the server,
    /// so prefer the same resolution the header uses; fall back to the session
    /// model and finally the local provider's model.
    fn onboarding_default_model_id(&self) -> String {
        if self.is_remote
            && let Some(model) = self.effective_remote_provider_model()
        {
            return model;
        }
        self.session
            .model
            .clone()
            .filter(|m| !m.trim().is_empty() && !m.eq_ignore_ascii_case("unknown"))
            .unwrap_or_else(|| self.provider.model())
    }

    /// Request a one-shot, lightweight live validation of the auto-selected
    /// default model for the clean new-session screen. We want a single line
    /// that tells the user their default model is actually working, rather than
    /// the usual login/import status spam.
    ///
    /// In remote/client mode the live default model is reported by the server
    /// asynchronously, so firing immediately can race ahead of the model id
    /// being known (resolving to "unknown" and validating the wrong provider).
    /// Instead we record a pending request and let `onboarding_tick` fire it
    /// once a concrete model id is available (or a short timeout elapses).
    pub(super) fn onboarding_validate_default_model(&mut self) {
        if !crate::auth::AuthStatus::check_fast().has_any_available() {
            return;
        }
        // After a login/import, provider activation and strongest-model selection
        // finish asynchronously in both local and remote mode. The pre-login
        // default is often already a concrete id, so validating immediately would
        // ping the stale provider and make Continue look broken. Defer until the
        // auth catalog refresh completes (or a short timeout).
        if self.recent_authenticated_provider.is_some() && self.auth_catalog_refresh_pending {
            self.onboarding_pending_model_validation =
                Some(OnboardingPendingValidation::awaiting_catalog_refresh(
                    self.session.id.clone(),
                    self.remote_model_catalog_generation,
                ));
            return;
        }
        // If we already know a concrete model (typically local mode), run it
        // right away; otherwise defer to the tick loop until the server reports
        // the live model id.
        if self.onboarding_default_model_id_is_concrete() {
            self.onboarding_spawn_model_validation();
        } else {
            self.onboarding_pending_model_validation =
                Some(OnboardingPendingValidation::new(self.session.id.clone()));
        }
    }

    /// Whether we currently have a concrete (non-"unknown") default model id to
    /// validate. In remote mode this becomes true once the server reports the
    /// live model.
    fn onboarding_default_model_id_is_concrete(&self) -> bool {
        let model = self.onboarding_default_model_id();
        let trimmed = model.trim();
        !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unknown")
    }

    /// Spawn the background validation ping for the current default model.
    fn onboarding_spawn_model_validation(&mut self) {
        let Some(provider) = self.onboarding_validation_provider() else {
            return;
        };
        let model_label = self.onboarding_default_model_label();
        let provider_key = crate::session::derive_session_provider_key(provider.name());
        let session_id = self.session.id.clone();
        self.set_status_notice(format!("Checking {model_label}..."));
        tokio::spawn(async move {
            let (ok, detail) = match Self::onboarding_run_model_validation(provider).await {
                Ok(()) => (true, None),
                Err(err) => (false, Some(Self::onboarding_trim_validation_error(&err))),
            };
            crate::bus::Bus::global().publish(crate::bus::BusEvent::OnboardingModelValidated(
                crate::bus::OnboardingModelValidated {
                    session_id,
                    model_label,
                    provider_key,
                    ok,
                    detail,
                },
            ));
        });
    }

    /// Drive a pending (deferred) model validation from the onboarding tick.
    /// Returns true if it fired this tick. Fires once a concrete model id is
    /// known, or after a short resolve timeout so the line always appears.
    pub(super) fn onboarding_tick_model_validation(&mut self) -> bool {
        let Some(pending) = self.onboarding_pending_model_validation.as_ref() else {
            return false;
        };
        if pending.session_id != self.session.id {
            // Session changed out from under us; drop the stale request.
            self.onboarding_pending_model_validation = None;
            return false;
        }
        if !self.onboarding_pending_validation_ready_to_fire() {
            return false;
        }
        self.onboarding_pending_model_validation = None;
        self.onboarding_spawn_model_validation();
        true
    }

    /// Whether the currently-pending validation should fire this tick. Pure
    /// decision logic (no side effects) so it can be unit-tested without the
    /// `tokio::spawn` in `onboarding_spawn_model_validation`.
    ///
    /// When waiting for the post-login catalog refresh, hold until the remote
    /// catalog generation advances or the local auth-refresh flag clears, so we
    /// validate the freshly-selected model rather than the stale pre-login
    /// default. The resolve timeout is always a backstop.
    pub(super) fn onboarding_pending_validation_ready_to_fire(&self) -> bool {
        let Some(pending) = self.onboarding_pending_model_validation.as_ref() else {
            return false;
        };
        let timed_out = pending.resolve_timed_out();
        if pending.await_catalog_refresh {
            let refreshed = if self.is_remote {
                self.remote_model_catalog_generation > pending.catalog_generation_at_request
            } else {
                !self.auth_catalog_refresh_pending
            };
            return refreshed || timed_out;
        }
        self.onboarding_default_model_id_is_concrete() || timed_out
    }

    /// Build the provider used for the onboarding model-validation ping.
    ///
    /// In local mode we fork the live provider. In remote/client mode the app's
    /// `self.provider` is a `NullProvider` (real turns run in the backend), so
    /// we spin up a real local provider and pin it to the displayed session
    /// model so the ping exercises the same model the user is about to use.
    fn onboarding_validation_provider(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::provider::Provider>> {
        if !self.is_remote {
            return Some(self.provider.fork());
        }
        let provider: std::sync::Arc<dyn crate::provider::Provider> =
            std::sync::Arc::new(crate::provider::MultiProvider::new_fast());
        let model = self.onboarding_default_model_id();
        if !model.trim().is_empty() && !model.eq_ignore_ascii_case("unknown") {
            // Best-effort: if the model can't be set locally we still ping the
            // provider default, which is enough to confirm credentials work.
            let _ = provider.set_model(&model);
        }
        Some(provider)
    }

    /// Run the lightweight live validation ping against the active provider.
    /// Succeeds as long as the provider returns any non-empty completion.
    async fn onboarding_run_model_validation(
        provider: std::sync::Arc<dyn crate::provider::Provider>,
    ) -> anyhow::Result<()> {
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            provider.complete_simple(
                "Reply with exactly: OK",
                "You are validating connectivity. Reply with exactly: OK",
            ),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timed out after 30s"))??;
        if reply.trim().is_empty() {
            anyhow::bail!("empty response");
        }
        Ok(())
    }

    /// Condense a validation error into a short, user-facing detail string.
    ///
    /// Provider errors are often a full JSON blob on a single line; we map the
    /// common cases to a tidy phrase so the onboarding summary stays readable,
    /// and otherwise fall back to a clipped first line.
    fn onboarding_trim_validation_error(err: &anyhow::Error) -> String {
        let msg = err.to_string();
        let lower = msg.to_ascii_lowercase();
        // Common, recognizable failures get a short canonical phrase.
        if lower.contains("401")
            || lower.contains("unauthorized")
            || lower.contains("invalid authentication")
            || lower.contains("invalid api key")
            || lower.contains("invalid x-api-key")
        {
            return "login expired or invalid".to_string();
        }
        if lower.contains("timed out") || lower.contains("timeout") {
            return "timed out".to_string();
        }
        if lower.contains("429") || lower.contains("rate limit") {
            return "rate limited".to_string();
        }
        if lower.contains("empty response") {
            return "no response".to_string();
        }
        let first_line = msg.lines().next().unwrap_or(&msg).trim();
        let trimmed: String = first_line.chars().take(100).collect();
        if trimmed.is_empty() {
            "unknown error".to_string()
        } else {
            trimmed
        }
    }

    /// Whether a validation detail string looks like an authentication failure
    /// (expired/invalid credentials), which is fixed by logging in again rather
    /// than by switching models.
    fn onboarding_detail_looks_like_auth(detail: &str) -> bool {
        let lower = detail.to_ascii_lowercase();
        lower.contains("401")
            || lower.contains("unauthorized")
            || lower.contains("authentication")
            || lower.contains("invalid api key")
            || lower.contains("invalid x-api-key")
            || lower.contains("credentials")
            || lower.contains("login expired")
            || lower.contains("expired or invalid")
    }

    /// resonix 化后没有交互式登录可建议：模型接入只走 `[[providers]]` 配置 +
    /// 统一 `.env`。返回配置引导命令供文案引用。
    pub(super) fn onboarding_provider_add_hint() -> String {
        "`jcode provider add <name> --base-url <url> --api-key-env <ENV_VAR>`".to_string()
    }

    /// Build the "other providers" rows for the onboarding readiness summary.
    ///
    /// We already ran a live ping for the default model; for the remaining
    /// configured providers we trust the cached auth probe (Available -> ready,
    /// Expired -> needs attention). `skip` is the provider key backing the
    /// default model so we don't list it twice.
    fn onboarding_other_provider_rows(skip: Option<&str>) -> (Vec<String>, Vec<String>) {
        use crate::auth::AuthState;
        let status = crate::auth::AuthStatus::check_fast();
        // Config-driven mode: only allowlisted providers appear in the
        // readiness summary, so removed built-ins never show up here.
        let providers = crate::provider_catalog::auth_status_login_providers_filtered()
            .into_iter()
            .filter(|provider| {
                !matches!(
                    provider.target,
                    crate::provider_catalog::LoginProviderTarget::AutoImport
                )
            })
            .map(|provider| {
                (
                    provider.display_name.to_string(),
                    provider.id.to_string(),
                    status.state_for_provider(provider),
                )
            })
            .collect::<Vec<_>>();
        let skip = skip.map(|s| s.trim().to_ascii_lowercase());
        let mut ready = Vec::new();
        let mut attention = Vec::new();
        for (name, key, state) in providers {
            if skip.as_deref() == Some(key.as_str()) {
                continue;
            }
            match state {
                AuthState::Available => ready.push(name.to_string()),
                AuthState::Expired => attention.push(format!("{name} - login expired")),
                AuthState::NotConfigured => {}
            }
        }
        (ready, attention)
    }

    /// Handle the result of the onboarding default-model validation: render one
    /// clean readiness summary listing the logins that work and the ones that
    /// need attention. Stale results (from a previous session) are ignored.
    pub(super) fn handle_onboarding_model_validated(
        &mut self,
        result: crate::bus::OnboardingModelValidated,
    ) -> bool {
        if result.session_id != self.session.id {
            return false;
        }

        let detail_text = result.detail.clone().unwrap_or_default();
        let looks_like_auth = !result.ok && Self::onboarding_detail_looks_like_auth(&detail_text);

        // Gather the other configured providers so the summary shows the full
        // picture, not just the default model. We skip the default model's own
        // provider since it gets the live-ping line below.
        let (mut ready, mut attention) =
            Self::onboarding_other_provider_rows(result.provider_key.as_deref());

        // Place the freshly-pinged default model at the top of whichever list it
        // belongs in, so it always reads first.
        if result.ok {
            ready.insert(0, format!("{} (default)", result.model_label));
        } else {
            let reason = if detail_text.is_empty() {
                "could not be validated".to_string()
            } else {
                detail_text.clone()
            };
            attention.insert(0, format!("{} (default) - {reason}", result.model_label));
        }

        // Render a single tidy block with the two sections.
        let mut body = String::new();
        if !ready.is_empty() {
            body.push_str("**Ready to use**\n");
            for row in &ready {
                body.push_str(&format!("- ✓ {row}\n"));
            }
        }
        if !attention.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str("**Needs attention**\n");
            for row in &attention {
                body.push_str(&format!("- ✕ {row}\n"));
            }
            let login = Self::onboarding_provider_add_hint();
            let fix_hint = if looks_like_auth || !ready.is_empty() {
                format!("Run {login} to fix provider credentials, or /model to pick another.")
            } else {
                format!("Run {login} to add a provider, or /model to pick another.")
            };
            body.push_str(&format!("\n{fix_hint}"));
        }
        if body.is_empty() {
            // Defensive: should not happen because the default model always
            // lands in one of the lists.
            body.push_str("Type anything to start.");
        }
        self.push_display_message(DisplayMessage::system(body.trim_end().to_string()));

        // Status-bar notice: concise, action-oriented.
        if attention.is_empty() {
            self.set_status_notice(format!(
                "{} ready - type anything to start",
                result.model_label
            ));
        } else if result.ok {
            self.set_status_notice(format!(
                "{} ready - type anything to start ({} login{} need attention)",
                result.model_label,
                attention.len(),
                if attention.len() == 1 { "" } else { "s" }
            ));
        } else {
            let hint = if looks_like_auth {
                format!(
                    "{} to fix credentials, or /model",
                    Self::onboarding_provider_add_hint()
                )
            } else {
                "type anything to try, or /model".to_string()
            };
            self.set_status_notice(format!("{} not validated - {hint}", result.model_label));
        }
        true
    }

    /// Mark the flow complete; the normal UI takes over.
    pub(super) fn onboarding_finish(&mut self) {
        self.onboarding_auto_model_selection_active
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(flow) = self.onboarding_flow.as_mut() {
            flow.phase = OnboardingPhase::Done;
        }
    }

    /// Drive auto-advancing phases. Call once per tick/redraw. Returns true if
    /// the flow state changed (so the caller can request a redraw).
    pub(super) fn onboarding_tick(&mut self) -> bool {
        // The onboarding simulator drives phases manually; never auto-advance
        // while it is walking screens.
        if self.onboarding_sim_active() {
            return false;
        }
        // Fresh-install bootstrap: if we were already logged in at the CLI before
        // the TUI launched, no in-TUI login event fired, so evaluate (once)
        // whether to begin the guided flow now that the TUI is up.
        let mut changed = false;
        if !self.onboarding_startup_checked {
            self.maybe_begin_onboarding_flow_on_startup();
            // If startup just kicked the flow on, request a redraw.
            changed = self.onboarding_flow_active();
        }
        // Drive the deferred new-session model validation independently of the
        // flow phase: it may be requested right as the flow finishes (the
        // no-transcripts path calls `onboarding_finish()` before validating), so
        // gating it on `onboarding_flow_active()` would strand it forever.
        if self.onboarding_tick_model_validation() {
            changed = true;
        }
        changed
    }
}
