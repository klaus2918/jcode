//! Plan-confirmation gate: op-style "plan -> user confirmation -> execute"
//! discipline for user-driven sessions (`agent-plan-confirm-discipline`).
//!
//! Default behavior (`features.require_plan_confirmation = true`): every user
//! request starts unconfirmed; mutating tools are refused until the user
//! explicitly confirms ("确认/继续/可以执行"...) or pre-authorizes the request
//! ("直接改，不用确认"). Read-only tools are never blocked.
//!
//! Sessions only become gated when an interactive client message registers with
//! [`begin_user_request`]: the server dispatches client turns through
//! `server::client_lifecycle::start_processing_message` (TUI / `jcode client`),
//! and the plain `jcode repl` loop registers before each turn. Mid-turn user
//! messages (soft interrupts) feed [`note_user_message`], which can only
//! confirm the in-flight request, never start a new one.
//!
//! Non-interactive turns (swarm workers, agent comms, background tasks, reload
//! recovery, ambient automation, `jcode run` batch execution) never register
//! and are therefore exempt, which keeps autonomous flows working while the
//! interactive session is held to the confirmation discipline.
//!
//! The state is process-local and keyed by session id: it is a runtime guard,
//! not a persisted credential. Losing it (restart / session switch) can only
//! re-lock, never leak a confirmation. The agent has no tool that writes the
//! confirmation; only a user action (message) can move `confirmed_epoch`.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Tools that change the workspace or run external work. Refused while the
/// current user request is unconfirmed. `batch` is absent on purpose: its
/// sub-calls re-enter `Registry::execute` and are checked individually there.
const MUTATING_TOOLS: &[&str] = &[
    "write",
    "edit",
    "multiedit",
    "patch",
    "apply_patch",
    "bash",
    "bg",
    "selfdev",
    "swarm",
    "skill_manage",
];

#[derive(Clone, Copy, Debug, Default)]
struct PlanGateState {
    /// Latest user request seen for this session (1-based; 0 = none yet).
    epoch: u64,
    /// Highest epoch the user explicitly confirmed or pre-authorized.
    confirmed_epoch: u64,
}

static PLAN_GATE: LazyLock<RwLock<HashMap<String, PlanGateState>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn feature_enabled() -> bool {
    crate::config::config().features.require_plan_confirmation
}

/// Register the start of a user-driven turn (interactive chat entry).
///
/// Synthetic auto-poke continuations are skipped: they continue the current
/// request rather than starting a new one, so they must not reset the state.
pub(crate) fn begin_user_request(session_id: &str, user_message: &str) {
    begin_user_request_with_media(session_id, user_message, false);
}

/// Like [`begin_user_request`], but knows whether the turn carries media
/// attachments. An image-only client message (empty text) is still a real user
/// request and must register; the plain text entry stays skipped so transport
/// continuations that send empty content are not misread as new requests.
pub(crate) fn begin_user_request_with_media(session_id: &str, user_message: &str, has_media: bool) {
    let trimmed = user_message.trim();
    if (trimmed.is_empty() && !has_media) || crate::todo::is_auto_poke_message(trimmed) {
        return;
    }
    let confirmed = !trimmed.is_empty()
        && (is_explicit_confirmation(trimmed) || is_explicit_authorization(trimmed));
    let mut map = PLAN_GATE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = map.entry(session_id.to_string()).or_default();
    state.epoch += 1;
    if confirmed {
        state.confirmed_epoch = state.epoch;
    }
    crate::logging::info(&format!(
        "[plan-gate] request session_id={} epoch={} confirmed={}",
        session_id, state.epoch, confirmed
    ));
}

/// Note a mid-turn user message (interactive soft interrupt) for the request
/// that is already in flight. A confirmation or direct authorization unlocks
/// the current epoch without starting a new one; any other message leaves the
/// state untouched, so a mid-turn instruction neither resets a confirmed
/// request nor unlocks an unconfirmed one. Untracked sessions (non-interactive
/// callers) stay exempt.
pub(crate) fn note_user_message(session_id: &str, user_message: &str) {
    let trimmed = user_message.trim();
    if trimmed.is_empty()
        || (!is_explicit_confirmation(trimmed) && !is_explicit_authorization(trimmed))
    {
        return;
    }
    let mut map = PLAN_GATE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(state) = map.get_mut(session_id) else {
        return;
    };
    if state.confirmed_epoch < state.epoch {
        state.confirmed_epoch = state.epoch;
        crate::logging::info(&format!(
            "[plan-gate] mid-turn confirmation session_id={} epoch={}",
            session_id, state.epoch
        ));
    }
}

