use crate::tui::info_widget::GitInfo;
use std::path::Path;
use std::time::Duration;

/// Copy text to clipboard, trying wl-copy first (Wayland), then arboard as fallback.
pub(super) fn copy_to_clipboard(text: &str) -> bool {
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        use std::io::Write;
        if let Some(stdin) = child.stdin.as_mut() {
            if stdin.write_all(text.as_bytes()).is_ok() {
                drop(child.stdin.take());
                return child.wait().map(|s| s.success()).unwrap_or(false);
            }
        }
    }
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .is_ok()
}

pub(super) fn effort_display_label(effort: &str) -> &str {
    match effort {
        "xhigh" => "Max",
        "high" => "High",
        "medium" => "Medium",
        "low" => "Low",
        "none" => "None",
        other => other,
    }
}

pub(super) fn effort_bar(index: usize, total: usize) -> String {
    let mut bar = String::new();
    for i in 0..total {
        if i == index {
            bar.push('●');
        } else {
            bar.push('○');
        }
    }
    bar
}

pub(super) fn mask_email(email: &str) -> String {
    let trimmed = email.trim();
    let Some((local, domain)) = trimmed.split_once('@') else {
        return trimmed.to_string();
    };

    if local.is_empty() {
        return format!("***@{}", domain);
    }

    let mut chars = local.chars();
    let first = chars.next().unwrap_or('*');
    let last = chars.last().unwrap_or(first);

    let masked_local = if local.chars().count() <= 2 {
        format!("{}*", first)
    } else {
        format!("{}***{}", first, last)
    };

    format!("{}@{}", masked_local, domain)
}

