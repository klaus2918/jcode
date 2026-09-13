//! Tab-bar model for the side panel: page classification, tab labels, the
//! horizontal tab layout, and the "changed while unfocused" badge bookkeeping
//! that backs the `•` marker on a tab.
//!
//! Why this lives here instead of in the protocol page type: the tab bar is a
//! presentation concern, and page kinds are already implied by the page-id
//! namespace in use across the codebase (`goals`, `goal.<id>`, `session_todos`,
//! `split_view`, `catchup`, `observe`, `image.<id>`, plus the free-form ids the
//! `side_panel` tool derives from file paths). Deriving the kind here keeps
//! `SidePanelPage` untouched, so every existing struct literal (about 45 across
//! the TUI, benches, and tests) keeps compiling and no persisted snapshot
//! changes shape.
//!
//! The layout is deliberately pure data: this module decides *which* tabs are
//! visible and *what text* each one shows; `ui_pinned` owns the styling.

use crate::side_panel::{SidePanelPage, SidePanelSnapshot};
use crossterm::event::{KeyCode, KeyModifiers};
use std::collections::{HashMap, HashSet};

/// Minimum label width (chars) a tab may be squeezed to before the bar starts
/// dropping tabs instead.
const MIN_LABEL_CHARS: usize = 3;
/// Columns reserved per hidden-run marker (`…+12` plus a trailing space).
const OVERFLOW_MARKER_COLS: usize = 5;
/// One leading and one trailing space around every label.
const LABEL_PADDING_COLS: usize = 2;
/// Cost of the separator between two adjacent tabs (`│`).
const SEPARATOR_COLS: usize = 1;
/// Below this width even a single squeezed tab plus markers cannot be drawn.
const MIN_BAR_WIDTH: usize = 4;

/// Built-in page ids owned by the TUI itself (not by a tool).
pub const PROGRESS_PAGE_ID: &str = "progress";
pub const REQ_MAP_PAGE_ID: &str = "req_map";

/// Where a side-panel page came from, derived from its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PageKind {
    /// Built-in progress page (`.op` change / tasks / goals / agents).
    Progress,
    /// Built-in requirement-to-implementation map page.
    ReqMap,
    /// The `goals` overview written by the `goal` tool.
    Goals,
    /// A single-goal detail page (`goal.<id>`).
    Goal,
    /// The session todo view (`session_todos`).
    Todos,
    /// Mirrored chat (`split_view`).
    SplitView,
    /// Post-history catch-up summary (`catchup`).
    Catchup,
    /// Observation page (`observe`).
    Observe,
    /// A generated image page (`image.<id>`).
    Image,
    /// Anything written by the `side_panel` tool (or an unknown id).
    Agent,
}

impl PageKind {
    /// Classify a page id. Unknown ids are agent/tool pages: the `side_panel`
    /// tool derives ids from free-form file paths, so there is no closed set.
    pub(super) fn from_id(id: &str) -> Self {
        if id == PROGRESS_PAGE_ID {
            return Self::Progress;
        }
        if id == REQ_MAP_PAGE_ID {
            return Self::ReqMap;
        }
        if id == "goals" {
            return Self::Goals;
        }
        if id.starts_with("goal.") {
            return Self::Goal;
        }
        if id == "session_todos" {
            return Self::Todos;
        }
        if id == "split_view" {
            return Self::SplitView;
        }
        if id == "catchup" {
            return Self::Catchup;
        }
        if id == "observe" {
            return Self::Observe;
        }
        if id.starts_with("image.") {
            return Self::Image;
        }
        Self::Agent
    }

    /// Tab-bar ordering weight: built-in information pages first, agent pages
    /// last. Ties keep the snapshot's insertion order (stable sort).
    pub(super) fn order(self) -> u8 {
        match self {
            Self::Progress => 0,
            Self::ReqMap => 1,
            Self::Goals => 2,
            Self::Goal => 3,
            Self::Todos => 4,
            Self::SplitView => 5,
            Self::Catchup => 6,
            Self::Observe => 7,
            Self::Image => 8,
            Self::Agent => 9,
        }
    }