/// Drop gate state for a session that is being replaced (restore/switch).
pub(crate) fn forget(session_id: &str) {
    let mut map = PLAN_GATE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.remove(session_id);
}

/// `Some(message)` when the call must be refused by the gate.
pub(crate) fn check(session_id: &str, tool_name: &str) -> Option<String> {
    check_with(feature_enabled(), session_id, tool_name)
}

fn check_with(enabled: bool, session_id: &str, tool_name: &str) -> Option<String> {
    if !enabled || !is_mutating_tool(tool_name) {
        return None;
    }
    let map = PLAN_GATE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = map.get(session_id)?;
    if state.confirmed_epoch >= state.epoch {
        return None;
    }
    Some(block_message(tool_name, state.epoch))
}

/// Hint appended to todo-tool output when a plan is recorded while the current
/// request is still unconfirmed.
pub(crate) fn plan_write_hint(session_id: &str) -> Option<&'static str> {
    plan_write_hint_with(feature_enabled(), session_id)
}

fn plan_write_hint_with(enabled: bool, session_id: &str) -> Option<&'static str> {
    if !enabled {
        return None;
    }
    let map = PLAN_GATE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = map.get(session_id)?;
    if state.confirmed_epoch >= state.epoch {
        return None;
    }
    Some(PLAN_WRITE_HINT)
}

fn is_mutating_tool(tool_name: &str) -> bool {
    MUTATING_TOOLS.contains(&tool_name)
}

const PLAN_WRITE_HINT: &str = "注意：把方案写进 todo 不等于拿到用户确认。本次请求尚未确认，\
写操作会被计划确认门禁拒绝——请先向用户展示方案并结束回合，等用户明确确认后再执行。";

fn block_message(tool_name: &str, epoch: u64) -> String {
    format!(
        "⛔ 计划确认门禁（plan-confirmation gate）：本次用户请求（#{epoch}）尚未获得确认，已拒绝执行「{tool_name}」。\n\
先向用户展示方案（目标与成功标准 / 改动点 / 影响面 / 验证方式 / 回滚方式），结束本回合等待用户确认；\
用户明确确认后（如回复「确认」「继续」「可以执行」，或在请求里说明「直接改，不用确认」）再重试本调用。\
不要重复重试，也不要借 swarm/batch/bg 等路径绕过门禁（同样受限）。\n\
Blocked by plan-confirmation gate: present the plan and wait for explicit user confirmation before mutating."
    )
}

/// Strip trailing filler/punctuation so "继续吧。"/"确认！！"/"ok~" normalize to
/// their plain forms. Only trailing characters are removed; the body is kept
/// intact so long sentences cannot collapse into a confirmation.
fn normalize_message(text: &str) -> String {
    let mut normalized = text.trim().to_lowercase();
    loop {
        let before = normalized.len();
        normalized = normalized
            .trim_end_matches([
                '。', '．', '.', '！', '!', '~', '～', '，', ',', '、', ' ', '　', '吧', '了',
                '哦', '呀', '啊', '哈', '呢',
            ])
            .to_string();
        if normalized.len() == before {
            break;
        }
    }
    normalized
}

/// Short, unambiguous acknowledgements that count as confirming a plan.
/// Deliberately conservative: anything longer or containing a revision reads
/// as feedback, not a confirmation.
const CONFIRM_EXACT: &[&str] = &[
    "确认",
    "同意",
    "批准",
    "通过",
    "可以",
    "可以执行",
    "可以继续",
    "可以开始",
    "可以可以",
    "去做",
    "做",
    "执行",
    "开始",
    "开工",
    "动工",
    "干",
    "继续",
    "请继续",
    "继续执行",
    "好的继续",
    "好继续",
    "好的，继续",
    "好，继续",
    "好的",
    "好",
    "行",
    "嗯",
    "没问题",
    "没有问题",
    "就这么办",
    "就按这个来",
    "就按这个做",
    "按这个来",
    "按这个做",
    "按你的方案来",
    "ok",
    "okay",
    "yes",
    "yep",
    "go",
    "go ahead",
    "proceed",
    "approved",
    "approve",
    "lgtm",
    "ship it",
    "do it",
    "just do it",
];

