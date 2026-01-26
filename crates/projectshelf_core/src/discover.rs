use crate::config::{IGNORED_DIRS, MAX_DEPTH};
use crate::fs_activity::get_last_fs_activity;
use crate::git::get_git_metadata;
use crate::models::{IconKind, Project};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DiscoveredProject {
    pub path: PathBuf,
    pub is_git: bool,
    pub has_marker: bool,
}

pub fn discover_projects(root: &Path) -> Vec<DiscoveredProject> {
    let mut projects = Vec::new();
    discover_recursive(root, 0, &mut projects);
    projects
}

fn discover_recursive(dir: &Path, depth: usize, projects: &mut Vec<DiscoveredProject>) {
    if depth > MAX_DEPTH {
        return;
    }

    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if IGNORED_DIRS.contains(&dir_name) {
        return;
    }

    let has_git = dir.join(".git").exists();
    let has_marker = dir.join(".projman").join("project.yaml").exists();

    if has_git || has_marker {
        projects.push(DiscoveredProject {
            path: dir.to_path_buf(),
            is_git: has_git,
            has_marker,
        });
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_recursive(&path, depth + 1, projects);
        }
    }
}

pub fn build_project(discovered: &DiscoveredProject) -> Project {
    let path_str = discovered.path.to_string_lossy().to_string();
    let project_id = generate_project_id(&path_str);
    let name = discovered
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let icon_kind = detect_icon_kind(&discovered.path, discovered.is_git);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let git_meta = get_git_metadata(&discovered.path);
    let last_fs_activity_ts = get_last_fs_activity(&discovered.path);

    Project {
        project_id,
        name,
        path: path_str,
        icon_kind,
        last_seen: now,
        last_commit_ts: git_meta.last_commit_ts,
        last_fs_activity_ts,
        dirty: git_meta.dirty,
        branch: git_meta.branch,
        primary_language: None,
    }
}

fn generate_project_id(canonical_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_path.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

fn detect_icon_kind(path: &Path, is_git: bool) -> IconKind {
    if path.join("Cargo.toml").exists() {
        return IconKind::Rust;
    }
    if path.join("pyproject.toml").exists() || path.join("requirements.txt").exists() {
        return IconKind::Python;
    }
    if path.join("package.json").exists() {
        return IconKind::Node;
    }
    if path.join("go.mod").exists() {
        return IconKind::Go;
    }
    if path.join("CMakeLists.txt").exists() {
        return IconKind::Cpp;
    }
    if is_git {
        IconKind::Git
    } else {
        IconKind::Marked
    }
}

pub fn scan_projects(root: &Path) -> Vec<Project> {
    discover_projects(root)
        .iter()
        .map(build_project)
        .collect()
}
