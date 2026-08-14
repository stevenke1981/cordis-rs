//! Unified SQLite store used by Core, Memory, Runtime, Workflow and Capability.

use cordis_contracts::{new_id, now_rfc3339};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

const DATABASE_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("store lock is poisoned")]
    Poisoned,
    #[error("record already exists: {0}")]
    AlreadyExists(String),
    #[error("record was not found: {0}")]
    NotFound(String),
    #[error("record is already finalized: {0}")]
    AlreadyFinalized(String),
    #[error("store invariant failed: {0}")]
    Invariant(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone)]
pub struct CordisStore {
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct StoredTask {
    pub task_id: String,
    pub project_id: String,
    pub domain: String,
    pub strategy_id: String,
    pub status: String,
    pub payload: Value,
    pub cognitive_ir: Value,
    pub prediction: Value,
    pub created_at: String,
    pub finalized_at: Option<String>,
}

impl CordisStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                StoreError::Invariant(format!("cannot create database directory: {error}"))
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let store = Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> StoreResult<T>,
    ) -> StoreResult<T> {
        let guard = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        operation(&guard)
    }

    pub fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> StoreResult<T>,
    ) -> StoreResult<T> {
        let mut guard = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let transaction = guard.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = operation(&transaction)?;
        transaction.commit()?;
        Ok(result)
    }

    fn migrate(&self) -> StoreResult<()> {
        self.with_transaction(|tx| {
            tx.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS task_records (
                    task_id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    domain TEXT NOT NULL,
                    strategy_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    cognitive_json TEXT NOT NULL,
                    prediction_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    finalized_at TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_task_project_domain
                    ON task_records(project_id, domain, status);

                CREATE TABLE IF NOT EXISTS feedback_events (
                    id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL UNIQUE REFERENCES task_records(task_id) ON DELETE CASCADE,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS domain_states (
                    state_key TEXT PRIMARY KEY,
                    payload_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS strategy_states (
                    state_key TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    domain TEXT NOT NULL,
                    strategy_id TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_strategy_project_domain
                    ON strategy_states(project_id, domain);

                CREATE TABLE IF NOT EXISTS episodes (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    domain TEXT NOT NULL,
                    goal TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_episode_project_domain
                    ON episodes(project_id, domain, created_at DESC);

                CREATE TABLE IF NOT EXISTS world_patterns (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    domain TEXT NOT NULL,
                    normalized_statement TEXT NOT NULL,
                    statement TEXT NOT NULL,
                    source_ids_json TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(project_id, domain, normalized_statement)
                );

                CREATE TABLE IF NOT EXISTS memory_items (
                    id TEXT PRIMARY KEY,
                    scope TEXT NOT NULL,
                    project_id TEXT,
                    conversation_id TEXT,
                    task_id TEXT,
                    kind TEXT NOT NULL,
                    subject TEXT NOT NULL,
                    content TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    source_count INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    trust TEXT NOT NULL,
                    instruction_safe INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    last_verified_at TEXT,
                    metadata_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_memory_scope_project
                    ON memory_items(scope, project_id, kind, status);
                CREATE INDEX IF NOT EXISTS idx_memory_task ON memory_items(task_id);

                CREATE TABLE IF NOT EXISTS memory_sources (
                    item_id TEXT NOT NULL REFERENCES memory_items(id) ON DELETE CASCADE,
                    source_id TEXT NOT NULL,
                    evidence_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY(item_id, source_id)
                );

                CREATE TABLE IF NOT EXISTS graph_nodes (
                    id TEXT PRIMARY KEY,
                    node_type TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    project_id TEXT,
                    label TEXT NOT NULL,
                    metadata_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS graph_edges (
                    id TEXT PRIMARY KEY,
                    from_id TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                    relation TEXT NOT NULL,
                    to_id TEXT NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
                    confidence REAL NOT NULL,
                    source_id TEXT,
                    created_at TEXT NOT NULL,
                    UNIQUE(from_id, relation, to_id, source_id)
                );

                CREATE TABLE IF NOT EXISTS focus_records (
                    task_id TEXT PRIMARY KEY,
                    payload_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS workflows (
                    workflow_id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL UNIQUE,
                    status TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_workflow_status ON workflows(status);

                CREATE TABLE IF NOT EXISTS capabilities (
                    name TEXT PRIMARY KEY,
                    payload_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS audit_events (
                    id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    task_id TEXT,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_audit_task ON audit_events(task_id, created_at DESC);
                "#,
            )?;
            tx.execute(
                "INSERT INTO meta(key, value) VALUES('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [DATABASE_SCHEMA_VERSION.to_string()],
            )?;
            Ok(())
        })
    }

    pub fn initialize(&self) -> StoreResult<()> {
        self.audit(
            "store_initialized",
            None,
            &serde_json::json!({"path": self.path.display().to_string()}),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_task<T: Serialize, C: Serialize, P: Serialize>(
        &self,
        task_id: &str,
        project_id: &str,
        domain: &str,
        strategy_id: &str,
        payload: &T,
        cognitive_ir: &C,
        prediction: &P,
    ) -> StoreResult<()> {
        let payload = serde_json::to_string(payload)?;
        let cognitive = serde_json::to_string(cognitive_ir)?;
        let prediction = serde_json::to_string(prediction)?;
        let created_at = now_rfc3339();
        self.with_transaction(|tx| {
            let changed = tx.execute(
                "INSERT OR IGNORE INTO task_records(task_id, project_id, domain, strategy_id, status, payload_json, cognitive_json, prediction_json, created_at) VALUES(?1, ?2, ?3, ?4, 'open', ?5, ?6, ?7, ?8)",
                params![task_id, project_id, domain, strategy_id, payload, cognitive, prediction, created_at],
            )?;
            if changed == 0 {
                return Err(StoreError::AlreadyExists(task_id.to_owned()));
            }
            Ok(())
        })
    }

    pub fn get_task(&self, task_id: &str) -> StoreResult<Option<StoredTask>> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT task_id, project_id, domain, strategy_id, status, payload_json, cognitive_json, prediction_json, created_at, finalized_at FROM task_records WHERE task_id=?1",
                    [task_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, Option<String>>(9)?,
                        ))
                    },
                )
                .optional()?
                .map(|row| {
                    Ok(StoredTask {
                        task_id: row.0,
                        project_id: row.1,
                        domain: row.2,
                        strategy_id: row.3,
                        status: row.4,
                        payload: serde_json::from_str(&row.5)?,
                        cognitive_ir: serde_json::from_str(&row.6)?,
                        prediction: serde_json::from_str(&row.7)?,
                        created_at: row.8,
                        finalized_at: row.9,
                    })
                })
                .transpose()
        })
    }

    pub fn update_task_cognitive<T: Serialize, P: Serialize>(
        &self,
        task_id: &str,
        cognitive_ir: &T,
        prediction: &P,
    ) -> StoreResult<()> {
        let cognitive = serde_json::to_string(cognitive_ir)?;
        let prediction = serde_json::to_string(prediction)?;
        self.with_transaction(|tx| {
            let changed = tx.execute(
                "UPDATE task_records SET cognitive_json=?2, prediction_json=?3 WHERE task_id=?1 AND status='open'",
                params![task_id, cognitive, prediction],
            )?;
            if changed == 0 {
                return Err(StoreError::NotFound(task_id.to_owned()));
            }
            Ok(())
        })
    }

    pub fn finalize_task<T: Serialize>(
        &self,
        task_id: &str,
        feedback_id: &str,
        feedback: &T,
    ) -> StoreResult<()> {
        let payload = serde_json::to_string(feedback)?;
        let now = now_rfc3339();
        self.with_transaction(|tx| {
            let status: Option<String> = tx
                .query_row(
                    "SELECT status FROM task_records WHERE task_id=?1",
                    [task_id],
                    |row| row.get(0),
                )
                .optional()?;
            match status.as_deref() {
                None => return Err(StoreError::NotFound(task_id.to_owned())),
                Some("open") => {}
                Some(_) => return Err(StoreError::AlreadyFinalized(task_id.to_owned())),
            }
            tx.execute(
                "INSERT INTO feedback_events(id, task_id, payload_json, created_at) VALUES(?1, ?2, ?3, ?4)",
                params![feedback_id, task_id, payload, now],
            )?;
            tx.execute(
                "UPDATE task_records SET status='finalized', finalized_at=?2 WHERE task_id=?1",
                params![task_id, now],
            )?;
            Ok(())
        })
    }

    pub fn load_json<T: DeserializeOwned>(
        &self,
        table: JsonTable,
        key: &str,
    ) -> StoreResult<Option<T>> {
        let (table_name, key_column) = table.parts();
        let sql = format!("SELECT payload_json FROM {table_name} WHERE {key_column}=?1");
        self.with_connection(|connection| {
            let payload: Option<String> = connection
                .query_row(&sql, [key], |row| row.get(0))
                .optional()?;
            payload
                .map(|value| Ok(serde_json::from_str(&value)?))
                .transpose()
        })
    }

    pub fn upsert_domain<T: Serialize>(&self, key: &str, payload: &T) -> StoreResult<()> {
        let payload = serde_json::to_string(payload)?;
        self.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO domain_states(state_key, payload_json, updated_at) VALUES(?1, ?2, ?3) ON CONFLICT(state_key) DO UPDATE SET payload_json=excluded.payload_json, updated_at=excluded.updated_at",
                params![key, payload, now_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn upsert_strategy<T: Serialize>(
        &self,
        key: &str,
        project_id: &str,
        domain: &str,
        strategy_id: &str,
        payload: &T,
    ) -> StoreResult<()> {
        let payload = serde_json::to_string(payload)?;
        self.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO strategy_states(state_key, project_id, domain, strategy_id, payload_json, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(state_key) DO UPDATE SET payload_json=excluded.payload_json, updated_at=excluded.updated_at",
                params![key, project_id, domain, strategy_id, payload, now_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn strategy_payloads(&self, project_id: &str, domain: &str) -> StoreResult<Vec<Value>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT payload_json FROM strategy_states WHERE project_id=?1 AND domain=?2",
            )?;
            let rows =
                statement.query_map(params![project_id, domain], |row| row.get::<_, String>(0))?;
            let mut result = Vec::new();
            for row in rows {
                result.push(serde_json::from_str(&row?)?);
            }
            Ok(result)
        })
    }

    pub fn insert_episode<T: Serialize>(
        &self,
        id: &str,
        project_id: &str,
        domain: &str,
        goal: &str,
        payload: &T,
    ) -> StoreResult<()> {
        let payload = serde_json::to_string(payload)?;
        self.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO episodes(id, project_id, domain, goal, payload_json, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, project_id, domain, goal, payload, now_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn episode_payloads(
        &self,
        project_id: &str,
        domain: &str,
        limit: usize,
    ) -> StoreResult<Vec<Value>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT payload_json FROM episodes WHERE domain=?1 AND project_id IN (?2, 'global') ORDER BY created_at DESC LIMIT ?3",
            )?;
            let rows = statement.query_map(params![domain, project_id, limit as i64], |row| row.get::<_, String>(0))?;
            let mut result = Vec::new();
            for row in rows {
                result.push(serde_json::from_str(&row?)?);
            }
            Ok(result)
        })
    }

    pub fn upsert_world_pattern<T: Serialize>(
        &self,
        id: &str,
        project_id: &str,
        domain: &str,
        statement: &str,
        source_ids: &[String],
        payload: &T,
    ) -> StoreResult<()> {
        let normalized = statement.trim().to_lowercase();
        let sources = serde_json::to_string(source_ids)?;
        let payload = serde_json::to_string(payload)?;
        self.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO world_patterns(id, project_id, domain, normalized_statement, statement, source_ids_json, payload_json, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(project_id, domain, normalized_statement) DO UPDATE SET source_ids_json=excluded.source_ids_json, payload_json=excluded.payload_json, updated_at=excluded.updated_at",
                params![id, project_id, domain, normalized, statement, sources, payload, now_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn find_world_pattern(
        &self,
        project_id: &str,
        domain: &str,
        statement: &str,
    ) -> StoreResult<Option<(String, Vec<String>, Value)>> {
        let normalized = statement.trim().to_lowercase();
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id, source_ids_json, payload_json FROM world_patterns WHERE project_id=?1 AND domain=?2 AND normalized_statement=?3",
                    params![project_id, domain, normalized],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
                )
                .optional()?
                .map(|(id, sources, payload)| Ok((id, serde_json::from_str(&sources)?, serde_json::from_str(&payload)?)))
                .transpose()
        })
    }

    pub fn world_pattern_payloads(
        &self,
        project_id: &str,
        domain: &str,
    ) -> StoreResult<Vec<Value>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT payload_json FROM world_patterns WHERE project_id=?1 AND domain=?2",
            )?;
            let rows =
                statement.query_map(params![project_id, domain], |row| row.get::<_, String>(0))?;
            let mut result = Vec::new();
            for row in rows {
                result.push(serde_json::from_str(&row?)?);
            }
            Ok(result)
        })
    }

    pub fn save_focus<T: Serialize>(&self, task_id: &str, payload: &T) -> StoreResult<()> {
        self.upsert_simple("focus_records", "task_id", task_id, None, payload)
    }

    pub fn load_focus<T: DeserializeOwned>(&self, task_id: &str) -> StoreResult<Option<T>> {
        self.load_json(JsonTable::Focus, task_id)
    }

    pub fn list_focus<T: DeserializeOwned>(&self) -> StoreResult<Vec<T>> {
        self.list_json("focus_records")
    }

    pub fn remove_focus(&self, task_id: &str) -> StoreResult<()> {
        self.with_transaction(|tx| {
            tx.execute("DELETE FROM focus_records WHERE task_id=?1", [task_id])?;
            Ok(())
        })
    }

    pub fn save_workflow<T: Serialize>(
        &self,
        workflow_id: &str,
        task_id: &str,
        status: &str,
        payload: &T,
    ) -> StoreResult<()> {
        let payload = serde_json::to_string(payload)?;
        self.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO workflows(workflow_id, task_id, status, payload_json, updated_at) VALUES(?1, ?2, ?3, ?4, ?5) ON CONFLICT(workflow_id) DO UPDATE SET status=excluded.status, payload_json=excluded.payload_json, updated_at=excluded.updated_at",
                params![workflow_id, task_id, status, payload, now_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn load_workflow<T: DeserializeOwned>(&self, workflow_id: &str) -> StoreResult<Option<T>> {
        self.load_json(JsonTable::Workflow, workflow_id)
    }

    pub fn workflow_status_counts(&self) -> StoreResult<Vec<(String, usize)>> {
        self.with_connection(|connection| {
            let mut statement =
                connection.prepare("SELECT status, COUNT(*) FROM workflows GROUP BY status")?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
    }

    pub fn save_capability<T: Serialize>(&self, name: &str, payload: &T) -> StoreResult<()> {
        self.upsert_simple("capabilities", "name", name, None, payload)
    }

    pub fn load_capability<T: DeserializeOwned>(&self, name: &str) -> StoreResult<Option<T>> {
        self.load_json(JsonTable::Capability, name)
    }

    pub fn list_capabilities<T: DeserializeOwned>(&self) -> StoreResult<Vec<T>> {
        self.list_json("capabilities")
    }

    pub fn audit<T: Serialize>(
        &self,
        kind: &str,
        task_id: Option<&str>,
        payload: &T,
    ) -> StoreResult<()> {
        let id = new_id("audit");
        let payload = serde_json::to_string(payload)?;
        self.with_transaction(|tx| {
            tx.execute(
                "INSERT INTO audit_events(id, kind, task_id, payload_json, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![id, kind, task_id, payload, now_rfc3339()],
            )?;
            Ok(())
        })
    }

    /// Import the v0.5 Python cognitive SQLite tables into the unified Rust database.
    /// Imported non-event memory is untrusted and never instruction-safe by default.
    pub fn import_legacy_cognition(&self, legacy_path: impl AsRef<Path>) -> StoreResult<Value> {
        let legacy_path = legacy_path.as_ref();
        if !legacy_path.is_file() {
            return Err(StoreError::NotFound(legacy_path.display().to_string()));
        }
        let guard = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let before: i64 =
            guard.query_row("SELECT COUNT(*) FROM memory_items", [], |row| row.get(0))?;
        guard.execute(
            "ATTACH DATABASE ?1 AS legacy",
            [legacy_path.display().to_string()],
        )?;
        let operation = (|| -> StoreResult<()> {
            let has_memory: i64 = guard.query_row(
                "SELECT COUNT(*) FROM legacy.sqlite_master WHERE type='table' AND name='memory_items'",
                [],
                |row| row.get(0),
            )?;
            if has_memory == 0 {
                return Err(StoreError::Invariant(
                    "legacy cognition database has no memory_items table".to_owned(),
                ));
            }
            guard.execute_batch(
                r#"
                BEGIN IMMEDIATE;
                INSERT OR IGNORE INTO memory_items(
                    id, scope, project_id, conversation_id, task_id, kind, subject, content,
                    confidence, source_count, status, trust, instruction_safe, created_at,
                    updated_at, last_verified_at, metadata_json
                )
                SELECT id, scope, project_id, conversation_id, task_id, kind, subject, content,
                       confidence, source_count, status,
                       CASE WHEN kind='event' THEN 'observed' ELSE 'untrusted' END,
                       0, created_at, updated_at, last_verified_at, metadata_json
                FROM legacy.memory_items;

                INSERT OR IGNORE INTO memory_sources(item_id, source_id, evidence_json, created_at)
                SELECT item_id, source_id, evidence_json, created_at FROM legacy.memory_sources;

                INSERT OR IGNORE INTO graph_nodes(
                    id, node_type, scope, project_id, label, metadata_json, created_at, updated_at
                )
                SELECT id, type, scope, project_id, label, metadata_json, created_at, updated_at
                FROM legacy.graph_nodes;

                INSERT OR IGNORE INTO graph_edges(
                    id, from_id, relation, to_id, confidence, source_id, created_at
                )
                SELECT id, from_id, relation, to_id, confidence, source_id, created_at
                FROM legacy.graph_edges;
                COMMIT;
                "#,
            )?;
            Ok(())
        })();
        if operation.is_err() {
            let _ = guard.execute_batch("ROLLBACK;");
        }
        let detach = guard.execute_batch("DETACH DATABASE legacy;");
        operation?;
        detach?;
        let after: i64 =
            guard.query_row("SELECT COUNT(*) FROM memory_items", [], |row| row.get(0))?;
        Ok(serde_json::json!({
            "legacy_path": legacy_path.display().to_string(),
            "memory_items_before": before,
            "memory_items_after": after,
            "memory_items_imported": after.saturating_sub(before),
            "trust_policy": "events=observed; all other legacy memory=untrusted; instruction_safe=false"
        }))
    }

    pub fn counts(&self) -> StoreResult<Value> {
        self.with_connection(|connection| {
            let tables = [
                "task_records",
                "feedback_events",
                "episodes",
                "world_patterns",
                "memory_items",
                "graph_nodes",
                "graph_edges",
                "focus_records",
                "workflows",
                "capabilities",
                "audit_events",
            ];
            let mut map = serde_json::Map::new();
            for table in tables {
                let count: i64 =
                    connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })?;
                map.insert(table.to_owned(), Value::from(count));
            }
            Ok(Value::Object(map))
        })
    }

    fn upsert_simple<T: Serialize>(
        &self,
        table: &str,
        key_column: &str,
        key: &str,
        status: Option<&str>,
        payload: &T,
    ) -> StoreResult<()> {
        let payload = serde_json::to_string(payload)?;
        let sql = if status.is_some() {
            format!(
                "INSERT INTO {table}({key_column}, status, payload_json, updated_at) VALUES(?1, ?2, ?3, ?4) ON CONFLICT({key_column}) DO UPDATE SET status=excluded.status, payload_json=excluded.payload_json, updated_at=excluded.updated_at"
            )
        } else {
            format!(
                "INSERT INTO {table}({key_column}, payload_json, updated_at) VALUES(?1, ?2, ?3) ON CONFLICT({key_column}) DO UPDATE SET payload_json=excluded.payload_json, updated_at=excluded.updated_at"
            )
        };
        self.with_transaction(|tx| {
            if let Some(status) = status {
                tx.execute(&sql, params![key, status, payload, now_rfc3339()])?;
            } else {
                tx.execute(&sql, params![key, payload, now_rfc3339()])?;
            }
            Ok(())
        })
    }

    fn list_json<T: DeserializeOwned>(&self, table: &str) -> StoreResult<Vec<T>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(&format!("SELECT payload_json FROM {table}"))?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            let mut result = Vec::new();
            for row in rows {
                result.push(serde_json::from_str(&row?)?);
            }
            Ok(result)
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum JsonTable {
    Domain,
    Strategy,
    Focus,
    Workflow,
    Capability,
}

impl JsonTable {
    fn parts(self) -> (&'static str, &'static str) {
        match self {
            Self::Domain => ("domain_states", "state_key"),
            Self::Strategy => ("strategy_states", "state_key"),
            Self::Focus => ("focus_records", "task_id"),
            Self::Workflow => ("workflows", "workflow_id"),
            Self::Capability => ("capabilities", "name"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn database_is_initialized_and_counts_are_visible() {
        let directory = tempdir().unwrap();
        let store = CordisStore::open(directory.path().join("cordis.db")).unwrap();
        store.initialize().unwrap();
        let counts = store.counts().unwrap();
        assert_eq!(counts["task_records"], 0);
        assert_eq!(counts["audit_events"], 1);
    }

    #[test]
    fn task_ids_are_immutable_and_feedback_is_single_use() {
        let directory = tempdir().unwrap();
        let store = CordisStore::open(directory.path().join("cordis.db")).unwrap();
        store
            .insert_task(
                "t1",
                "p",
                "d",
                "s",
                &serde_json::json!({}),
                &serde_json::json!({}),
                &serde_json::json!({}),
            )
            .unwrap();
        assert!(matches!(
            store.insert_task(
                "t1",
                "p",
                "d",
                "s",
                &serde_json::json!({}),
                &serde_json::json!({}),
                &serde_json::json!({})
            ),
            Err(StoreError::AlreadyExists(_))
        ));
        store
            .finalize_task("t1", "f1", &serde_json::json!({"outcome":"failure"}))
            .unwrap();
        assert!(matches!(
            store.finalize_task("t1", "f2", &serde_json::json!({})),
            Err(StoreError::AlreadyFinalized(_))
        ));
    }
}