/// Prefixes that stay confirmations only when the whole message is tiny and
/// carries no revision marker. Length-gated so "确认，但第三点改一下" cannot
/// slip through.
const CONFIRM_PREFIX: &[&str] = &[
    "确认，",
    "确认：",
    "确认:",
    "同意，",
    "批准，",
    "可以执行，",
    "没问题，",
];
const CONFIRM_PREFIX_MAX_CHARS: usize = 7;
const CONFIRM_PREFIX_REJECT: &[&str] = &[
    "但", "不过", "改成", "换成", "先", "等", "不", "如果", "假如",
];

fn is_explicit_confirmation(text: &str) -> bool {
    let normalized = normalize_message(text);
    if CONFIRM_EXACT.contains(&normalized.as_str()) {
        return true;
    }
    for prefix in CONFIRM_PREFIX {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let total_chars = normalized.chars().count();
            if total_chars <= CONFIRM_PREFIX_MAX_CHARS
                && !CONFIRM_PREFIX_REJECT
                    .iter()
                    .any(|marker| rest.contains(marker))
            {
                return true;
            }
        }
    }
    false
}

/// Phrases in a request that authorize executing without a separate
/// confirmation step ("直接改，不用确认"). A negation guard keeps "别直接改"
/// from authorizing the opposite.
const AUTHORIZE_PHRASES: &[&str] = &[
    "不用确认",
    "无需确认",
    "不需要确认",
    "免确认",
    "跳过确认",
    "不用等我确认",
    "不用问我",
    "不用再问",
    "不用先问",
    "别再问",
    "别问了",
    "直接执行",
    "直接改",
    "直接做",
    "直接开始",
    "直接动手",
    "直接干",
    "直接推进",
    "just do it",
    "no need to confirm",
    "don't ask",
    "dont ask",
    "without confirmation",
];

fn is_explicit_authorization(text: &str) -> bool {
    let lowered = text.to_lowercase();
    // "别直接改" / "不要直接执行" contain an authorization phrase but mean the
    // opposite; refuse to auto-authorize when a negation is attached to it.
    if ["别直接", "不要直接", "不能直接", "不准直接"]
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return false;
    }
    AUTHORIZE_PHRASES
        .iter()
        .any(|phrase| lowered.contains(phrase))
}

