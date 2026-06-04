use crate::config::{data_dir, db_path};
use crate::languages::LanguageBreakdown;
use crate::models::{
    HealthChecks, IconKind, Milestone, MilestoneStatus, Project, TaskProgress, UserMeta,
};
use rusqlite::{params, Connection, Result};
use std::fs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open() -> Result<Self, DbError> {
        let data = data_dir();
        fs::create_dir_all(&data)?;

        let conn = Connection::open(db_path())?;
        // WAL + synchronous=NORMAL: writes go to a write-ahead log and only
        // fsync at checkpoints instead of on every commit. SQLite's default
        // (DELETE journal + synchronous=FULL) fsyncs per commit, so a scan's
        // many small upserts become an fsync storm that stalls on the ext4
        // journal and freezes the UI when the disk is busy. WAL also lets reads
        // proceed during writes. busy_timeout avoids SQLITE_BUSY when a
        // checkpoint overlaps a write. (NORMAL can lose the last transaction on
        // an OS/power crash, never corrupt the db — fine for a rebuildable cache.)
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;
        let db = Database { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS projects (
                project_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT UNIQUE NOT NULL,
                icon_kind TEXT NOT NULL,
                last_seen INTEGER NOT NULL,
                last_commit_ts INTEGER,
                last_fs_activity_ts INTEGER,
                dirty INTEGER NOT NULL DEFAULT 0,
                branch TEXT,
                primary_language TEXT,
                github_url TEXT,
                has_readme INTEGER NOT NULL DEFAULT 0,
                has_license INTEGER NOT NULL DEFAULT 0,
                has_tests INTEGER NOT NULL DEFAULT 0,
                has_ci INTEGER NOT NULL DEFAULT 0,
                task_total INTEGER NOT NULL DEFAULT 0,
                task_done INTEGER NOT NULL DEFAULT 0,
                task_source TEXT
            );

            CREATE TABLE IF NOT EXISTS languages (
                project_id TEXT NOT NULL,
                language TEXT NOT NULL,
                bytes INTEGER NOT NULL,
                PRIMARY KEY (project_id, language),
                FOREIGN KEY (project_id) REFERENCES projects(project_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS user_meta (
                project_id TEXT PRIMARY KEY,
                pinned INTEGER NOT NULL DEFAULT 0,
                notes TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                FOREIGN KEY (project_id) REFERENCES projects(project_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS milestones (
                project_id TEXT NOT NULL,
                id TEXT NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                due_ts INTEGER,
                link TEXT,
                PRIMARY KEY (project_id, id),
                FOREIGN KEY (project_id) REFERENCES projects(project_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tags (
                project_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (project_id, tag),
                FOREIGN KEY (project_id) REFERENCES projects(project_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS scan_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_scan_ts INTEGER NOT NULL,
                version INTEGER NOT NULL DEFAULT 1
            );

            INSERT OR IGNORE INTO scan_state (id, last_scan_ts, version) VALUES (1, 0, 1);
            "#,
        )?;

        // Migration: add github_url column if missing
        let _ = self.conn.execute(
            "ALTER TABLE projects ADD COLUMN github_url TEXT",
            [],
        );

        // Migration: add health-check columns if missing
        for col in ["has_readme", "has_license", "has_tests", "has_ci"] {
            let _ = self.conn.execute(
                &format!("ALTER TABLE projects ADD COLUMN {col} INTEGER NOT NULL DEFAULT 0"),
                [],
            );
        }

        // Migration: add task-progress columns if missing
        for col in ["task_total", "task_done"] {
            let _ = self.conn.execute(
                &format!("ALTER TABLE projects ADD COLUMN {col} INTEGER NOT NULL DEFAULT 0"),
                [],
            );
        }
        let _ = self
            .conn
            .execute("ALTER TABLE projects ADD COLUMN task_source TEXT", []);

        // Migration: add the user-authored description column if missing.
        let _ = self.conn.execute(
            "ALTER TABLE user_meta ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            [],
        );

        Ok(())
    }

    pub fn upsert_project(&self, project: &Project) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            INSERT INTO projects (
                project_id, name, path, icon_kind, last_seen,
                last_commit_ts, last_fs_activity_ts, dirty, branch, primary_language, github_url,
                has_readme, has_license, has_tests, has_ci,
                task_total, task_done, task_source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ON CONFLICT(project_id) DO UPDATE SET
                name = excluded.name,
                path = excluded.path,
                icon_kind = excluded.icon_kind,
                last_seen = excluded.last_seen,
                last_commit_ts = excluded.last_commit_ts,
                last_fs_activity_ts = excluded.last_fs_activity_ts,
                dirty = excluded.dirty,
                branch = excluded.branch,
                primary_language = excluded.primary_language,
                github_url = excluded.github_url,
                has_readme = excluded.has_readme,
                has_license = excluded.has_license,
                has_tests = excluded.has_tests,
                has_ci = excluded.has_ci,
                task_total = excluded.task_total,
                task_done = excluded.task_done,
                task_source = excluded.task_source
            "#,
            params![
                project.project_id,
                project.name,
                project.path,
                project.icon_kind.as_str(),
                project.last_seen,
                project.last_commit_ts,
                project.last_fs_activity_ts,
                project.dirty as i32,
                project.branch,
                project.primary_language,
                project.github_url,
                project.health.has_readme as i32,
                project.health.has_license as i32,
                project.health.has_tests as i32,
                project.health.has_ci as i32,
                project.tasks.total,
                project.tasks.done,
                project.tasks.source,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_projects(&self) -> Result<Vec<Project>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT project_id, name, path, icon_kind, last_seen,
                   last_commit_ts, last_fs_activity_ts, dirty, branch, primary_language, github_url,
                   has_readme, has_license, has_tests, has_ci,
                   task_total, task_done, task_source
            FROM projects
            ORDER BY last_seen DESC
            "#,
        )?;

        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    project_id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    icon_kind: IconKind::from_str(&row.get::<_, String>(3)?),
                    last_seen: row.get(4)?,
                    last_commit_ts: row.get(5)?,
                    last_fs_activity_ts: row.get(6)?,
                    dirty: row.get::<_, i32>(7)? != 0,
                    branch: row.get(8)?,
                    primary_language: row.get(9)?,
                    github_url: row.get(10)?,
                    health: HealthChecks {
                        has_readme: row.get::<_, i32>(11)? != 0,
                        has_license: row.get::<_, i32>(12)? != 0,
                        has_tests: row.get::<_, i32>(13)? != 0,
                        has_ci: row.get::<_, i32>(14)? != 0,
                    },
                    tasks: TaskProgress {
                        total: row.get::<_, i64>(15)? as u32,
                        done: row.get::<_, i64>(16)? as u32,
                        source: row.get(17)?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    pub fn update_scan_state(&self, ts: i64) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE scan_state SET last_scan_ts = ?1 WHERE id = 1",
            params![ts],
        )?;
        Ok(())
    }

    pub fn get_scan_state(&self) -> Result<(i64, i32), DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT last_scan_ts, version FROM scan_state WHERE id = 1")?;
        let result = stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(result)
    }

    pub fn upsert_languages(&self, project_id: &str, breakdown: &LanguageBreakdown) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM languages WHERE project_id = ?1",
            params![project_id],
        )?;

        for (lang, bytes) in &breakdown.languages {
            self.conn.execute(
                "INSERT INTO languages (project_id, language, bytes) VALUES (?1, ?2, ?3)",
                params![project_id, lang, *bytes as i64],
            )?;
        }
        Ok(())
    }

    pub fn get_languages(&self, project_id: &str) -> Result<Vec<(String, u64)>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT language, bytes FROM languages WHERE project_id = ?1 ORDER BY bytes DESC",
        )?;

        let langs = stmt
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(langs)
    }

    pub fn upsert_user_meta(&self, meta: &UserMeta) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            INSERT INTO user_meta (project_id, pinned, notes, description)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(project_id) DO UPDATE SET
                pinned = excluded.pinned,
                notes = excluded.notes,
                description = excluded.description
            "#,
            params![meta.project_id, meta.pinned as i32, meta.notes, meta.description],
        )?;
        Ok(())
    }

    pub fn get_user_meta(&self, project_id: &str) -> Result<UserMeta, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT project_id, pinned, notes, description FROM user_meta WHERE project_id = ?1",
        )?;

        let result = stmt.query_row(params![project_id], |row| {
            Ok(UserMeta {
                project_id: row.get(0)?,
                pinned: row.get::<_, i32>(1)? != 0,
                notes: row.get(2)?,
                description: row.get(3)?,
            })
        });

        match result {
            Ok(meta) => Ok(meta),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(UserMeta {
                project_id: project_id.to_string(),
                ..Default::default()
            }),
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    pub fn upsert_milestone(&self, milestone: &Milestone) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            INSERT INTO milestones (project_id, id, title, status, due_ts, link)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(project_id, id) DO UPDATE SET
                title = excluded.title,
                status = excluded.status,
                due_ts = excluded.due_ts,
                link = excluded.link
            "#,
            params![
                milestone.project_id,
                milestone.id,
                milestone.title,
                milestone.status.as_str(),
                milestone.due_ts,
                milestone.link,
            ],
        )?;
        Ok(())
    }

    pub fn delete_milestone(&self, project_id: &str, milestone_id: &str) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM milestones WHERE project_id = ?1 AND id = ?2",
            params![project_id, milestone_id],
        )?;
        Ok(())
    }

    pub fn add_tag(&self, project_id: &str, tag: &str) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO tags (project_id, tag) VALUES (?1, ?2)",
            params![project_id, tag],
        )?;
        Ok(())
    }

    pub fn remove_tag(&self, project_id: &str, tag: &str) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM tags WHERE project_id = ?1 AND tag = ?2",
            params![project_id, tag],
        )?;
        Ok(())
    }

    pub fn get_tags(&self, project_id: &str) -> Result<Vec<String>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT tag FROM tags WHERE project_id = ?1 ORDER BY tag",
        )?;
        let tags = stmt
            .query_map(params![project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(tags)
    }

    pub fn get_all_tags(&self) -> Result<Vec<String>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT tag FROM tags ORDER BY tag",
        )?;
        let tags = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(tags)
    }

    pub fn get_milestones(&self, project_id: &str) -> Result<Vec<Milestone>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT project_id, id, title, status, due_ts, link FROM milestones WHERE project_id = ?1",
        )?;

        let milestones = stmt
            .query_map(params![project_id], |row| {
                Ok(Milestone {
                    project_id: row.get(0)?,
                    id: row.get(1)?,
                    title: row.get(2)?,
                    status: MilestoneStatus::from_str(&row.get::<_, String>(3)?),
                    due_ts: row.get(4)?,
                    link: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(milestones)
    }
}