/// Spawn a new terminal window that resumes a jcode session.
/// Returns Ok(true) if a terminal was successfully launched, Ok(false) if no terminal found.
#[cfg(unix)]
pub(super) fn spawn_in_new_terminal(
    exe: &Path,
    session_id: &str,
    cwd: &Path,
    socket: Option<&str>,
) -> anyhow::Result<bool> {
    use std::process::{Command, Stdio};

    let mut candidates: Vec<String> = Vec::new();
    if let Ok(term) = std::env::var("JCODE_TERMINAL") {
        if !term.trim().is_empty() {
            candidates.push(term);
        }
    }
    candidates.extend(
        [
            "kitty",
            "wezterm",
            "alacritty",
            "gnome-terminal",
            "konsole",
            "xterm",
            "foot",
        ]
        .iter()
        .map(|s| s.to_string()),
    );

    for term in candidates {
        let mut cmd = Command::new(&term);
        cmd.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match term.as_str() {
            "kitty" => {
                cmd.args(["--title", "jcode split", "-e"])
                    .arg(exe)
                    .arg("--resume")
                    .arg(session_id);
                if let Some(socket) = socket {
                    cmd.arg("--socket").arg(socket);
                }
            }
            "wezterm" => {
                cmd.args([
                    "start",
                    "--always-new-process",
                    "--",
                    exe.to_string_lossy().as_ref(),
                    "--resume",
                    session_id,
                ]);
            }
            "alacritty" => {
                cmd.args(["-e"]).arg(exe).arg("--resume").arg(session_id);
            }
            "gnome-terminal" => {
                cmd.args(["--", exe.to_string_lossy().as_ref(), "--resume", session_id]);
            }
            "konsole" => {
                cmd.args(["-e"]).arg(exe).arg("--resume").arg(session_id);
            }
            "xterm" => {
                cmd.args(["-e"]).arg(exe).arg("--resume").arg(session_id);
            }
            "foot" => {
                cmd.args(["-e"]).arg(exe).arg("--resume").arg(session_id);
            }
            _ => continue,
        }

        if cmd.spawn().is_ok() {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(not(unix))]
pub(super) fn spawn_in_new_terminal(
    _exe: &Path,
    _session_id: &str,
    _cwd: &Path,
    _socket: Option<&str>,
) -> anyhow::Result<bool> {
    Ok(false)
}

/// Try to get an image from the system clipboard.
///
/// Returns `Some((media_type, base64_data))` if an image is available.
/// Uses `wl-paste` on Wayland, `osascript` on macOS, falls back to `arboard::get_image()`.
pub(super) fn clipboard_image() -> Option<(String, String)> {
    use base64::Engine;

    // Try wl-paste first (native Wayland - better image format support)
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        if let Ok(output) = std::process::Command::new("wl-paste")
            .arg("--list-types")
            .output()
        {
            let types = String::from_utf8_lossy(&output.stdout);
            crate::logging::info(&format!(
                "clipboard_image: wl-paste types: {:?}",
                types.trim()
            ));
            let (mime, wl_type) = if types.lines().any(|t| t.trim() == "image/png") {
                ("image/png", "image/png")
            } else if types.lines().any(|t| t.trim() == "image/jpeg") {
                ("image/jpeg", "image/jpeg")
            } else if types.lines().any(|t| t.trim() == "image/webp") {
                ("image/webp", "image/webp")
            } else if types.lines().any(|t| t.trim() == "image/gif") {
                ("image/gif", "image/gif")
            } else {
                ("", "")
            };

            if !mime.is_empty() {
                if let Ok(img_output) = std::process::Command::new("wl-paste")
                    .args(["--type", wl_type, "--no-newline"])
                    .output()
                {
                    if img_output.status.success() && !img_output.stdout.is_empty() {
                        let b64 =
                            base64::engine::general_purpose::STANDARD.encode(&img_output.stdout);
                        return Some((mime.to_string(), b64));
                    }
                }
            }

            // Fallback: check text/html for <img> tags (Discord copies HTML with image URLs)
            if types.lines().any(|t| t.trim() == "text/html") {
                if let Ok(html_output) = std::process::Command::new("wl-paste")
                    .args(["--type", "text/html"])
                    .output()
                {
                    if html_output.status.success() && !html_output.stdout.is_empty() {
                        let html = String::from_utf8_lossy(&html_output.stdout);
                        crate::logging::info(&format!(
                            "clipboard_image: checking HTML for img tags ({} bytes)",
                            html.len()
                        ));
                        if let Some(url) = extract_image_url(&html) {
                            crate::logging::info(&format!(
                                "clipboard_image: found image URL in HTML: {}",
                                &url[..url.len().min(80)]
                            ));
                            if let Some(result) = download_image_url(&url) {
                                return Some(result);
                            }
                        }
                    }
                }
            }
        }
    }

    // macOS: use osascript to check clipboard for images and save as PNG via temp file
    #[cfg(target_os = "macos")]
    {
        let temp_path = std::env::temp_dir().join("jcode_clipboard.png");
        let script = format!(
            r#"use framework \"AppKit\"
            set pb to current application's NSPasteboard's generalPasteboard()
            set imgClasses to current application's NSArray's arrayWithObject:(current application's NSImage)
            if (pb's canReadObjectForClasses:imgClasses options:(missing value)) then
                set imgList to pb's readObjectsForClasses:imgClasses options:(missing value)
                set img to item 1 of imgList
                set tiffData to img's TIFFRepresentation()
                set bitmapRep to current application's NSBitmapImageRep's imageRepWithData:tiffData
                set pngData to bitmapRep's representationUsingType:(current application's NSBitmapImageFileTypePNG) properties:(missing value)
                pngData's writeToFile:\"{}\" atomically:true
                return \"ok\"
            else
                return \"none\"
            end if"#,
            temp_path.to_string_lossy()
        );
        if let Ok(output) = std::process::Command::new("osascript")
            .args(["-l", "AppleScript", "-e", &script])
            .output()
        {
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if result == "ok" {
                if let Ok(data) = std::fs::read(&temp_path) {
                    let _ = std::fs::remove_file(&temp_path);
                    if !data.is_empty() {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        return Some(("image/png".to_string(), b64));
                    }
                }
            }
        }
    }

    // Fallback: arboard (works on X11/XWayland and macOS via NSPasteboard)
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(img) = clipboard.get_image() {
            // img.bytes is RGBA pixel data - encode as PNG
            if let Some(png_data) = encode_rgba_as_png(img.width, img.height, &img.bytes) {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
                return Some(("image/png".to_string(), b64));
            }
        }
    }

    None
}

/// Extract an image URL from text that looks like an HTML img tag or a bare image URL.
/// Returns the URL if found.
pub(super) fn extract_image_url(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // Check for <img src="..."> pattern (Discord web copies)
    if let Some(start) = trimmed.find("<img") {
        if let Some(src_start) = trimmed[start..].find("src=\"") {
            let url_start = start + src_start + 5;
            if let Some(url_end) = trimmed[url_start..].find('"') {
                let url = &trimmed[url_start..url_start + url_end];
                if url.starts_with("http") {
                    return Some(url.to_string());
                }
            }
        }
        if let Some(src_start) = trimmed[start..].find("src='") {
            let url_start = start + src_start + 5;
            if let Some(url_end) = trimmed[url_start..].find('\'') {
                let url = &trimmed[url_start..url_start + url_end];
                if url.starts_with("http") {
                    return Some(url.to_string());
                }
            }
        }
    }

    // Check for bare image URL
    if trimmed.starts_with("http")
        && (trimmed.contains(".png")
            || trimmed.contains(".jpg")
            || trimmed.contains(".jpeg")
            || trimmed.contains(".gif")
            || trimmed.contains(".webp"))
    {
        // Strip query params for extension check but return full URL
        return Some(trimmed.to_string());
    }

    None
}

/// Download an image from a URL and return (media_type, base64_data).
/// Uses curl for simplicity (available on all platforms).
pub(super) fn download_image_url(url: &str) -> Option<(String, String)> {
    use base64::Engine;

    let output = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "10", "--max-filesize", "10000000", url])
        .output()
        .ok()?;

    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }

    // Detect image type from magic bytes
    let data = &output.stdout;
    let media_type = if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    Some((media_type.to_string(), b64))
}

