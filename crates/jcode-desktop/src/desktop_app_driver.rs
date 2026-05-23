#![allow(dead_code)]

use crate::desktop_scene::DesktopScene;
use crate::session_launch;
use crate::workspace::KeyOutcome;

pub(crate) const DESKTOP_UI_SNAPSHOT_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopUiSnapshot {
    pub(crate) version: u16,
    pub(crate) mode: &'static str,
    pub(crate) title: String,
    pub(crate) live_session_id: Option<String>,
}

impl DesktopUiSnapshot {
    pub(crate) fn new(mode: &'static str, title: String, live_session_id: Option<String>) -> Self {
        Self {
            version: DESKTOP_UI_SNAPSHOT_VERSION,
            mode,
            title,
            live_session_id,
        }
    }
}

pub(crate) struct DesktopSceneBuildContext {
    pub(crate) scene: DesktopScene,
}

impl DesktopSceneBuildContext {
    pub(crate) fn new(scene: DesktopScene) -> Self {
        Self { scene }
    }
}

pub(crate) trait DesktopAppDriver {
    type KeyInput;
    type KeyOutcome;

    fn mode(&self) -> &'static str;
    fn status_title(&self) -> String;
    fn live_session_id(&self) -> Option<String>;
    fn has_background_work(&self) -> bool;
    fn has_frame_animation(&self) -> bool;
    fn handle_key_input(&mut self, key: Self::KeyInput) -> Self::KeyOutcome;
    fn apply_session_event(&mut self, event: session_launch::DesktopSessionEvent);
    fn build_scene(&self, context: DesktopSceneBuildContext) -> DesktopScene;
    fn snapshot(&self) -> DesktopUiSnapshot;
    fn restore_snapshot(
        &mut self,
        snapshot: DesktopUiSnapshot,
    ) -> Result<(), DesktopSnapshotRestoreError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSnapshotRestoreError {
    UnsupportedVersion { version: u16 },
    UnsupportedMode { mode: &'static str },
}

impl std::fmt::Display for DesktopSnapshotRestoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { version } => {
                write!(formatter, "unsupported desktop snapshot version {version}")
            }
            Self::UnsupportedMode { mode } => {
                write!(formatter, "cannot restore desktop snapshot for mode {mode}")
            }
        }
    }
}

impl std::error::Error for DesktopSnapshotRestoreError {}

pub(crate) type DesktopKeyDriverOutcome = KeyOutcome;