    /// Short stable tag, used for the page-list filter and diagnostics. Titles
    /// are preferred for display; this is the machine-readable name.
    pub(super) fn tag(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::ReqMap => "req_map",
            Self::Goals => "goals",
            Self::Goal => "goal",
            Self::Todos => "todos",
            Self::SplitView => "split_view",
            Self::Catchup => "catchup",
            Self::Observe => "observe",
            Self::Image => "image",
            Self::Agent => "agent",
        }
    }
}

/// Tab indices in display order: stable sort by [`PageKind::order`], so pages of
/// the same kind keep the order the snapshot lists them in.
pub fn tab_order(pages: &[SidePanelPage]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..pages.len()).collect();
    indices.sort_by_key(|&index| PageKind::from_id(&pages[index].id).order());
    indices
}

/// Map a page id to its position in [`tab_order`], or `None` when absent.
pub fn tab_position(pages: &[SidePanelPage], page_id: &str) -> Option<usize> {
    tab_order(pages)
        .iter()
        .position(|&index| pages[index].id == page_id)
}

/// One tab, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TabItem {
    pub page_id: String,
    pub kind: PageKind,
    /// Full (whitespace-collapsed) title; truncation happens in the layout.
    pub title: String,
    pub focused: bool,
    pub updated: bool,
}

/// Build the tab list in display order.
pub(super) fn tab_items(
    snapshot: &SidePanelSnapshot,
    is_updated: impl Fn(&str) -> bool,
) -> Vec<TabItem> {
    let focused_id = snapshot.focused_page_id.as_deref();
    tab_order(&snapshot.pages)
        .into_iter()
        .map(|index| {
            let page = &snapshot.pages[index];
            TabItem {
                page_id: page.id.clone(),
                kind: PageKind::from_id(&page.id),
                title: collapse_whitespace(&page.title),
                focused: focused_id == Some(page.id.as_str()),
                updated: is_updated(&page.id),
            }
        })
        .collect()
}

/// A visible tab: the label after truncation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TabSlot {
    pub page_id: String,
    pub kind: PageKind,
    pub label: String,
    pub focused: bool,
    pub updated: bool,
}

/// Result of fitting tabs into a width.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct TabBarLayout {
    pub slots: Vec<TabSlot>,
    /// How many tabs were dropped off the left edge.
    pub leading_hidden: usize,
    /// How many tabs were dropped off the right edge.
    pub trailing_hidden: usize,
}

impl TabBarLayout {
    #[cfg(test)]
    pub(super) fn has_overflow(&self) -> bool {
        self.leading_hidden > 0 || self.trailing_hidden > 0
    }

    /// Plain-text rendering: labels joined by `│`, `•` prefix for tabs that
    /// changed while unfocused, and `…+N` markers for hidden runs. The live
    /// renderer builds styled spans instead; this is how the width tests and
    /// debug output express the same layout.
    #[cfg(test)]
    pub(super) fn plain_text(&self) -> String {
        // No visible tab means there is no anchor for an overflow marker, so
        // the bar renders nothing at all.
        if self.slots.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        if self.leading_hidden > 0 {
            out.push_str(&overflow_marker(self.leading_hidden));
            out.push(' ');
        }
        for (position, slot) in self.slots.iter().enumerate() {
            if position > 0 {
                out.push('│');
            }
            if slot.updated {
                out.push('•');
            }
            out.push_str(&slot.label);
        }
        if self.trailing_hidden > 0 {
            out.push(' ');
            out.push_str(&overflow_marker(self.trailing_hidden));
        }
        out
    }
}

#[cfg(test)]
fn overflow_marker(hidden: usize) -> String {
    format!("…+{hidden}")
}

