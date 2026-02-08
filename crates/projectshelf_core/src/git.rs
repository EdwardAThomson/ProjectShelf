use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct GitMetadata {
    pub is_git_repo: bool,
    pub last_commit_ts: Option<i64>,
    pub dirty: bool,
    pub branch: Option<String>,
    pub remote_url: Option<String>,
    pub github_url: Option<String>,
}

pub fn get_git_metadata(path: &Path) -> GitMetadata {
    if !is_git_repo(path) {
        return GitMetadata::default();
    }

    let remote_url = get_remote_url(path);
    let github_url = remote_url.as_ref().and_then(|url| parse_github_url(url));

    GitMetadata {
        is_git_repo: true,
        last_commit_ts: get_last_commit_timestamp(path),
        dirty: is_dirty(path),
        branch: get_current_branch(path),
        remote_url,
        github_url,
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

fn get_remote_url(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "remote", "get-url", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

fn parse_github_url(remote_url: &str) -> Option<String> {
    // Handle SSH format: git@github.com:owner/repo.git
    if remote_url.starts_with("git@github.com:") {
        let path = remote_url.strip_prefix("git@github.com:")?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        return Some(format!("https://github.com/{}", path));
    }

    // Handle HTTPS format: https://github.com/owner/repo.git
    if remote_url.starts_with("https://github.com/") {
        let url = remote_url.strip_suffix(".git").unwrap_or(remote_url);
        return Some(url.to_string());
    }

    // Handle HTTP format: http://github.com/owner/repo.git
    if remote_url.starts_with("http://github.com/") {
        let path = remote_url.strip_prefix("http://github.com/")?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        return Some(format!("https://github.com/{}", path));
    }

    None
}
