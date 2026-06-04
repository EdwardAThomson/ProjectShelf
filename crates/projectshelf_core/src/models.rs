use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub name: String,
    pub path: String,
    pub icon_kind: IconKind,
    pub last_seen: i64,
    pub last_commit_ts: Option<i64>,
    pub last_fs_activity_ts: Option<i64>,
    pub dirty: bool,
    pub branch: Option<String>,
    pub primary_language: Option<String>,
    pub github_url: Option<String>,
    pub health: HealthChecks,
    pub tasks: TaskProgress,
}

/// Presence flags for common project hygiene files, computed on scan.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct HealthChecks {
    pub has_readme: bool,
    pub has_license: bool,
    pub has_tests: bool,
    pub has_ci: bool,
}

/// Cached progress of a markdown task list (`- [ ]` / `- [x]`) found in a
/// project's tracking file. The full item list is re-read from disk on demand;
/// these counts exist so the list can show progress instantly from cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskProgress {
    pub total: u32,
    pub done: u32,
    /// Filename the tasks were read from, e.g. `TODO.md`. `None` if no list.
    pub source: Option<String>,
}

impl Project {
    pub fn activity_ts(&self) -> i64 {
        match (self.last_commit_ts, self.last_fs_activity_ts) {
            (Some(c), Some(f)) => c.max(f),
            (Some(c), None) => c,
            (None, Some(f)) => f,
            (None, None) => self.last_seen,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IconKind {
    Rust,
    Python,
    Node,
    Go,
    Cpp,
    #[default]
    Git,
    Marked,
}

impl IconKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            IconKind::Rust => "rust",
            IconKind::Python => "python",
            IconKind::Node => "node",
            IconKind::Go => "go",
            IconKind::Cpp => "cpp",
            IconKind::Git => "git",
            IconKind::Marked => "marked",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "rust" => IconKind::Rust,
            "python" => IconKind::Python,
            "node" => IconKind::Node,
            "go" => IconKind::Go,
            "cpp" => IconKind::Cpp,
            "marked" => IconKind::Marked,
            _ => IconKind::Git,
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            IconKind::Rust => "🔧",
            IconKind::Python => "🐍",
            IconKind::Node => "📦",
            IconKind::Go => "🐹",
            IconKind::Cpp => "⚙️",
            IconKind::Git => "📁",
            IconKind::Marked => "📌",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageStats {
    pub project_id: String,
    pub language: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MilestoneStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl MilestoneStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MilestoneStatus::Todo => "todo",
            MilestoneStatus::InProgress => "in_progress",
            MilestoneStatus::Blocked => "blocked",
            MilestoneStatus::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => MilestoneStatus::InProgress,
            "blocked" => MilestoneStatus::Blocked,
            "done" => MilestoneStatus::Done,
            _ => MilestoneStatus::Todo,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub project_id: String,
    pub id: String,
    pub title: String,
    pub status: MilestoneStatus,
    pub due_ts: Option<i64>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserMeta {
    pub project_id: String,
    pub pinned: bool,
    pub notes: String,
    /// User-authored project description. Only used as a fallback when the
    /// project's `ROADMAP.md` has no preamble to derive one from.
    pub description: String,
}