/// Fit `items` into `width` columns.
///
/// Strategy, in order: show every tab at its natural width; then shrink every
/// label in step (uniformly, so no single tab looks arbitrarily clipped); then,
/// once every label is at [`MIN_LABEL_CHARS`], drop tabs from the edges,
/// alternating sides and always keeping the focused tab visible.
pub(super) fn layout_tab_bar(items: &[TabItem], width: usize) -> TabBarLayout {
    if items.is_empty() || width < MIN_BAR_WIDTH {
        return TabBarLayout {
            slots: Vec::new(),
            leading_hidden: items.len(),
            trailing_hidden: 0,
        };
    }

    let natural: Vec<usize> = items
        .iter()
        .map(|item| item.title.chars().count())
        .collect();
    let mut max_label = natural.iter().copied().max().unwrap_or(MIN_LABEL_CHARS);
    while max_label >= MIN_LABEL_CHARS {
        if bar_cost(&natural, max_label) <= width {
            return all_visible(items, &natural, max_label);
        }
        if max_label == MIN_LABEL_CHARS {
            break;
        }
        max_label -= 1;
    }

    window_layout(items, &natural, width)
}

/// Total columns used when every tab is shown with labels capped at `max_label`.
fn bar_cost(natural: &[usize], max_label: usize) -> usize {
    let labels: usize = natural
        .iter()
        .map(|&len| len.min(max_label).max(MIN_LABEL_CHARS))
        .sum();
    labels + LABEL_PADDING_COLS * natural.len() + SEPARATOR_COLS * natural.len().saturating_sub(1)
}

fn all_visible(items: &[TabItem], natural: &[usize], max_label: usize) -> TabBarLayout {
    let slots = items
        .iter()
        .zip(natural)
        .map(|(item, &len)| {
            slot_of(
                item,
                truncate_label(&item.title, len.min(max_label).max(MIN_LABEL_CHARS)),
            )
        })
        .collect();
    TabBarLayout {
        slots,
        leading_hidden: 0,
        trailing_hidden: 0,
    }
}

/// Every label is at its floor width and the bar still does not fit, so drop
/// whole tabs from the edges around the focused one.
fn window_layout(items: &[TabItem], natural: &[usize], width: usize) -> TabBarLayout {
    let focused = items.iter().position(|item| item.focused).unwrap_or(0);
    let labels: Vec<String> = items
        .iter()
        .zip(natural)
        .map(|(item, &len)| truncate_label(&item.title, len.max(MIN_LABEL_CHARS)))
        .collect();

    let cost_of = |lo: usize, hi: usize, hidden_left: bool, hidden_right: bool| -> usize {
        let mut total = LABEL_PADDING_COLS * (hi - lo + 1)
            + SEPARATOR_COLS * (hi - lo)
            + labels[lo..=hi]
                .iter()
                .map(|label| label.chars().count())
                .sum::<usize>();
        if hidden_left {
            total += OVERFLOW_MARKER_COLS + 1;
        }
        if hidden_right {
            total += OVERFLOW_MARKER_COLS + 1;
        }
        total
    };

    let (mut lo, mut hi) = (focused, focused);
    loop {
        let expand_left = lo > 0;
        let expand_right = hi + 1 < items.len();
        if !expand_left && !expand_right {
            break;
        }
        // Alternate sides, expanding whichever still has more tabs left so the
        // focused tab stays roughly centred in the visible run.
        let take_left = match (expand_left, expand_right) {
            (true, false) => true,
            (false, true) => false,
            _ => lo >= items.len() - 1 - hi,
        };
        let (next_lo, next_hi) = if take_left {
            (lo - 1, hi)
        } else {
            (lo, hi + 1)
        };
        if cost_of(next_lo, next_hi, next_lo > 0, next_hi + 1 < items.len()) > width {
            break;
        }
        lo = next_lo;
        hi = next_hi;
    }

    let slots = (lo..=hi)
        .map(|index| slot_of(&items[index], labels[index].clone()))
        .collect();
    TabBarLayout {
        slots,
        leading_hidden: lo,
        trailing_hidden: items.len() - 1 - hi,
    }
}

fn slot_of(item: &TabItem, label: String) -> TabSlot {
    TabSlot {
        page_id: item.page_id.clone(),
        kind: item.kind,
        label,
        focused: item.focused,
        updated: item.updated,
    }
}

