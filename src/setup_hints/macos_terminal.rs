use crate::storage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MacTerminalKind {
    Ghostty,
    Iterm2,
    AppleTerminal,
    WezTerm,
    Warp,
    Alacritty,
    Vscode,
    Unknown,
}

impl MacTerminalKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Ghostty => "Ghostty",
            Self::Iterm2 => "iTerm2",
            Self::AppleTerminal => "Terminal.app",
            Self::WezTerm => "WezTerm",
            Self::Warp => "Warp",
            Self::Alacritty => "Alacritty",
            Self::Vscode => "VS Code terminal",
            Self::Unknown => "your current terminal",
        }
    }

    pub(super) fn cli_value(self) -> &'static str {
        match self {
            Self::Ghostty => "ghostty",
            Self::Iterm2 => "iterm2",
            Self::AppleTerminal => "terminal",
            Self::WezTerm => "wezterm",
            Self::Warp => "warp",
            Self::Alacritty => "alacritty",
            Self::Vscode => "vscode",
            Self::Unknown => "terminal",
        }
    }

    fn from_cli_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ghostty" => Some(Self::Ghostty),
            "iterm2" | "iterm" => Some(Self::Iterm2),
            "terminal" | "terminal.app" | "apple_terminal" => Some(Self::AppleTerminal),
            "wezterm" => Some(Self::WezTerm),
            "warp" => Some(Self::Warp),
            "alacritty" => Some(Self::Alacritty),
            "vscode" | "code" => Some(Self::Vscode),
            _ => None,
        }
    }
}

impl fmt::Display for MacTerminalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MacTerminalPreference {
    terminal: String,
}

fn mac_terminal_pref_path() -> Result<PathBuf> {
    Ok(storage::jcode_dir()?.join("preferred_terminal.json"))
}

fn load_preferred_macos_terminal() -> Option<MacTerminalKind> {
    let path = mac_terminal_pref_path().ok()?;
    let pref: MacTerminalPreference = storage::read_json(&path).ok()?;
    MacTerminalKind::from_cli_value(&pref.terminal)
}

pub(super) fn save_preferred_macos_terminal(terminal: MacTerminalKind) -> Result<()> {
    let path = mac_terminal_pref_path()?;
    storage::write_json(
        &path,
        &MacTerminalPreference {
            terminal: terminal.cli_value().to_string(),
        },
    )
}

pub(super) fn effective_macos_terminal() -> MacTerminalKind {
    load_preferred_macos_terminal().unwrap_or_else(detect_macos_terminal)
}

fn detect_macos_terminal() -> MacTerminalKind {
    let term_program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_lowercase();
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();

    if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok()
        || std::env::var("GHOSTTY_BIN_DIR").is_ok()
        || term_program == "ghostty"
        || term.contains("ghostty")
    {
        return MacTerminalKind::Ghostty;
    }

    match term_program.as_str() {
        "iterm.app" => MacTerminalKind::Iterm2,
        "apple_terminal" => MacTerminalKind::AppleTerminal,
        "wezterm" => MacTerminalKind::WezTerm,
        "vscode" => MacTerminalKind::Vscode,
        _ => {
            if term.contains("alacritty") {
                MacTerminalKind::Alacritty
            } else if term.contains("warp") {
                MacTerminalKind::Warp
            } else {
                MacTerminalKind::Unknown
            }
        }
    }
}

pub(super) fn escape_shell_single_quotes(input: &str) -> String {
    input.replace('\'', r#"'\''"#)
}

pub(super) fn escape_applescript_text(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn paused_jcode_shell_command(exe_path: &str) -> String {
    let escaped_exe = escape_shell_single_quotes(exe_path);
    format!(
        "if [ ! -x '{exe}' ]; then printf 'jcode executable not found.\\n'; exit 127; fi; '{exe}'; status=$?; if [ \"$status\" -ne 0 ]; then printf '\\nJcode exited with status %s.\\n' \"$status\"; printf 'Press Enter to close... '; read -r _; fi; exit \"$status\"",
        exe = escaped_exe,
    )
}

pub(super) fn launch_command_for_macos_terminal(
    terminal: MacTerminalKind,
    shell_command: &str,
) -> String {
    let escaped_shell = escape_shell_single_quotes(shell_command);
    match terminal {
        MacTerminalKind::Ghostty => {
            format!("/usr/bin/open -na Ghostty --args -e /bin/bash -lc '{escaped_shell}'")
        }
        MacTerminalKind::Alacritty => {
            format!("/usr/bin/open -na Alacritty --args -e /bin/bash -lc '{escaped_shell}'")
        }
        MacTerminalKind::WezTerm => format!(
            "/usr/bin/open -na WezTerm --args start --always-new-process -- /bin/bash -lc '{escaped_shell}'"
        ),
        MacTerminalKind::Iterm2 => format!(
            "/usr/bin/osascript <<'APPLESCRIPT'\ntell application \"iTerm2\"\n    create window with default profile command \"{}\"\n    activate\nend tell\nAPPLESCRIPT",
            escape_applescript_text(shell_command)
        ),
        MacTerminalKind::AppleTerminal
        | MacTerminalKind::Warp
        | MacTerminalKind::Vscode
        | MacTerminalKind::Unknown => format!(
            "/usr/bin/osascript <<'APPLESCRIPT'\ntell application \"Terminal\"\n    activate\n    do script \"{}\"\nend tell\nAPPLESCRIPT",
            escape_applescript_text(shell_command)
        ),
    }
}

#[cfg(target_os = "macos")]
pub(super) fn launch_script_for_macos_terminal(
    terminal: MacTerminalKind,
    shell_command: &str,
) -> String {
    format!(
        "#!/bin/bash\nset -e\n{}\n",
        launch_command_for_macos_terminal(terminal, shell_command)
    )
}