/// Encode raw RGBA pixel data as PNG bytes.
pub(super) fn encode_rgba_as_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    use image::{ImageBuffer, RgbaImage};
    use std::io::Cursor;

    let img: RgbaImage = ImageBuffer::from_raw(width as u32, height as u32, rgba.to_vec())?;
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    Some(buf)
}

pub(super) fn gather_git_info() -> Option<GitInfo> {
    use std::sync::Mutex;
    use std::time::Instant;

    static CACHE: Mutex<Option<(Instant, Option<GitInfo>)>> = Mutex::new(None);

    const TTL: Duration = Duration::from_secs(5);

    if let Ok(guard) = CACHE.lock() {
        if let Some((ts, ref cached)) = *guard {
            if ts.elapsed() < TTL {
                return cached.clone();
            }
        }
    }

    let result = gather_git_info_inner();

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), result.clone()));
    }

    result
}

fn gather_git_info_inner() -> Option<GitInfo> {
    use std::process::Command;

    let in_repo = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !in_repo {
        return None;
    }

    let branch = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let b = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if b.is_empty() { None } else { Some(b) }
            } else {
                None
            }
        })
        .unwrap_or_else(|| "HEAD".to_string());

    let mut modified = 0;
    let mut staged = 0;
    let mut untracked = 0;
    let mut dirty_files = Vec::new();

    if let Ok(output) = Command::new("git").args(["status", "--porcelain"]).output() {
        if output.status.success() {
            let status = String::from_utf8_lossy(&output.stdout);
            for line in status.lines() {
                if line.len() < 3 {
                    continue;
                }
                let index_status = line.as_bytes()[0];
                let worktree_status = line.as_bytes()[1];
                let file_path = line[3..].to_string();

                if index_status == b'?' {
                    untracked += 1;
                } else {
                    if index_status != b' ' && index_status != b'?' {
                        staged += 1;
                    }
                    if worktree_status != b' ' && worktree_status != b'?' {
                        modified += 1;
                    }
                }

                if dirty_files.len() < 10 {
                    dirty_files.push(file_path);
                }
            }
        }
    }

    let (ahead, behind) = Command::new("git")
        .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let parts: Vec<&str> = text.split('\t').collect();
                if parts.len() == 2 {
                    let a = parts[0].parse::<usize>().unwrap_or(0);
                    let b = parts[1].parse::<usize>().unwrap_or(0);
                    Some((a, b))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or((0, 0));

    Some(GitInfo {
        branch,
        modified,
        staged,
        untracked,
        ahead,
        behind,
        dirty_files,
    })
}
