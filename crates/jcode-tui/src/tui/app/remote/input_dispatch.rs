use super::super::{PendingRemoteMessage, PendingSplitPrompt};
use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "remote send needs explicit message payload, reminders, retry metadata, and image attachments"
)]
pub(in crate::tui::app) async fn begin_remote_send(
    app: &mut App,
    remote: &mut RemoteConnection,
    content: String,
    images: Vec<(String, String)>,
    is_system: bool,
    system_reminder: Option<String>,
    auto_retry: bool,
    retry_attempts: u8,
) -> Result<u64> {
    let msg_id = remote
        .send_message_with_images_and_reminder(
            content.clone(),
            images.clone(),
            system_reminder.clone(),
        )
        .await?;
    app.current_message_id = Some(msg_id);
    app.deferred_stream_done_id = None;
    app.is_processing = true;
    app.status = ProcessingStatus::Sending;
    app.status_detail = None;
    app.processing_started = Some(Instant::now());
    if !content.is_empty() {
        if is_system {
            app.visible_turn_started.get_or_insert_with(Instant::now);
        } else {
            app.visible_turn_started = Some(Instant::now());
        }
    }
    app.last_stream_activity = Some(Instant::now());
    app.remote_resume_activity = None;
    app.reset_streaming_tps();
    // New turn -> new API call: the next usage report must replace, not merge
    // into, the previous call's cache counters (issue #441). Newer servers
    // also emit KvCacheRequest per call, which re-arms this flag per call.
    app.mark_stream_usage_call_boundary();
    app.thought_line_inserted = false;
    app.thinking_prefix_emitted = false;
    app.thinking_buffer.clear();
    app.rate_limit_pending_message = Some(PendingRemoteMessage {
        content,
        images,
        is_system,
        system_reminder,
        auto_retry,
        retry_attempts,
        retry_at: None,
    });
    app.autoreview_after_current_turn = !is_system;
    app.autojudge_after_current_turn = !is_system;
    remote.reset_call_output_tokens_seen();
    Ok(msg_id)
}

pub(in crate::tui::app) fn restore_prepared_remote_input(
    app: &mut App,
    prepared: input::PreparedInput,
) {
    app.input = prepared.raw_input;
    app.cursor_pos = app.input.len();
    app.pending_images = prepared.images;
}

pub(in crate::tui::app) fn history_matches_pending_startup_prompt(app: &App) -> bool {
    if !app.submit_input_on_startup || !app.pending_images.is_empty() || app.input.trim().is_empty()
    {
        return false;
    }

    app.display_messages()
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .is_some_and(|message| message.content == app.input)
}

pub(in crate::tui::app) async fn submit_prepared_remote_input(
    app: &mut App,
    remote: &mut RemoteConnection,
    prepared: input::PreparedInput,
) -> Result<()> {
    if app.remote_model_switch_in_flight || app.auth_catalog_refresh_pending {
        app.pending_prompt_after_model_switch = Some(prepared);
        app.set_status_notice(if app.auth_catalog_refresh_pending {
            "Prompt queued until model setup completes"
        } else {
            "Prompt queued until model switch completes"
        });
        return Ok(());
    }

    // Submitting before the bootstrap History payload has been applied is racy:
    // the session-change branch of the History handler calls
    // `clear_display_messages()`, which wipes the user message we are about to
    // echo locally (the prompt appears to "vanish" while the server still
    // streams a reply against it). Hold the prompt and let
    // `process_remote_followups` dispatch it once history is loaded - the same
    // gating that startup auto-submit already relies on.
    if !remote.has_loaded_history() {
        crate::logging::info(
            "Deferring manually submitted prompt until remote history loads (avoids first-prompt clobber)",
        );
        app.pending_prompt_before_history = Some(prepared);
        app.set_status_notice("Loading session...");
        return Ok(());
    }

    if let Some(command) = input::extract_input_shell_command(&prepared.expanded) {
        submit_remote_input_shell(app, remote, prepared.raw_input, command.to_string()).await?;
        return Ok(());
    }

    app.commit_pending_streaming_assistant_message();
    // A manually submitted prompt supersedes any armed post-error fallback
    // offer (and its staged resend): the user chose to continue differently.
    app.clear_pending_fallback_offer();
    // Remember the typed prompt so we can restore it to the input box if this turn
    // fails (e.g. "token refresh needed"), instead of dropping it.
    app.last_submitted_input = Some(prepared.raw_input.clone());
    app.push_display_message(DisplayMessage {
        role: "user".to_string(),
        content: prepared.raw_input,
        tool_calls: vec![],
        duration_secs: None,
        title: None,
        tool_data: None,
    });
    let _ = app
        .begin_remote_send(remote, prepared.expanded, prepared.images, false)
        .await;
    Ok(())
}