/// Truncate to `max_chars` display characters, appending `…` when clipped.
fn truncate_label(title: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= max_chars {
        return title.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out: String = chars[..max_chars - 1].iter().collect();
    out.push('…');
    out
}

/// Tabs read better when a title's runs of whitespace are squeezed to one space.
fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for ch in value.trim().chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    out
}

/// Badge state: which pages changed while they were not the focused page.
///
/// A page gets a `•` when its content signature changes and it is not the
/// focused page. Brand-new pages never get a badge: pages appear as the result
/// of an action the user just triggered, so badging them is noise. Focusing a
/// page clears its badge.
#[derive(Debug, Default)]
pub struct SidePanelTabState {
    signatures: HashMap<String, u64>,
    updated: HashSet<String>,
    /// Set once a snapshot has been folded in at least once, so the first
    /// observation of a pre-existing page is not mistaken for a change.
    primed: bool,
}

impl SidePanelTabState {
    /// Fold a snapshot in and update badges. Cheap enough to call whenever the
    /// snapshot version changes: the change detector is `updated_at_ms` plus
    /// content length, not a content hash.
    pub(crate) fn sync(&mut self, snapshot: &SidePanelSnapshot) {
        let focused_id = snapshot.focused_page_id.as_deref();
        let mut seen: HashSet<&str> = HashSet::with_capacity(snapshot.pages.len());
        for page in &snapshot.pages {
            seen.insert(page.id.as_str());
            let signature = page_signature(page);
            match self.signatures.insert(page.id.clone(), signature) {
                Some(previous) if previous != signature => {
                    if focused_id != Some(page.id.as_str()) {
                        self.updated.insert(page.id.clone());
                    }
                }
                _ => {}
            }
        }
        if let Some(id) = focused_id {
            // The focused page is being read: drop its badge.
            self.updated.remove(id);
        }
        if self.primed {
            self.updated.retain(|id| seen.contains(id.as_str()));
        }
        self.signatures.retain(|id, _| seen.contains(id.as_str()));
        self.primed = true;
    }

    pub(crate) fn is_updated(&self, page_id: &str) -> bool {
        self.updated.contains(page_id)
    }

    #[cfg(test)]
    pub(crate) fn updated_count(&self) -> usize {
        self.updated.len()
    }

    pub(crate) fn clear_badge(&mut self, page_id: &str) {
        self.updated.remove(page_id);
    }

    #[cfg(test)]
    pub(crate) fn mark_updated_for_tests(&mut self, page_id: &str) {
        self.updated.insert(page_id.to_string());
    }
}

/// Cheap change detector for a page: millisecond timestamp plus content length.
/// A same-millisecond edit that keeps the length is not observable — acceptable
/// for a `•` hint, and far cheaper than hashing every page every frame.
fn page_signature(page: &SidePanelPage) -> u64 {
    page.updated_at_ms
        .wrapping_mul(31)
        .wrapping_add(page.content.len() as u64)
}

/// One row of the page list overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerRow {
    pub page_id: String,
    pub title: String,
    /// Machine-readable source tag (`progress`, `goal`, `agent`, ...).
    pub tag: &'static str,
    pub focused: bool,
}

/// Rows to draw in the page list overlay: the picker's matches, in picker
/// order, already reduced to what the renderer needs.
pub fn picker_rows(
    pages: &[SidePanelPage],
    picker: &PagePickerState,
    focused_page_id: Option<&str>,
) -> Vec<PickerRow> {
    picker
        .matches(pages)
        .into_iter()
        .map(|index| {
            let page = &pages[index];
            PickerRow {
                page_id: page.id.clone(),
                title: page.title.clone(),
                tag: PageKind::from_id(&page.id).tag(),
                focused: focused_page_id == Some(page.id.as_str()),
            }
        })
        .collect()
}

/// What one keystroke means while the page list (`Alt+L`) is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePickerAction {
    /// Key not used by the picker; the overlay stays open and does nothing.
    Ignored,
    Close,
    /// Focus the selected page and close.
    Commit,
    /// Move the selection by `delta` rows (wrapping).
    Move(isize),
    Push(char),
    Pop,
    ClearQuery,
}

