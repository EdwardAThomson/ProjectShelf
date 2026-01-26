use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct GitMetadata {
    pub is_git_repo: bool,
    pub last_commit_ts: Option<i64>,
    pub dirty: bool,
    pub branch: Option<String>,
}

pub fn get_git_metadata(path: &Path) -> GitMetadata {
    if !is_git_repo(path) {
        return GitMetadata::default();
    }

    GitMetadata {
        is_git_repo: true,
        last_commit_ts: get_last_commit_timestamp(path),
        dirty: is_dirty(path),
        branch: get_current_branch(path),
    }
}

fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_last_commit_timestamp(path: &Path) -> Option<i64> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "log", "-1", "--format=%ct"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()
}

fn is_dirty(path: &Path) -> bool {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "status", "--porcelain"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn get_current_branch(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}