#[cfg(test)]
fn snapshot(session_id: &str) -> Option<(u64, u64)> {
    let map = PLAN_GATE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.get(session_id)
        .map(|state| (state.epoch, state.confirmed_epoch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_patterns_accept_core_phrases() {
        for message in [
            "确认",
            "确认吧",
            "确认。",
            "确认！",
            "可以执行",
            "可以了",
            "继续",
            "继续吧。",
            "好的，继续",
            "去做",
            "开工",
            "同意",
            "批准",
            "没问题",
            "ok",
            "OK!",
            "go ahead",
            "approved",
            "lgtm",
            "就按这个来",
            "按你的方案来",
            "嗯",
        ] {
            assert!(
                is_explicit_confirmation(message),
                "should count as confirmation: {message}"
            );
        }
    }

    #[test]
    fn confirmation_patterns_reject_revisions_questions_and_negations() {
        for message in [
            "确认，但第三点改一下",
            "确认之后再说",
            "不确认",
            "别确认了",
            "继续，但是B改成C",
            "为什么",
            "这个方案看起来不错",
            "嗯……再说吧",
            "先别动",
            "帮我看看这个模块",
            "确认一下这个接口的返回值是不是 null?",
        ] {
            assert!(
                !is_explicit_confirmation(message),
                "should NOT count as confirmation: {message}"
            );
        }
    }

    #[test]
    fn authorization_patterns_accept_and_respect_negation() {
        for message in [
            "直接改吧，不用确认",
            "先做了，不用问我",
            "直接执行，无需确认",
            "just do it",
            "no need to confirm, go",
        ] {
            assert!(
                is_explicit_authorization(message),
                "should authorize: {message}"
            );
        }
        for message in ["别直接改", "不要直接执行", "不能直接动"] {
            assert!(
                !is_explicit_authorization(message),
                "negation guard should block: {message}"
            );
        }
    }

    #[test]
    fn gate_blocks_until_confirmed_and_resets_per_request() {
        let session = "plan-gate-unit-epochs";
        forget(session);

        assert!(
            check_with(true, session, "bash").is_none(),
            "untracked sessions are exempt"
        );

        begin_user_request(session, "帮我重构这个模块");
        assert!(
            check_with(true, session, "bash").is_some(),
            "a new request starts locked"
        );
        assert!(
            check_with(true, session, "read").is_none(),
            "read-only tools are exempt"
        );
        assert!(
            check_with(true, session, "todo").is_none(),
            "todo is planning, not mutating"
        );
        assert!(
            check_with(true, session, "batch").is_none(),
            "batch subcalls are checked individually"
        );
        assert!(
            check_with(false, session, "bash").is_none(),
            "feature off bypasses the gate"
        );

        begin_user_request(session, "确认");
        assert!(
            check_with(true, session, "bash").is_none(),
            "confirmation unlocks the current request"
        );

        begin_user_request(session, "再帮我看看别的地方");
        assert!(
            check_with(true, session, "write").is_some(),
            "a new request re-locks"
        );

        begin_user_request(session, "不用确认，直接改");
        assert!(
            check_with(true, session, "write").is_none(),
            "explicit authorization unlocks"
        );

        let before = snapshot(session).expect("state exists");
        begin_user_request(
            session,
            "You have 2 incomplete todos. Continue working, or update the todo tool.",
        );
        let after = snapshot(session).expect("state exists");
        assert_eq!(before, after, "auto-poke must not start a new epoch");
        assert!(
            check_with(true, session, "write").is_none(),
            "state survives auto-poke"
        );

        forget(session);
        assert!(
            check_with(true, session, "bash").is_none(),
            "forget clears the session state"
        );
    }

    #[test]
    fn plan_write_hint_only_when_unconfirmed() {
        let session = "plan-gate-unit-hint";
        forget(session);
        assert!(
            plan_write_hint_with(true, session).is_none(),
            "untracked sessions get no hint"
        );

        begin_user_request(session, "修一下这个接口");
        assert!(
            plan_write_hint_with(true, session).is_some(),
            "unconfirmed requests get the hint"
        );
        assert!(
            plan_write_hint_with(false, session).is_none(),
            "feature off suppresses the hint"
        );

        begin_user_request(session, "确认");
        assert!(
            plan_write_hint_with(true, session).is_none(),
            "confirmed requests get no hint"
        );
        forget(session);
    }

    #[test]
    fn mid_turn_confirmation_unlocks_without_new_epoch() {
        let session = "plan-gate-unit-midturn";
        forget(session);

        // Untracked sessions stay exempt even for confirmation-looking input.
        note_user_message(session, "确认");
        assert!(snapshot(session).is_none(), "no state is created");

        begin_user_request(session, "帮我改一下这个模块");
        assert_eq!(snapshot(session), Some((1, 0)));
        assert!(check_with(true, session, "write").is_some());

        // A mid-turn instruction is neither a confirmation nor a new request.
        note_user_message(session, "顺便看看日志");
        assert_eq!(snapshot(session), Some((1, 0)));
        assert!(check_with(true, session, "write").is_some());

        // A mid-turn confirmation unlocks the in-flight epoch, epoch unchanged.
        note_user_message(session, "确认");
        assert_eq!(snapshot(session), Some((1, 1)));
        assert!(check_with(true, session, "write").is_none());

        // Repeated confirmations and mid-turn authorization are idempotent.
        note_user_message(session, "不用确认，直接改");
        assert_eq!(snapshot(session), Some((1, 1)));
        forget(session);
    }

    #[test]
    fn media_only_requests_register_but_empty_continuations_do_not() {
        let session = "plan-gate-unit-media";
        forget(session);

        // Empty text without media is a transport continuation, not a request.
        begin_user_request_with_media(session, "   ", false);
        assert!(snapshot(session).is_none());

        // An image-only client message is still a user request: lock it.
        begin_user_request_with_media(session, "", true);
        assert_eq!(snapshot(session), Some((1, 0)));
        assert!(check_with(true, session, "write").is_some());

        forget(session);
    }

    #[test]
    fn mutating_tool_list_covers_execution_tools() {
        for tool in [
            "write",
            "edit",
            "multiedit",
            "patch",
            "apply_patch",
            "bash",
            "bg",
            "selfdev",
            "swarm",
            "skill_manage",
        ] {
            assert!(is_mutating_tool(tool), "{tool} must be gated");
        }
        for tool in [
            "read",
            "ls",
            "agentgrep",
            "webfetch",
            "websearch",
            "todo",
            "memory",
            "session_search",
            "open",
            "side_panel",
            "context_remaining",
            "initiative",
            "schedule",
            "batch",
        ] {
            assert!(!is_mutating_tool(tool), "{tool} must NOT be gated");
        }
    }
}