/// Incremental-filter state for the page list overlay, plus the query it holds.
///
/// Lives next to the tab model because "which pages exist and in what order" is
/// exactly what the tab bar and the picker must agree on.
#[derive(Debug, Default, Clone)]
pub struct PagePickerState {
    query: String,
    selected: usize,
}

impl PagePickerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Append a filter character. Real characters reset the selection to the
    /// best match.
    pub fn push_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.query.push(ch);
        self.selected = 0;
    }

    pub fn pop_char(&mut self) {
        if self.query.pop().is_some() {
            self.selected = 0;
        }
    }

    pub fn clear_query(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// Display-order indices matching the query, best score first. An empty
    /// query lists every tab in bar order, so the picker doubles as a plain
    /// "what pages do I have" list.
    ///
    /// Matching reuses the crate's typo-tolerant slash-command matcher
    /// (`crate::tui::fuzzy`), so `req` finds "Requirements", `splt` finds
    /// "Split View", and single typos still match.
    pub fn matches(&self, pages: &[SidePanelPage]) -> Vec<usize> {
        let order = tab_order(pages);
        let needle = self.query.trim();
        if needle.is_empty() {
            return order;
        }
        let mut scored: Vec<(i32, usize)> = order
            .into_iter()
            .filter_map(|index| {
                let page = &pages[index];
                let kind = PageKind::from_id(&page.id);
                [
                    crate::tui::fuzzy::fuzzy_score(needle, &page.title),
                    crate::tui::fuzzy::fuzzy_score(needle, &page.id),
                    crate::tui::fuzzy::fuzzy_score(needle, kind.tag()),
                ]
                .into_iter()
                .flatten()
                .max()
                .map(|score| (score, index))
            })
            .collect();
        // Best score first; ties keep tab-bar order (stable by index).
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, index)| index).collect()
    }

    /// Move the selection within `match_count` rows, wrapping.
    pub fn move_selection(&mut self, delta: isize, match_count: usize) {
        if match_count == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected.min(match_count - 1) as isize;
        self.selected = (current + delta).rem_euclid(match_count as isize) as usize;
    }

    /// Selected row, clamped to the current match list. `None` when nothing
    /// matches.
    pub fn selected_row(&self, match_count: usize) -> Option<usize> {
        if match_count == 0 {
            return None;
        }
        Some(self.selected.min(match_count - 1))
    }
}

/// Map a key to a picker action. Pure so the key map is unit-testable without
/// an `App`.
pub fn page_picker_action(
    code: KeyCode,
    modifiers: KeyModifiers,
    match_count: usize,
) -> PagePickerAction {
    let control = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    match code {
        KeyCode::Esc => PagePickerAction::Close,
        KeyCode::Char('c') if control => PagePickerAction::Close,
        KeyCode::Enter => PagePickerAction::Commit,
        KeyCode::Up | KeyCode::BackTab => PagePickerAction::Move(-1),
        KeyCode::Down | KeyCode::Tab => PagePickerAction::Move(1),
        KeyCode::Char('p') if control => PagePickerAction::Move(-1),
        KeyCode::Char('n') if control => PagePickerAction::Move(1),
        KeyCode::PageUp => PagePickerAction::Move(-5),
        KeyCode::PageDown => PagePickerAction::Move(5),
        KeyCode::Home => PagePickerAction::Move(-(match_count as isize)),
        KeyCode::End => PagePickerAction::Move(match_count as isize),
        KeyCode::Backspace => PagePickerAction::Pop,
        KeyCode::Delete => PagePickerAction::ClearQuery,
        KeyCode::Char(_) if control || alt => PagePickerAction::Ignored,
        KeyCode::Char(ch) => PagePickerAction::Push(ch),
        _ => PagePickerAction::Ignored,
    }
}

#[cfg(test)]
#[path = "side_panel_tabs_tests.rs"]
mod tests;