/// Route a slash input through the remote client instead of the local
/// `submit_input` path. Built-in slash commands still belong to the client,
/// but a skill invocation with a trailing prompt must become a remote turn.
/// Calling `App::submit_input` directly for that case sets `pending_turn`,
/// which only the local run loop consumes, leaving remote sessions stuck in
/// the sending state.
pub(in crate::tui::app) async fn submit_remote_slash_input(
    app: &mut App,
    remote: &mut RemoteConnection,
    prepared: input::PreparedInput,
) -> Result<()> {
    let raw_input = prepared.raw_input.clone();

    // Re-read global skills from disk for slash-leading input before resolving
    // so a skill added or edited while this session was open is visible to the
    // running remote session immediately (in-progress refresh), matching the
    // local path.
    if raw_input.trim_start().starts_with('/') {
        app.refresh_skills_snapshot();
    }

    // Text that merely starts with `/` is not necessarily a command. A terminal
    // file drop (`/tmp/shot.png`) or a bare path (`/home/me/notes`) is ordinary
    // user input. Routing those through `App::submit_input` stages a *local*
    // turn via `pending_turn`, which no remote run loop consumes, so the client
    // parks in "Sending" forever. Send them as a normal remote turn instead.
    //
    // `/?` is the one builtin whose token is not identifier-shaped, so it is
    // allowed through explicitly.
    // Resolve registered multi-word skill names before falling back to the
    // existing single-token command handling.
    let snapshot = app.current_skills_snapshot();
    let trimmed = raw_input.trim();
    let is_command_shaped = trimmed == "/?"
        || (input::parse_dropped_paths(&raw_input).is_none()
            && snapshot.resolve_invocation(&raw_input).is_some());
    if !is_command_shaped {
        return submit_prepared_remote_input(app, remote, prepared).await;
    }

    let Some(invocation) = snapshot.resolve_invocation(&raw_input) else {
        app.input = raw_input;
        app.cursor_pos = app.input.len();
        app.submit_input();
        return Ok(());
    };

    let Some(trailing_prompt) = invocation.prompt else {
        app.input = raw_input;
        app.cursor_pos = app.input.len();
        app.submit_input();
        return Ok(());
    };

    let skill_name = invocation.name.to_string();
    let mut skill = snapshot.get(&skill_name).cloned();
    if skill.is_none() {
        app.refresh_skills_snapshot();
        skill = app.current_skills_snapshot().get(&skill_name).cloned();
    }
    if skill.is_none() {
        // Preserve the existing unknown-skill and built-in slash-command
        // handling, including the helpful endorsed-skill installation hint.
        app.input = raw_input;
        app.cursor_pos = app.input.len();
        app.submit_input();
        return Ok(());
    }

    // Reuse the normal bare invocation path to update active_skill and show
    // the activation notice, then prepare only the trailing prompt for the
    // remote request. This avoids duplicating slash-command presentation and
    // keeps pasted images attached to the same user turn.
    app.input = format!("/{}", skill_name);
    app.cursor_pos = app.input.len();
    app.pending_images.clear();
    app.submit_input();

    let expanded_prompt = app
        .current_skills_snapshot()
        .resolve_invocation(&prepared.expanded)
        .and_then(|invocation| invocation.prompt)
        .unwrap_or(trailing_prompt)
        .to_string();
    submit_prepared_remote_input(
        app,
        remote,
        input::PreparedInput {
            raw_input: prepared.raw_input,
            expanded: expanded_prompt,
            images: prepared.images,
        },
    )
    .await
}

