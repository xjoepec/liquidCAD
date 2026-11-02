//! Local SQLite store: recent projects and per-project revision history.
//! Every accepted edit commits a full project snapshot, so any revision can
//! be restored and the history survives restarts.

use chrono::Utc;
use liquidcad_core::Project;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Core(#[from] liquidcad_core::Error),
    #[error("revision {0} not found")]
    MissingRevision(i64),
    #[error("database schema {found} is newer than this build ({supported})")]
    Schema { found: i64, supported: i64 },
}

pub struct Store {
    conn: Connection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevisionSummary {
    pub id: i64,
    pub created_at: String,
    pub note: String,
    pub component_count: i64,
    pub mass_lb: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub path: String,
    pub last_opened_at: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS projects (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 path TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 last_opened_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS revisions (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 created_at TEXT NOT NULL,
                 note TEXT NOT NULL,
                 snapshot TEXT NOT NULL,
                 component_count INTEGER NOT NULL,
                 mass_lb REAL NOT NULL
             );
             CREATE INDEX IF NOT EXISTS revisions_by_project ON revisions(project_id, id);",
        )?;
        let found: Option<i64> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .and_then(|v| v.parse().ok());
        match found {
            Some(v) if v > SCHEMA_VERSION => {
                return Err(StoreError::Schema {
                    found: v,
                    supported: SCHEMA_VERSION,
                })
            }
            Some(_) => {}
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )?;
            }
        }
        Ok(Self { conn })
    }

    /// Records that a project was opened from `path`.
    pub fn touch_project(&self, project: &Project, path: &str) -> Result<(), StoreError> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, path = excluded.path,
                 last_opened_at = excluded.last_opened_at",
            params![project.id, project.name, path, now],
        )?;
        Ok(())
    }

    pub fn commit(
        &self,
        project: &Project,
        note: &str,
        component_count: usize,
        mass_lb: f64,
    ) -> Result<i64, StoreError> {
        let snapshot = project.to_json()?;
        self.conn.execute(
            "INSERT INTO revisions (project_id, created_at, note, snapshot, component_count, mass_lb)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project.id,
                Utc::now().to_rfc3339(),
                note,
                snapshot,
                component_count as i64,
                mass_lb
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn history(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Result<Vec<RevisionSummary>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_at, note, component_count, mass_lb FROM revisions
             WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![project_id, limit as i64], |row| {
            Ok(RevisionSummary {
                id: row.get(0)?,
                created_at: row.get(1)?,
                note: row.get(2)?,
                component_count: row.get(3)?,
                mass_lb: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn snapshot(&self, project_id: &str, revision: i64) -> Result<Project, StoreError> {
        let text: String = self
            .conn
            .query_row(
                "SELECT snapshot FROM revisions WHERE id = ?1 AND project_id = ?2",
                params![revision, project_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::MissingRevision(revision))?;
        Ok(Project::from_json(&text)?)
    }

    pub fn recent_projects(&self, limit: usize) -> Result<Vec<RecentProject>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, last_opened_at FROM projects ORDER BY last_opened_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(RecentProject {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                last_opened_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::templates::modular_dock;

    #[test]
    fn records_and_restores_revisions() {
        let store = Store::open_in_memory().unwrap();
        let mut project = modular_dock();
        store.touch_project(&project, "/tmp/dock.json").unwrap();
        let first = store.commit(&project, "open", 80, 1200.0).unwrap();
        project.set_parameter("width_in", 120.0).unwrap();
        store
            .commit(&project, "width_in = 120", 96, 1400.0)
            .unwrap();
        let history = store.history(&project.id, 10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].note, "width_in = 120");
        let restored = store.snapshot(&project.id, first).unwrap();
        assert_eq!(restored.parameters["width_in"].value, 96.0);
        assert!(matches!(
            store.snapshot(&project.id, 999),
            Err(StoreError::MissingRevision(999))
        ));
    }

    #[test]
    fn persists_across_connections() {
        let dir = std::env::temp_dir().join(format!("liquidcad-store-{}", std::process::id()));
        let path = dir.join("liquidcad.sqlite");
        let _ = std::fs::remove_file(&path);
        let project = modular_dock();
        {
            let store = Store::open(&path).unwrap();
            store.touch_project(&project, "a.json").unwrap();
            store.commit(&project, "open", 1, 1.0).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.history(&project.id, 5).unwrap().len(), 1);
        assert_eq!(store.recent_projects(5).unwrap()[0].path, "a.json");
    }
}
