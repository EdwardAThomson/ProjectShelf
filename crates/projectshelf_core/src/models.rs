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
            IconKind::Rust => "🦀",
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
}
