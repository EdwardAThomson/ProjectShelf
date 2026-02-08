use crate::models::{Milestone, MilestoneStatus, UserMeta};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectFile {
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub milestones: Vec<ProjectFileMilestone>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFileMilestone {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
}

impl ProjectFile {
    pub fn load(project_path: &Path) -> Option<Self> {
        let yaml_path = project_path.join(".projman").join("project.yaml");
        if !yaml_path.exists() {
            return None;
        }

        let content = fs::read_to_string(&yaml_path).ok()?;
        serde_yaml::from_str(&content).ok()
    }

    pub fn save(&self, project_path: &Path) -> std::io::Result<()> {
        let projman_dir = project_path.join(".projman");
        fs::create_dir_all(&projman_dir)?;

        let yaml_path = projman_dir.join("project.yaml");
        let content = serde_yaml::to_string(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;

        fs::write(yaml_path, content)
    }

    pub fn to_milestones(&self, project_id: &str) -> Vec<Milestone> {
        self.milestones
            .iter()
            .map(|m| Milestone {
                project_id: project_id.to_string(),
                id: m.id.clone(),
                title: m.title.clone(),
                status: MilestoneStatus::from_str(&m.status),
                due_ts: m.due.as_ref().and_then(|d| parse_date(d)),
                link: m.link.clone(),
            })
            .collect()
    }

    pub fn to_user_meta(&self, project_id: &str) -> UserMeta {
        UserMeta {
            project_id: project_id.to_string(),
            pinned: self.pinned,
            notes: self.notes.clone(),
        }
    }

    pub fn from_data(milestones: &[Milestone], user_meta: &UserMeta) -> Self {
        ProjectFile {
            notes: user_meta.notes.clone(),
            pinned: user_meta.pinned,
            milestones: milestones
                .iter()
                .map(|m| ProjectFileMilestone {
                    id: m.id.clone(),
                    title: m.title.clone(),
                    status: m.status.as_str().to_string(),
                    due: m.due_ts.map(format_date),
                    link: m.link.clone(),
                })
                .collect(),
        }
    }
}

fn parse_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }

    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;

    let days_since_epoch = days_from_date(year, month, day)?;
    Some(days_since_epoch * 86400)
}

fn days_from_date(year: i32, month: u32, day: u32) -> Option<i64> {
    if month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }

    let mut days: i64 = 0;

    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    let days_in_months = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        days += days_in_months[m as usize] as i64;
        if m == 2 && is_leap_year(year) {
            days += 1;
        }
    }

    days += (day - 1) as i64;
    Some(days)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn format_date(ts: i64) -> String {
    let days = ts / 86400;
    let mut remaining_days = days;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1;

    for m in 1..=12 {
        let mut dim = days_in_months[m];
        if m == 2 && is_leap_year(year) {
            dim += 1;
        }
        if remaining_days < dim {
            month = m;
            break;
        }
        remaining_days -= dim;
    }

    let day = remaining_days + 1;
    format!("{:04}-{:02}-{:02}", year, month, day)
}
