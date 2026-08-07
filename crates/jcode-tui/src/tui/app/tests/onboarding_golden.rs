// Golden state-space walker for the first-run onboarding welcome screen.
//
// This renders every onboarding phase to an offscreen TestBackend and captures
// the exact text the user sees. It serves two purposes:
//
//   1. A reviewable, deterministic dump of every onboarding screen (run with
//      `--nocapture` to read them), so we can verify every word of copy without
//      manually walking the live flow.
//   2. Regression guards on the exact wording / option layout of each phase.
//
// To see all rendered screens:
//   cargo test -p jcode-tui onboarding_golden -- --nocapture

// NOTE: This file is `include!`d into `crate::tui::app::tests`, which already
// imports `OnboardingFlow`, and `OnboardingPhase` via the sibling
// `onboarding_flow.rs` include. To avoid duplicate-import errors we
// reference types through fully-qualified paths / local aliases below instead
// of adding module-level `use` statements.

/// Render the onboarding welcome screen for `app` into a fixed-size buffer and
/// return the visible text, one line per row, trailing blank rows trimmed.
fn render_onboarding_text(app: &App, width: u16, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            crate::tui::ui::draw_onboarding_welcome_for_tests(frame, app, area);
        })
        .unwrap();

    let buffer = terminal.backend().buffer().clone();
    let mut rows: Vec<String> = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = String::new();
        for x in 0..width {
            row.push_str(buffer[(x, y)].symbol());
        }
        rows.push(row.trim_end().to_string());
    }
    while rows.last().map(|r| r.is_empty()).unwrap_or(false) {
        rows.pop();
    }
    rows.join("\n")
}

/// Force the app into a specific onboarding phase, bypassing the on-disk
/// new-user heuristic.
fn app_in_phase(phase: OnboardingPhase) -> App {
    let mut app = create_test_app();
    let mut flow = OnboardingFlow::begin();
    flow.phase = phase;
    app.onboarding_flow = Some(flow);
    app
}

fn dump(title: &str, text: &str) {
    println!("\n========== {title} ==========");
    println!("{text}");
    println!("==========================================");
}

#[test]
fn onboarding_golden_walks_every_phase() {
    let width = 80u16;
    let height = 30u16;

    // 1. "Configure a model provider?" Yes/No prompt (config-driven; external
    // login imports are not part of live onboarding).
    {
        let app = app_in_phase(OnboardingPhase::ConfigureProvider {
            yes_highlighted: true,
        });
        let text = render_onboarding_text(&app, width, height);
        dump("Configure provider prompt", &text);
        // Lean prompt: just the question + the Yes/No lozenge pills. The Esc hint
        // already covers the "skip / configure later" path, so no extra prose.
        assert!(text.contains("Configure a model provider?"), "{text}");
        assert!(text.contains("Yes") && text.contains("No"), "{text}");
        assert!(
            text.contains("\u{25D6} Yes \u{25D7}") && text.contains("\u{25D6} No \u{25D7}"),
            "yes/no lozenge pills: {text}"
        );
        // The redundant "Choose No to skip" line was removed.
        assert!(
            !text.contains("Choose \"No\" to skip"),
            "redundant skip line should be gone: {text}"
        );
    }

    // 2. Suggestions (resting state).
    {
        let app = app_in_phase(OnboardingPhase::Suggestions);
        let text = render_onboarding_text(&app, width, height);
        dump("Suggestions", &text);
        assert!(text.contains("Welcome to jcode onboarding"), "{text}");
    }
}

/// Guided-screen polish walk: enforces polish invariants on every guided
/// screen that remains in the config-driven flow:
///
///   * It always renders the welcome title + tagline (no blank/garbled card).
///   * Every guided screen advertises the universal Esc escape hatch, so the
///     user can always see a way out (the liveness guarantee, made visible).
///
/// Run with `--nocapture` to eyeball every screen.
#[test]
fn onboarding_golden_walks_failure_and_async_states() {
    let width = 80u16;
    let height = 32u16;

    // Helper: assert the shared polish invariants for a guided screen.
    let assert_guided_polish = |title: &str, text: &str| {
        assert!(
            text.contains("Welcome to jcode onboarding"),
            "{title}: must render the welcome title\n{text}"
        );
        assert!(
            text.contains("Esc to skip onboarding"),
            "{title}: every guided screen must advertise the Esc escape hatch\n{text}"
        );
    };

    // The "Configure a model provider?" prompt must advertise the Esc escape
    // hatch (polish invariant across guided screens).
    {
        let app = app_in_phase(OnboardingPhase::ConfigureProvider {
            yes_highlighted: true,
        });
        let text = render_onboarding_text(&app, width, height);
        dump("ConfigureProvider (Esc hint)", &text);
        assert_guided_polish("ConfigureProvider", &text);
    }
}

