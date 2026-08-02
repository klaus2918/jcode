//! Runtime/build provenance (build channel, git checkout, CI, cargo).
//!
//! These describe *how* the running jcode binary was built and launched. They
//! were previously owned by the telemetry crate because telemetry used them in
//! its event envelope, but they are build metadata rather than usage reporting:
//! the discovery tool also sends them as coarse request headers to separate
//! likely user demand from self-dev and test traffic.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProvenance {
    pub build_channel: String,
    pub is_git_checkout: bool,
    pub is_ci: bool,
    pub ran_from_cargo: bool,
}

pub fn runtime_provenance() -> RuntimeProvenance {
    RuntimeProvenance {
        build_channel: build_channel(),
        is_git_checkout: is_git_checkout(),
        is_ci: is_ci(),
        ran_from_cargo: ran_from_cargo(),
    }
}

pub fn build_channel() -> String {
    // `JCODE_CLIENT_SELFDEV_MODE` matches `jcode_selfdev_types::CLIENT_SELFDEV_ENV`.
    if std::env::var("JCODE_CLIENT_SELFDEV_MODE").is_ok() {
        return "selfdev".to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.to_string_lossy();
        if path.contains("/target/debug/") || path.contains("\\target\\debug\\") {
            return "debug".to_string();
        }
        if path.contains("/target/release/") || path.contains("\\target\\release\\") {
            return "local_build".to_string();
        }
    }
    if jcode_repo_dir().is_some() {
        return "git_checkout".to_string();
    }
    "release".to_string()
}

pub fn is_git_checkout() -> bool {
    jcode_repo_dir().is_some()
}

pub fn is_ci() -> bool {
    [
        "CI",
        "GITHUB_ACTIONS",
        "BUILDKITE",
        "JENKINS_URL",
        "GITLAB_CI",
        "CIRCLECI",
    ]
    .iter()
    .any(|key| std::env::var(key).is_ok())
}

pub fn ran_from_cargo() -> bool {
    std::env::var("CARGO").is_ok() || std::env::var("CARGO_MANIFEST_DIR").is_ok()
}

/// Locate the jcode repository root (used to classify a build as a git
/// checkout). Honors `JCODE_REPO_DIR`, then walks ancestors of the manifest
/// directory, then the current executable's ancestors.
fn jcode_repo_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("JCODE_REPO_DIR") {
        let path = PathBuf::from(path);
        if is_jcode_repo_dir(&path) {
            return Some(path);
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo) = find_jcode_repo_in_ancestors(&manifest_dir) {
        return Some(repo);
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
        && let Some(repo) = find_jcode_repo_in_ancestors(parent)
    {
        return Some(repo);
    }

    None
}

fn is_jcode_repo_dir(dir: &Path) -> bool {
    let cargo_toml = dir.join("Cargo.toml");
    if !cargo_toml.exists() || !dir.join(".git").exists() {
        return false;
    }

    std::fs::read_to_string(cargo_toml)
        .map(|content| content.contains("name = \"jcode\""))
        .unwrap_or(false)
}

fn find_jcode_repo_in_ancestors(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| is_jcode_repo_dir(dir))
        .map(Path::to_path_buf)
}
