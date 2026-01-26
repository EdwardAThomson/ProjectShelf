use crate::config::{data_dir, db_path};
use crate::languages::LanguageBreakdown;
use crate::models::{IconKind, Project};
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
                primary_language TEXT
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
        Ok(())
    }

    pub fn upsert_project(&self, project: &Project) -> Result<(), DbError> {
        self.conn.execute(
            r#"
            INSERT INTO projects (
                project_id, name, path, icon_kind, last_seen,
                last_commit_ts, last_fs_activity_ts, dirty, branch, primary_language
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(project_id) DO UPDATE SET
                name = excluded.name,
                path = excluded.path,
                icon_kind = excluded.icon_kind,
                last_seen = excluded.last_seen,
                last_commit_ts = excluded.last_commit_ts,
                last_fs_activity_ts = excluded.last_fs_activity_ts,
                dirty = excluded.dirty,
                branch = excluded.branch,
                primary_language = excluded.primary_language
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
            ],
        )?;
        Ok(())
    }

    pub fn get_all_projects(&self) -> Result<Vec<Project>, DbError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT project_id, name, path, icon_kind, last_seen,
                   last_commit_ts, last_fs_activity_ts, dirty, branch, primary_language
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
}
