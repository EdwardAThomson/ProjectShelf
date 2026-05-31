use crate::config::{IGNORED_DIRS, MAX_DEPTH};
use crate::fs_activity::get_last_fs_activity;
use crate::git::get_git_metadata;
use crate::languages::{detect_languages, LanguageBreakdown};
use crate::checklist::parse_checklist;
use crate::models::{HealthChecks, IconKind, Project, TaskProgress};
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
        // Deliberately do NOT return here. Keep descending so nested *independent*
        // repos are also discovered — i.e. a "meta-repo" workspace that is itself a
        // git repo (tracking shared/docs) but contains its own sub-repos in
        // subdirectories. `IGNORED_DIRS` (skips `.git`, `node_modules`, `vendor`,
        // `target`, …) and `MAX_DEPTH` bound the extra walking; a normal project's
        // own source tree contains no markers, so nothing spurious is added.
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

pub fn build_project(discovered: &DiscoveredProject) -> (Project, LanguageBreakdown) {
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
    let lang_breakdown = detect_languages(&discovered.path);
    let primary_language = lang_breakdown.primary_language().map(|s| s.to_string());
    let health = detect_health(&discovered.path);
    let tasks = parse_checklist(&discovered.path)
        .map(|c| TaskProgress {
            total: c.total(),
            done: c.done(),
            source: Some(c.source),
        })
        .unwrap_or_default();

    let project = Project {
        project_id,
        name,
        path: path_str,
        icon_kind,
        last_seen: now,
        last_commit_ts: git_meta.last_commit_ts,
        last_fs_activity_ts,
        dirty: git_meta.dirty,
        branch: git_meta.branch,
        primary_language,
        github_url: git_meta.github_url,
        health,
        tasks,
    };

    (project, lang_breakdown)
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

/// Check for common project-hygiene files: a README, a license, a test
/// directory, and a CI config. All checks are shallow (project root only).
fn detect_health(path: &Path) -> HealthChecks {
    let mut health = HealthChecks::default();

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let upper = name.to_uppercase();
            if upper.starts_with("README") {
                health.has_readme = true;
            }
            if upper.starts_with("LICENSE") || upper.starts_with("LICENCE") || upper == "COPYING" {
                health.has_license = true;
            }
            // Test files that conventionally live alongside source (Go, Python,
            // JS/TS) rather than in a dedicated dir.
            if name.ends_with("_test.go")
                || (name.starts_with("test_") && name.ends_with(".py"))
                || name.ends_with("_test.py")
                || name.ends_with(".test.js")
                || name.ends_with(".test.ts")
                || name.ends_with(".spec.js")
                || name.ends_with(".spec.ts")
            {
                health.has_tests = true;
            }
        }
    }

    let test_dirs = ["tests", "test", "spec", "__tests__"];
    health.has_tests = health.has_tests || test_dirs.iter().any(|d| path.join(d).is_dir());

    health.has_ci = dir_has_entries(&path.join(".github").join("workflows"))
        || path.join(".gitlab-ci.yml").exists()
        || path.join(".travis.yml").exists()
        || path.join(".circleci").is_dir()
        || path.join("azure-pipelines.yml").exists()
        || path.join("Jenkinsfile").exists();

    health
}

fn dir_has_entries(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut e| e.next().is_some())
        .unwrap_or(false)
}

pub fn scan_projects(root: &Path) -> Vec<(Project, LanguageBreakdown)> {
    discover_projects(root)
        .iter()
        .map(build_project)
        .collect()
}

/// Walk up from `path` (a file or dir that changed) toward `root`, returning
/// the nearest ancestor that looks like a project (has `.git/` or a marker).
/// Returns `None` if no project is found before reaching `root`.
pub fn find_project_root(path: &Path, root: &Path) -> Option<PathBuf> {
    let mut cur = Some(path);
    while let Some(c) = cur {
        if !c.starts_with(root) {
            break;
        }
        let is_project =
            c.join(".git").exists() || c.join(".projman").join("project.yaml").exists();
        if is_project {
            return Some(c.to_path_buf());
        }
        if c == root {
            break;
        }
        cur = c.parent();
    }
    None
}

/// Re-scan a single known project directory (used by the file watcher for
/// incremental updates).
pub fn scan_single_project(dir: &Path) -> (Project, LanguageBreakdown) {
    let discovered = DiscoveredProject {
        path: dir.to_path_buf(),
        is_git: dir.join(".git").exists(),
        has_marker: dir.join(".projman").join("project.yaml").exists(),
    };
    build_project(&discovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn mk_dir(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
    }

    /// Create a directory that looks like a git repo (just the `.git` dir).
    fn mk_repo(dir: &Path) {
        fs::create_dir_all(dir.join(".git")).unwrap();
    }

    /// A meta-repo (itself a git repo) that contains its own independent
    /// sub-repos must surface both the meta-repo and every nested repo, while
    /// leaving plain source dirs and ignored dirs alone.
    #[test]
    fn discovers_nested_independent_repos() {
        let root = std::env::temp_dir().join("projectshelf_discover_test_nested");
        let _ = fs::remove_dir_all(&root);
        mk_dir(&root);

        // Meta-repo workspace: a repo that holds sub-repos in subdirectories.
        mk_repo(&root.join("meta"));
        mk_repo(&root.join("meta").join("inner")); // nested repo (direct child)
        mk_repo(&root.join("meta").join("client").join("demo")); // repo under a non-repo subdir
        mk_dir(&root.join("meta").join("src")); // plain source dir — not a project
        mk_repo(&root.join("meta").join("node_modules")); // ignored dir — must NOT surface

        // A normal standalone repo with a source subdir.
        mk_repo(&root.join("normal"));
        mk_dir(&root.join("normal").join("src"));

        let found: HashSet<String> = discover_projects(&root)
            .iter()
            .map(|p| {
                p.path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert!(found.contains("meta"), "meta-repo itself should be discovered");
        assert!(found.contains("normal"), "standalone repo should be discovered");
        assert!(found.contains("meta/inner"), "nested repo should be discovered");
        assert!(
            found.contains("meta/client/demo"),
            "repo nested under a non-repo subdir should be discovered"
        );
        assert!(
            !found.iter().any(|p| p.contains("node_modules")),
            "repos inside IGNORED_DIRS must not surface"
        );
        assert!(
            !found.contains("meta/src") && !found.contains("normal/src"),
            "plain source dirs are not projects"
        );
        assert_eq!(
            found.len(),
            4,
            "exactly: meta, meta/inner, meta/client/demo, normal — got {found:?}"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
