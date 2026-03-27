use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = pkg_version.split('.').collect();
    let major = parts.first().unwrap_or(&"0");
    let minor = parts.get(1).unwrap_or(&"0");

    let build_number = increment_build_number(major, minor);

    let git_hash = env_or_metadata_or_git(
        "JCODE_BUILD_GIT_HASH",
        "git_hash",
        ["rev-parse", "--short", "HEAD"],
    )
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "unknown".to_string());

    // Get git commit date (full datetime with timezone for accurate age calculation)
    let git_date = env_or_metadata_or_git(
        "JCODE_BUILD_GIT_DATE",
        "git_date",
        ["log", "-1", "--format=%ci"],
    )
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "unknown".to_string());

    let dirty = match std::env::var("JCODE_BUILD_GIT_DIRTY") {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "dirty"
        ),
        Err(_) => metadata_value("git_dirty")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "dirty"
                )
            })
            .or_else(|| git_output(["status", "--porcelain"]).map(|output| !output.is_empty()))
            .unwrap_or(false),
    };

    // Get git tag (e.g., "v0.1.2" if HEAD is tagged, or "v0.1.2-3-gabc1234" if ahead)
    let git_tag = env_or_metadata_or_git(
        "JCODE_BUILD_GIT_TAG",
        "git_tag",
        ["describe", "--tags", "--always"],
    )
    .unwrap_or_default();

    // Get recent commit messages with commit timestamps and version tag decorations.
    // Format: "hash|timestamp|decorations|subject" per line.
    // We embed a deeper window so /changelog can cover many more releases.
    let raw_log = std::env::var("JCODE_BUILD_CHANGELOG_RAW")
        .ok()
        .or_else(|| metadata_value("changelog_raw"))
        .or_else(|| git_output(["log", "-700", "--format=%h|%ct|%D|%s"]))
        .unwrap_or_default();

    // Normalize to "hash<RS>tag<RS>timestamp<RS>subject" — extract version tag or
    // leave empty. We use ASCII record/unit separators so fields can safely
    // contain punctuation.
    let changelog = raw_log
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '|');
            let hash = parts.next()?;
            let timestamp = parts.next().unwrap_or("");
            let decorations = parts.next().unwrap_or("");
            let subject = parts.next()?;
            let tag = decorations
                .split(',')
                .map(|d| d.trim())
                .find(|d| d.starts_with("tag: v"))
                .and_then(|d| d.strip_prefix("tag: "))
                .unwrap_or("");
            Some(format!(
                "{}\x1e{}\x1e{}\x1e{}",
                hash, tag, timestamp, subject
            ))
        })
        .collect::<Vec<_>>()
        .join("\x1f");

    // Build version string:
    //   Release: v0.2.0 (abc1234)
    //   Dev:     v0.2.5 (abc1234)
    //   Dirty:   v0.2.5-dirty (abc1234)
    let is_release = std::env::var("JCODE_RELEASE_BUILD").is_ok();
    let patch = parts.get(2).unwrap_or(&"0");
    let version = if is_release {
        format!("v{}.{}.{} ({})", major, minor, patch, git_hash)
    } else if dirty {
        format!("v{}.{}.{}-dirty ({})", major, minor, build_number, git_hash)
    } else {
        format!("v{}.{}.{} ({})", major, minor, build_number, git_hash)
    };

    // Get actual build timestamp
    let build_time = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S %z")
        .to_string();

    // Set environment variables for compilation
    println!("cargo:rustc-env=JCODE_GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=JCODE_GIT_DATE={}", git_date);
    println!("cargo:rustc-env=JCODE_BUILD_TIME={}", build_time);
    println!("cargo:rustc-env=JCODE_VERSION={}", version);
    println!("cargo:rustc-env=JCODE_BUILD_NUMBER={}", build_number);
    println!("cargo:rustc-env=JCODE_GIT_TAG={}", git_tag);
    println!("cargo:rustc-env=JCODE_CHANGELOG={}", changelog);

    // Forward JCODE_RELEASE_BUILD env var if set (CI sets this for release binaries)
    if std::env::var("JCODE_RELEASE_BUILD").is_ok() {
        println!("cargo:rustc-env=JCODE_RELEASE_BUILD=1");
    }

    // Re-run if git HEAD changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-env-changed=JCODE_RELEASE_BUILD");
}

fn env_or_metadata_or_git<const N: usize>(
    env_name: &str,
    metadata_key: &str,
    git_args: [&str; N],
) -> Option<String> {
    std::env::var(env_name)
        .ok()
        .or_else(|| metadata_value(metadata_key))
        .or_else(|| git_output(git_args))
        .map(|value| value.trim().to_string())
}

fn git_output<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn metadata_value(key: &str) -> Option<String> {
    let path = std::env::var("JCODE_BUILD_METADATA_FILE").ok()?;
    let data = fs::read_to_string(path).ok()?;
    data.lines().find_map(|line| {
        let (entry_key, entry_value) = line.split_once('=')?;
        if entry_key == key {
            Some(entry_value.to_string())
        } else {
            None
        }
    })
}

/// Get and increment the build number, scoped to the current major.minor version.
/// Resets to 1 when the version in Cargo.toml is bumped.
fn increment_build_number(major: &str, minor: &str) -> u32 {
    let jcode_dir = dirs::home_dir()
        .map(|h| h.join(".jcode"))
        .unwrap_or_else(|| PathBuf::from(".jcode"));

    let _ = fs::create_dir_all(&jcode_dir);

    let build_file = jcode_dir.join("build_number");
    let version_file = jcode_dir.join("build_version");

    let current_version = format!("{}.{}", major, minor);

    // Check if the version changed (Cargo.toml was bumped)
    let stored_version = fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if stored_version != current_version {
        // Version bumped — reset build number
        let _ = fs::write(&version_file, &current_version);
        let _ = fs::write(&build_file, "1");
        return 1;
    }

    // Same version — increment
    let current = fs::read_to_string(&build_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);

    let next = current + 1;
    let _ = fs::write(&build_file, next.to_string());

    next
}
