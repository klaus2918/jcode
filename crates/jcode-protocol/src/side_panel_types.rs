//! Side-panel page types (merged from the removed jcode-side-panel-types
//! crate, feature-simplification #31 / M-1, 2026-08-16). Hosted in jcode-protocol
//! because jcode-base already depends on jcode-protocol; re-exported by
//! jcode-base::side_panel for the TUI/tool layer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidePanelPageFormat {
    #[default]
    Markdown,
}

impl SidePanelPageFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidePanelPageSource {
    #[default]
    Managed,
    LinkedFile,
    Ephemeral,
}

impl SidePanelPageSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::LinkedFile => "linked_file",
            Self::Ephemeral => "ephemeral",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedSidePanelState {
    #[serde(default)]
    pub focused_page_id: Option<String>,
    #[serde(default)]
    pub pages: Vec<PersistedSidePanelPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSidePanelPage {
    pub id: String,
    pub title: String,
    pub file_path: String,
    #[serde(default)]
    pub format: SidePanelPageFormat,
    #[serde(default)]
    pub source: SidePanelPageSource,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SidePanelPage {
    pub id: String,
    pub title: String,
    pub file_path: String,
    #[serde(default)]
    pub format: SidePanelPageFormat,
    #[serde(default)]
    pub source: SidePanelPageSource,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SidePanelSnapshot {
    #[serde(default)]
    pub focused_page_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<SidePanelPage>,
}

impl SidePanelSnapshot {
    pub fn has_pages(&self) -> bool {
        !self.pages.is_empty()
    }

    pub fn focused_page(&self) -> Option<&SidePanelPage> {
        let focused_id = self.focused_page_id.as_deref()?;
        self.pages.iter().find(|page| page.id == focused_id)
    }
}

pub fn snapshot_is_empty(snapshot: &SidePanelSnapshot) -> bool {
    !snapshot.has_pages()
}