pub(in crate::tui::app) async fn route_prepared_input_to_new_remote_session(
    app: &mut App,
    remote: &mut RemoteConnection,
    prepared: input::PreparedInput,
) -> Result<()> {
    app.route_next_prompt_to_new_session = false;
    app.pending_split_startup_message = None;
    app.pending_split_prompt = Some(PendingSplitPrompt {
        content: prepared.expanded,
        images: prepared.images,
    });
    app.pending_split_model_override = None;
    app.pending_split_provider_key_override = None;
    app.pending_split_label = Some("Prompt".to_string());
    app.pending_split_started_at = Some(Instant::now());

    app.pending_split_request = false;
    if app.is_processing {
        app.set_status_notice("Prompt launching in new session");
        if let Err(error) = remote.split().await {
            let pending = app
                .pending_split_prompt
                .take()
                .map(|prompt| input::PreparedInput {
                    raw_input: prepared.raw_input,
                    expanded: prompt.content,
                    images: prompt.images,
                });
            app.pending_split_model_override = None;
            app.pending_split_provider_key_override = None;
            app.pending_split_label = None;
            if let Some(prepared) = pending {
                restore_prepared_remote_input(app, prepared);
            }
            return Err(error);
        }
        return Ok(());
    }

    begin_remote_split_launch(app, "Prompt");
    if let Err(error) = remote.split().await {
        finish_remote_split_launch(app);
        let pending = app
            .pending_split_prompt
            .take()
            .map(|prompt| input::PreparedInput {
                raw_input: prepared.raw_input,
                expanded: prompt.content,
                images: prompt.images,
            });
        app.pending_split_model_override = None;
        app.pending_split_provider_key_override = None;
        app.pending_split_label = None;
        if let Some(prepared) = pending {
            restore_prepared_remote_input(app, prepared);
        }
        return Err(error);
    }
    Ok(())
}

pub(in crate::tui::app) fn begin_remote_split_launch(app: &mut App, label: &str) {
    app.is_processing = true;
    app.status = ProcessingStatus::Sending;
    app.status_detail = None;
    let started_at = Instant::now();
    app.pending_split_started_at = Some(started_at);
    app.processing_started = Some(started_at);
    app.last_stream_activity = Some(started_at);
    app.remote_resume_activity = None;
    app.reset_streaming_tps();
    app.thought_line_inserted = false;
    app.thinking_prefix_emitted = false;
    app.thinking_buffer.clear();
    app.current_message_id = None;
    app.set_status_notice(format!("{} launching", label));
}

pub(in crate::tui::app) fn finish_remote_split_launch(app: &mut App) {
    if !app.is_processing || app.current_message_id.is_some() {
        return;
    }
    if !matches!(app.status, ProcessingStatus::Sending) {
        return;
    }
    app.is_processing = false;
    app.status = ProcessingStatus::Idle;
    app.stream_message_ended = false;
    app.processing_started = None;
    app.clear_visible_turn_started();
    app.last_stream_activity = None;
    app.reset_streaming_tps();
    app.current_message_id = None;
}

async fn submit_remote_input_shell(
    app: &mut App,
    remote: &mut RemoteConnection,
    raw_input: String,
    command: String,
) -> Result<()> {
    app.commit_pending_streaming_assistant_message();
    app.push_display_message(DisplayMessage::user(raw_input));

    if command.trim().is_empty() {
        app.push_display_message(DisplayMessage::system(
            "Shell command cannot be empty after !.",
        ));
        app.set_status_notice("Shell command is empty");
        return Ok(());
    }

    let request_id = remote.send_input_shell(command.clone()).await?;
    app.current_message_id = Some(request_id);
    app.is_processing = true;
    app.status = ProcessingStatus::Sending;
    app.status_detail = None;
    app.processing_started = Some(Instant::now());
    app.visible_turn_started = Some(Instant::now());
    app.last_stream_activity = Some(Instant::now());
    app.remote_resume_activity = None;
    app.reset_streaming_tps();
    app.thought_line_inserted = false;
    app.thinking_prefix_emitted = false;
    app.thinking_buffer.clear();
    app.rate_limit_pending_message = None;
    remote.reset_call_output_tokens_seen();
    app.set_status_notice(format!(
        "Running remote shell: {}",
        crate::util::truncate_str(&command, 48)
    ));
    Ok(())
}

/// Stage a submitted turn for the remote tick loop when the app is attached to
/// a remote session, returning true when it took ownership of the turn.
///
/// Only the LOCAL run loop consumes `App::pending_turn`. Any path that reaches
/// `App::submit_input` while remote (a slash command that turned out not to be
/// one, an unknown skill fallback, a staged prompt) would otherwise set a flag
/// nobody dispatches, freezing the client in "Sending" forever. Queueing hands
/// the turn to `process_remote_followups`, which also echoes the user message.
pub(in crate::tui::app) fn stage_turn_for_remote_tick_loop(app: &mut App, input: &str) -> bool {
    if !app.is_remote {
        return false;
    }
    if app.is_processing && !app.queue_mode {
        let images = std::mem::take(&mut app.pending_images);
        input::stage_local_interleave(app, input.to_string(), images);
        return true;
    }
    app.queued_messages.push(input.to_string());
    app.pending_images.clear();
    true
}
