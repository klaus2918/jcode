use crate::build;
use anyhow::{Context, Result};
use jcode_update_core::summarize_git_pull_failure;
pub use jcode_update_core::{
    DownloadProgress, GIT_PULL_DIVERGED_SUMMARY, format_download_progress_bar,
    summarize_update_error, summary_is_divergence,
};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[path = "update_metadata.rs"]
mod update_metadata;
pub use update_metadata::UpdateMetadata;

pub fn print_centered(msg: &str) {
    let msg = crate::output_style::terminal_text(msg);
    let width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    for line in msg.lines() {
        let visible_len = unicode_display_width(line);
        if visible_len >= width {
            println!("{}", line);
        } else {
            let pad = (width - visible_len) / 2;
            println!("{:>pad$}{}", "", line, pad = pad);
        }
    }
}

fn unicode_display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    let mut w = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        w += UnicodeWidthChar::width(c).unwrap_or(0);
    }
    w
}

pub fn run_git_pull_ff_only(repo_dir: &Path, quiet: bool) -> Result<()> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("pull").arg("--ff-only");
    if quiet {
        cmd.arg("-q");
    }
    let output = cmd
        .current_dir(repo_dir)
        .output()
        .context("Failed to run git pull")?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!("{}", summarize_git_pull_failure(&output.stderr));
    }
}

/// 从本地安装包（.tar.gz 或裸二进制 .exe）安装/更新 jcode，全程离线、不访问官网。
///
/// 走标准的安装管线（versions 归档 + stable/current 通道 + launcher 切换）。
pub fn install_local_artifact_blocking(
    artifact_path: &Path,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf> {
    let bytes = fs::read(artifact_path)
        .with_context(|| format!("Failed to read local artifact {}", artifact_path.display()))?;
    on_progress(DownloadProgress {
        downloaded: bytes.len() as u64,
        total: Some(bytes.len() as u64),
    });

    let (version, installed_version_dir) = if artifact_path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".tar.gz"))
    {
        let version = probe_tar_gz_version(&bytes)?;
        let installed = install_tar_gz_archive_blocking(&bytes, &version)?;
        (version, installed)
    } else {
        // 裸二进制（Windows .exe / Unix 可执行文件）：先复制到临时文件再安装。
        // install_binary_at_version 优先用硬链接，直接链接用户提供的安装包会让
        // 已安装版本与源文件共享 inode——用户之后在同一路径重建/覆盖安装包会
        // 污染已安装的版本目录。复制到临时文件后硬链接临时文件，装完删除临时
        // 文件，源文件与已安装版本彻底解耦。
        // 临时文件同时补上可执行权限，避免 Unix 上源 ELF 缺执行位导致 --version
        // 探测失败。
        let temp_path =
            std::env::temp_dir().join(format!("jcode-update-{}-local", std::process::id()));
        fs::copy(artifact_path, &temp_path).with_context(|| {
            format!("Failed to stage local artifact {}", artifact_path.display())
        })?;
        crate::platform::set_permissions_executable(&temp_path)?;
        let version = probe_binary_version(&temp_path)?;
        let installed = build::install_binary_at_version(&temp_path, &version);
        let _ = fs::remove_file(&temp_path);
        (version, installed?)
    };

    if let Err(error) = build::advance_shared_server_if_tracking_stable(&version) {
        crate::logging::warn(&format!(
            "update: failed to advance shared-server channel to {}: {}",
            version, error
        ));
    }
    build::update_stable_symlink(&version)?;
    build::update_current_symlink(&version)?;
    build::update_launcher_symlink_to_current()?;

    let mut metadata = UpdateMetadata::load().unwrap_or_default();
    metadata.installed_version = Some(version.clone());
    metadata.installed_from = Some(artifact_path.display().to_string());
    metadata.last_check = SystemTime::now();
    metadata.save()?;

    Ok(installed_version_dir)
}

/// 运行本地 jcode 二进制并解析其版本号（如 `0.64.2-dev`）。
fn probe_binary_version(bin: &Path) -> Result<String> {
    let output = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .with_context(|| format!("Failed to run local jcode binary {}", bin.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "Local jcode binary {} exited with {:?}",
            bin.display(),
            output.status.code()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_binary_version(&stdout).with_context(|| {
        format!(
            "Could not parse version from local binary {} output: {:?}",
            bin.display(),
            stdout.trim()
        )
    })
}

/// 从 `jcode --version` 输出解析版本号，兼容普通行（`jcode v0.64.2-dev (...)`
/// / `jcode 0.64.2-dev`）与 `jcode version` 的 tab 格式（`version\t0.64.2-dev`）。
fn parse_binary_version(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        let candidate = if let Some(rest) = line.strip_prefix("version\t") {
            rest.trim().to_string()
        } else if let Some(idx) = line.find("jcode ") {
            line[idx + "jcode ".len()..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('v')
                .to_string()
        } else {
            continue;
        };
        if is_semver_like(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// 宽松的 semver 形态检查：`数字.数字.数字开头` 即可（允许 `-dev` 等后缀）。
fn is_semver_like(value: &str) -> bool {
    let mut parts = value.split('.');
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts.next().and_then(|p| p.parse::<u32>().ok());
    let patch = parts.next();
    let patch_ok = patch.is_some_and(|p| {
        p.chars().next().is_some_and(|c| c.is_ascii_digit())
            // 整个 patch 段（含 -dev/-rc 后缀）白名单：字母、数字、`.`、`-`、`+`，
            // 防止 `0.64.2/../../x` 这类值被拼进 `builds/versions/{version}` 路径。
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    });
    major.is_some() && minor.is_some() && patch_ok
}

/// 从内存中的 tar.gz 发布包提取并安装到 `versions/<version>/`，返回版本化二进制路径。
///
/// 输入是发布包字节，输出是已安装的版本化二进制（launcher/通道切换由调用方负责）。
fn install_tar_gz_archive_blocking(archive_bytes: &[u8], version: &str) -> Result<PathBuf> {
    let temp_path = std::env::temp_dir().join(format!("jcode-update-{}", std::process::id()));
    let cursor = std::io::Cursor::new(archive_bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);
    let extract_dir = temp_path.with_extension("extract");
    if extract_dir.exists() {
        let _ = fs::remove_dir_all(&extract_dir);
    }
    fs::create_dir_all(&extract_dir).context("Failed to create archive extraction dir")?;
    let mut extracted_binary: Option<PathBuf> = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();
        if entry_path.components().count() != 1 {
            continue;
        }
        let file_name = entry_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.is_empty() || file_name.ends_with(".tar.gz") {
            continue;
        }
        let dest = extract_dir.join(&file_name);
        entry.unpack(&dest)?;
        if file_name.starts_with("jcode") && !file_name.ends_with(".bin") {
            extracted_binary = Some(dest);
        }
    }
    let Some(extracted_binary) = extracted_binary else {
        anyhow::bail!("Could not find jcode binary inside tar.gz archive");
    };
    crate::platform::set_permissions_executable(&extracted_binary)?;

    let dest_dir = build::builds_dir()?.join("versions").join(version);
    fs::create_dir_all(&dest_dir).context("Failed to create version install dir")?;
    let mut installed_files = Vec::new();
    for entry in fs::read_dir(&extract_dir).context("Failed to read extracted archive")? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name_string = name.to_string_lossy();
        // 本地安装包常带 git hash 后缀（如 jcode-windows-x86_64-2dc3213a6.exe）。
        // 凡是以 jcode 开头、不是 .bin payload 的顶层文件都视为主二进制，统一
        // 重命名为标准名。
        let dest_name = if name_string.starts_with("jcode") && !name_string.ends_with(".bin") {
            build::binary_name().to_string()
        } else {
            name_string.to_string()
        };
        let dest = dest_dir.join(dest_name);
        if dest.exists() {
            fs::remove_file(&dest)?;
        }
        fs::copy(entry.path(), &dest)
            .with_context(|| format!("Failed to install {}", dest.display()))?;
        if dest
            .file_name()
            .is_some_and(|name| name == build::binary_name())
            || dest.extension().is_some_and(|ext| ext == "bin")
        {
            crate::platform::set_permissions_executable(&dest)?;
        }
        installed_files.push(dest);
    }
    // Give every installed file the same mtime. The wrapper script and the
    // `.bin` payload otherwise land with whatever sub-second skew the copy
    // loop produced, and any code comparing binary freshness by mtime then
    // sees two "different age" files for one logical install.
    let install_stamp = SystemTime::now();
    for path in &installed_files {
        if let Ok(file) = fs::File::options().write(true).open(path) {
            let _ = file.set_modified(install_stamp);
        }
    }
    let _ = fs::remove_dir_all(&extract_dir);
    Ok(dest_dir.join(build::binary_name()))
}

/// 提取 tar.gz 到临时目录，运行其中二进制探测版本号，探测后清理临时目录。
fn probe_tar_gz_version(archive_bytes: &[u8]) -> Result<String> {
    let temp_path = std::env::temp_dir().join(format!("jcode-update-{}", std::process::id()));
    let extract_dir = temp_path.with_extension("probe");
    if extract_dir.exists() {
        let _ = fs::remove_dir_all(&extract_dir);
    }
    fs::create_dir_all(&extract_dir).context("Failed to create probe dir")?;
    let cursor = std::io::Cursor::new(archive_bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);
    let mut binary: Option<PathBuf> = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();
        if entry_path.components().count() != 1 {
            continue;
        }
        let file_name = entry_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.is_empty() || file_name.ends_with(".tar.gz") {
            continue;
        }
        let dest = extract_dir.join(&file_name);
        entry.unpack(&dest)?;
        if file_name.starts_with("jcode") && !file_name.ends_with(".bin") {
            binary = Some(dest);
        }
    }
    let Some(binary) = binary else {
        let _ = fs::remove_dir_all(&extract_dir);
        anyhow::bail!("Could not find jcode binary inside tar.gz archive");
    };
    crate::platform::set_permissions_executable(&binary)?;
    let result = probe_binary_version(&binary);
    let _ = fs::remove_dir_all(&extract_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_download_progress_bar_known_total() {
        let rendered = format_download_progress_bar(DownloadProgress {
            downloaded: 512,
            total: Some(1024),
        });
        assert!(rendered.contains("50%"));
        assert!(rendered.contains("512 B/1.0 KiB"));
        assert!(rendered.contains('█'));
        assert!(rendered.contains('░'));
    }

    #[test]
    fn test_format_download_progress_bar_unknown_total() {
        let rendered = format_download_progress_bar(DownloadProgress {
            downloaded: 2 * 1024 * 1024,
            total: None,
        });
        assert_eq!(rendered, "Downloading update... 2.0 MiB downloaded");
    }

    #[test]
    fn parse_binary_version_handles_plain_and_tab_formats() {
        // 普通行：`jcode v0.64.2-dev (2dc3213a6)`
        assert_eq!(
            parse_binary_version("jcode v0.64.2-dev (2dc3213a6)\n"),
            Some("0.64.2-dev".to_string())
        );
        // 普通行：无 v 前缀
        assert_eq!(
            parse_binary_version("jcode 0.64.2\n"),
            Some("0.64.2".to_string())
        );
        // `jcode version` 的 tab 格式：version\t0.64.2-dev
        assert_eq!(
            parse_binary_version("version\t0.64.2-dev\n"),
            Some("0.64.2-dev".to_string())
        );
        // 前置遥测提示行不应干扰解析
        assert_eq!(
            parse_binary_version("telemetry notice line\njcode v0.64.2 (abc1234)\n"),
            Some("0.64.2".to_string())
        );
        // 无版本信息
        assert_eq!(parse_binary_version("unrelated output\n"), None);
        assert_eq!(parse_binary_version(""), None);
    }

    #[test]
    fn is_semver_like_accepts_release_and_dev_suffixes() {
        assert!(is_semver_like("0.64.2"));
        assert!(is_semver_like("0.64.2-dev"));
        assert!(is_semver_like("1.2.3-rc.1"));
        assert!(!is_semver_like("main-2dc3213a6"));
        assert!(!is_semver_like(""));
        assert!(!is_semver_like("abc"));
        // 路径注入防护：patch 段不得含路径分隔符 `/`（会被拼进 builds/versions/{version}）
        assert!(!is_semver_like("0.64.2/../../x"));
        assert!(!is_semver_like("0.64.2-dev/.."));
        // 第 4+ 段被忽略（split('.') 的宽松语义），目录名含 `.` 无害
        assert!(is_semver_like("0.64.2.."));
    }

    #[test]
    fn test_summarize_git_pull_failure_diverged() {
        let stderr = b"hint: You have divergent branches and need to specify how to reconcile them.\nfatal: Need to specify how to reconcile divergent branches.\n";
        assert_eq!(
            summarize_git_pull_failure(stderr),
            jcode_update_core::GIT_PULL_DIVERGED_SUMMARY
        );
        assert!(jcode_update_core::summary_is_divergence(
            &summarize_git_pull_failure(stderr)
        ));
    }

    #[test]
    fn test_summarize_git_pull_failure_no_tracking_branch() {
        let stderr = b"There is no tracking information for the current branch.\n";
        assert_eq!(
            summarize_git_pull_failure(stderr),
            "git pull failed: current branch has no upstream tracking branch"
        );
    }

    #[test]
    fn test_summarize_git_pull_failure_uses_first_non_hint_line() {
        let stderr = b"hint: test hint\nfatal: repository not found\n";
        assert_eq!(
            summarize_git_pull_failure(stderr),
            "git pull failed: repository not found"
        );
    }
}
