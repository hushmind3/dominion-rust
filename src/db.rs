use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalizeOutcome {
    Applied,
    AlreadyApplied,
    Conflict,
}

fn upsert_tx(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    value: &Value,
) -> Result<(), String> {
    let id = value["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("{kind} record has no id"))?;
    tx.execute(
        "INSERT INTO records(kind, id, data) VALUES(?1, ?2, ?3)
         ON CONFLICT(kind, id) DO UPDATE SET data=excluded.data, updated_at=datetime('now')",
        params![kind, id, value.to_string()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn upsert_secret(conn: &Connection, name: &str, sealed: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO secrets(name, value) VALUES(?1, ?2)
         ON CONFLICT(name) DO UPDATE SET value=excluded.value, updated_at=datetime('now')",
        params![name, sealed],
    )
}

/// Merges a computed improvement update into the record currently stored in
/// the finalization transaction. Observation counters are applied as deltas so
/// two attempts finishing concurrently cannot lose an increment.
fn merge_improvement_update(current: &Value, update: &Value) -> Value {
    let mut merged = current.clone();
    for counter in ["uses", "successes", "failures"] {
        let delta = update["__counterDelta"][counter].as_i64().unwrap_or(0);
        let base = current[counter].as_i64().unwrap_or(0);
        merged[counter] = json!(base.saturating_add(delta).max(0));
    }
    let mut applied = current["appliedExecutionIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for id in update["appliedExecutionIds"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !applied.contains(id) {
            applied.push(id.clone());
        }
    }
    if applied.len() > 200 {
        applied = applied.split_off(applied.len() - 200);
    }
    merged["appliedExecutionIds"] = json!(applied);
    if update.get("lastCompliance").is_some() {
        merged["lastCompliance"] = update["lastCompliance"].clone();
    }
    if update.get("lastObservedAt").is_some() {
        merged["lastObservedAt"] = update["lastObservedAt"].clone();
    }
    let rank = |status: &str| match status {
        "rolled_back" => 2,
        "validated" => 1,
        _ => 0,
    };
    let update_status = update["status"].as_str().unwrap_or("");
    let current_status = current["status"].as_str().unwrap_or("");
    if rank(update_status) > rank(current_status) {
        for field in [
            "status",
            "evidenceGrade",
            "validatedAt",
            "rolledBackAt",
            "reason",
        ] {
            if update.get(field).is_some() {
                merged[field] = update[field].clone();
            }
        }
    }
    merged
}

/// Durable store. Records are schemaless JSON documents grouped by `kind`,
/// which is exactly the contract the React client already speaks.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS records(
                 kind TEXT NOT NULL,
                 id TEXT NOT NULL,
                 data TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now')),
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                 PRIMARY KEY(kind, id)
             );
             CREATE TABLE IF NOT EXISTS messages(
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL DEFAULT '',
                 reasoning TEXT NOT NULL DEFAULT '',
                 agent_id TEXT,
                 tool_calls TEXT,
                 tool_call_id TEXT,
                 metadata TEXT NOT NULL DEFAULT '{}',
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);
             CREATE TABLE IF NOT EXISTS audit(
                 id TEXT PRIMARY KEY,
                 event TEXT NOT NULL,
                 detail TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS secrets(
                 name TEXT PRIMARY KEY,
                 value TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS event_log(
                 seq INTEGER PRIMARY KEY,
                 data TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE IF NOT EXISTS adaptation_schema(
                 version INTEGER PRIMARY KEY,
                 applied_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS adaptation_finalizations(
                 attempt_id TEXT NOT NULL,
                 evaluation_revision TEXT NOT NULL,
                 payload_hash TEXT NOT NULL,
                 applied_at TEXT NOT NULL,
                 PRIMARY KEY(attempt_id, evaluation_revision)
             );
             CREATE TABLE IF NOT EXISTS adaptation_updates(
                 observation_id TEXT NOT NULL,
                 projection_version TEXT NOT NULL,
                 PRIMARY KEY(observation_id, projection_version)
             );
             CREATE INDEX IF NOT EXISTS idx_records_kind_updated
                 ON records(kind, updated_at DESC);
             CREATE INDEX IF NOT EXISTS idx_operation_run
                 ON records(json_extract(data,'$.runId'))
                 WHERE kind='execution_operation';
             CREATE INDEX IF NOT EXISTS idx_execution_task
                 ON records(json_extract(data,'$.taskId'))
                 WHERE kind='execution';",
        )?;
        // Older databases predate reasoning/tool columns.
        for (column, decl) in [
            ("reasoning", "TEXT NOT NULL DEFAULT ''"),
            ("agent_id", "TEXT"),
            ("tool_calls", "TEXT"),
            ("tool_call_id", "TEXT"),
            ("metadata", "TEXT NOT NULL DEFAULT '{}'"),
        ] {
            let _ = conn.execute(
                &format!("ALTER TABLE messages ADD COLUMN {column} {decl}"),
                [],
            );
        }
        conn.execute(
            "INSERT OR IGNORE INTO adaptation_schema(version, applied_at) VALUES(1, datetime('now'))",
            [],
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        })
    }

    pub fn all(&self, kind: &str) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 ORDER BY updated_at DESC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([kind], record_from_row) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    pub fn execution_operations(&self, run_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT data, id, created_at, updated_at FROM records
             WHERE kind='execution_operation' AND json_extract(data,'$.runId')=?1
             ORDER BY CAST(json_extract(data,'$.opSeq') AS INTEGER), id",
        ).map_err(|error| error.to_string())?;
        let rows = statement.query_map([run_id], record_from_row).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }

    /// Error-preserving variant for the new learning/finalization paths.
    /// `all` remains for React-facing compatibility, but new paths must not
    /// interpret a database error as an empty collection.
    pub fn try_all(&self, kind: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn
            .prepare(
                "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 ORDER BY updated_at DESC",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([kind], record_from_row)
            .map_err(|error| error.to_string())?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|error| error.to_string())?);
        }
        Ok(records)
    }

    pub fn count(&self, kind: &str) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM records WHERE kind=?1",
            [kind],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    pub fn get(&self, kind: &str, id: &str) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            record_from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn put(&self, kind: &str, value: &Value) -> Option<Value> {
        let object = value.as_object()?;
        let id = object
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut data = value.clone();
        data["id"] = json!(id);
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO records(kind, id, data) VALUES(?1, ?2, ?3)
                 ON CONFLICT(kind, id) DO UPDATE SET data=excluded.data, updated_at=datetime('now')",
                params![kind, id, data.to_string()],
            )
            .ok()?;
        }
        self.get(kind, &id)
    }

    pub fn save_channel(&self, value: &Value, secret: Option<(&str, &str)>) -> Option<Value> {
        let mut conn = self.conn.lock().ok()?;
        let tx = conn.transaction().ok()?;
        upsert_tx(&tx, "channel", value).ok()?;
        if let Some((name, sealed)) = secret {
            upsert_secret(&tx, name, sealed).ok()?;
        }
        let saved = tx.query_row(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind='channel' AND id=?1",
            params![value["id"].as_str()?],
            record_from_row,
        ).ok()?;
        tx.commit().ok()?;
        Some(saved)
    }

    /// Writes several records in one SQLite transaction. Any failure rolls
    /// back all of them, which is required for experiment promotion.
    pub fn transaction(&self, records: &[(String, Value)]) -> Result<Vec<Value>, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let mut saved = Vec::new();
        for (kind, value) in records {
            upsert_tx(&tx, kind, value)?;
            saved.push(value.clone());
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(saved)
    }

    /// Transaction that only commits when the current record still has the
    /// expected status value. Returns false on a CAS miss.
    pub fn transaction_if_status(
        &self,
        kind: &str,
        id: &str,
        expected: &str,
        records: &[(String, Value)],
    ) -> Result<bool, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let status = current
            .and_then(|data| serde_json::from_str::<Value>(&data).ok())
            .and_then(|value| value["status"].as_str().map(str::to_string));
        if status.as_deref() != Some(expected) {
            return Ok(false);
        }
        for (record_kind, value) in records {
            upsert_tx(&tx, record_kind, value)?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// Durable idempotency key. Unlike `put`, this never overwrites a record.
    pub fn put_if_absent(&self, kind: &str, value: &Value) -> Option<Value> {
        let id = value.get("id")?.as_str().filter(|id| !id.is_empty())?;
        let conn = self.conn.lock().unwrap();
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO records(kind, id, data) VALUES(?1, ?2, ?3)",
                params![kind, id, value.to_string()],
            )
            .ok()?;
        if changed != 1 {
            return None;
        }
        conn.query_row(
            "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            record_from_row,
        )
        .ok()
    }

    /// Atomically creates the root episode when absent, inserts the execution
    /// and appends the attempt link. A crash cannot leave an execution
    /// detached from its task, and retrying the same attempt is idempotent.
    pub fn create_attempt(
        &self,
        root: &Value,
        execution: &Value,
        attempt: &Value,
    ) -> Option<Value> {
        let root_id = root["id"].as_str()?.to_string();
        let execution_id = execution["id"].as_str()?.to_string();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().ok()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind='task_episode' AND id=?1",
                [&root_id],
                |row| row.get(0),
            )
            .optional()
            .ok()?;
        let mut episode = match existing {
            Some(text) => serde_json::from_str(&text).ok()?,
            None => {
                upsert_tx(&tx, "task_episode", root).ok()?;
                root.clone()
            }
        };
        upsert_tx(&tx, "execution", execution).ok()?;
        let attempts = episode["attempts"].as_array_mut()?;
        if !attempts
            .iter()
            .any(|item| item["executionId"].as_str() == Some(execution_id.as_str()))
        {
            attempts.push(attempt.clone());
        }
        upsert_tx(&tx, "task_episode", &episode).ok()?;
        tx.commit().ok()?;
        drop(conn);
        self.get("execution", &execution_id)
    }

    /// Compare-and-patch under the connection lock, used to claim queued work.
    pub fn patch_if(
        &self,
        kind: &str,
        id: &str,
        field: &str,
        expected: &Value,
        patch: &Value,
    ) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut record = conn
            .query_row(
                "SELECT data, id, created_at, updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                record_from_row,
            )
            .ok()?;
        if record.get(field).unwrap_or(&Value::Null) != expected {
            return None;
        }
        for (key, value) in patch.as_object()? {
            if key != "id" {
                record[key] = value.clone();
            }
        }
        conn.execute(
            "UPDATE records SET data=?3, updated_at=datetime('now') WHERE kind=?1 AND id=?2",
            params![kind, id, record.to_string()],
        )
        .ok()?;
        Some(record)
    }

    pub fn patch_records(&self, kind: &str, ids: &[String], patch: &Value) -> Option<Vec<Value>> {
        let patch = patch.as_object()?;
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn.transaction().ok()?;
        let mut records = Vec::new();
        for id in ids {
            let mut record = transaction
                .query_row(
                    "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                    params![kind, id],
                    record_from_row,
                )
                .ok()?;
            for (key, value) in patch {
                if key != "id" {
                    record[key] = value.clone();
                }
            }
            transaction
                .execute(
                    "UPDATE records SET data=?3,updated_at=datetime('now') WHERE kind=?1 AND id=?2",
                    params![kind, id, record.to_string()],
                )
                .ok()?;
            records.push(record);
        }
        transaction.commit().ok()?;
        Some(records)
    }

    /// Increment evidence counters under the same lock as their read. Parallel
    /// workers must not lose updates through separate get/put operations.
    pub fn increment_counters(&self, kind: &str, id: &str, fields: &[&str]) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut record = conn
            .query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                record_from_row,
            )
            .optional()
            .ok()?
            .unwrap_or_else(|| json!({"id":id}));
        for field in fields {
            record[*field] = json!(record[*field].as_u64().unwrap_or(0).saturating_add(1));
        }
        record["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        conn.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')",params![kind,id,record.to_string()]).ok()?;
        conn.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
            record_from_row,
        )
        .ok()
    }

    /// Atomically writes the terminal execution, improvement updates,
    /// observations and finalization marker. Duplicate identical payloads are
    /// AlreadyApplied; a different payload for the same attempt is a Conflict.
    pub fn finalize_once(
        &self,
        execution: &Value,
        marker: &Value,
        improvements: &[Value],
        observations: &[Value],
    ) -> Result<FinalizeOutcome, String> {
        let execution_id = execution["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("finalization execution id is missing")?;
        let marker_id = marker["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("finalization marker id is missing")?;
        let payload_hash = marker["payloadHash"]
            .as_str()
            .ok_or("finalization payload hash is missing")?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind='execution_finalization' AND id=?1",
                [marker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(existing) = existing {
            let value: Value = serde_json::from_str(&existing)
                .map_err(|error| format!("broken finalization marker: {error}"))?;
            if value["payloadHash"].as_str() != Some(payload_hash) {
                return Ok(FinalizeOutcome::Conflict);
            }
            if value["statsApplied"].as_bool() == Some(true) {
                return Ok(FinalizeOutcome::AlreadyApplied);
            }
        }
        upsert_tx(&tx, "execution", execution)?;
        for improvement in improvements {
            let id = improvement["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("improvement id is missing")?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT data FROM records WHERE kind='improvement' AND id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let mut record = match existing {
                Some(text) => {
                    let current: Value = serde_json::from_str(&text)
                        .map_err(|error| format!("broken improvement record: {error}"))?;
                    merge_improvement_update(&current, improvement)
                }
                None => improvement.clone(),
            };
            record["id"] = json!(id);
            if let Some(object) = record.as_object_mut() {
                object.remove("__counterDelta");
            }
            upsert_tx(&tx, "improvement", &record)?;
        }
        for observation in observations {
            upsert_tx(&tx, "strategy_observation", observation)?;
        }
        let mut applied_marker = marker.clone();
        applied_marker["statsApplied"] = json!(true);
        applied_marker["appliedAt"] = json!(chrono::Utc::now().to_rfc3339());
        upsert_tx(&tx, "execution_finalization", &applied_marker)?;
        tx.execute(
            "INSERT OR IGNORE INTO adaptation_finalizations(attempt_id, evaluation_revision, payload_hash, applied_at)
             VALUES(?1, ?2, ?3, datetime('now'))",
            params![execution_id, "evaluator:finalize-semantic-v1", payload_hash],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(FinalizeOutcome::Applied)
    }

    pub fn remove(&self, kind: &str, id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM records WHERE kind=?1 AND id=?2",
            params![kind, id],
        )
        .map(|changed| changed > 0)
        .unwrap_or(false)
    }

    pub fn write_message(&self, message: &Value) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let id = insert_message(&conn, message)?;
        query_message(&conn, &id)
    }

    pub fn message(&self, id: &str) -> Option<Value> {
        query_message(&self.conn.lock().unwrap(), id)
    }

    /// A human room message and its dispatch job are one durable operation.
    /// Insert-only IDs make retries observable and never overwrite prior work.
    pub fn write_message_with_record(
        &self,
        message: &Value,
        kind: &str,
        record: &Value,
    ) -> Option<(Value, Value)> {
        let record_id = record.get("id")?.as_str().filter(|id| !id.is_empty())?;
        record.as_object()?;
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn.transaction().ok()?;
        let message_id = insert_message(&transaction, message)?;
        transaction
            .execute(
                "INSERT INTO records(kind,id,data) VALUES(?1,?2,?3)",
                params![kind, record_id, record.to_string()],
            )
            .ok()?;
        let saved_message = query_message(&transaction, &message_id)?;
        let saved_record = transaction
            .query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind, record_id],
                record_from_row,
            )
            .ok()?;
        transaction.commit().ok()?;
        Some((saved_message, saved_record))
    }

    pub fn messages(&self, session_id: &str) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY created_at ASC,rowid ASC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([session_id], message_from_row) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    /// Error-preserving variant for model-context assembly. A database failure
    /// must abort the turn, not look like a session with no history.
    pub fn try_messages(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn
            .prepare(
                "SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE session_id=?1 ORDER BY created_at ASC,rowid ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([session_id], message_from_row)
            .map_err(|error| error.to_string())?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row.map_err(|error| error.to_string())?);
        }
        Ok(messages)
    }

    /// Increment a durable cancellation epoch under the same SQLite lock.
    pub fn cancel_session_epoch(&self, session_id: &str) -> Option<u64> {
        let conn = self.conn.lock().unwrap();
        let prior = conn
            .query_row(
                "SELECT data FROM records WHERE kind='session_cancel' AND id=?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()?;
        let prior = prior
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or(Value::Null);
        let generation = prior["generation"].as_u64().unwrap_or(0).checked_add(1)?;
        let data = json!({"id":session_id,"generation":generation,"cancelled":true,"cancelledAt":chrono::Utc::now().to_rfc3339()});
        conn.execute("INSERT INTO records(kind,id,data) VALUES('session_cancel',?1,?2) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')",params![session_id,data.to_string()]).ok()?;
        Some(generation)
    }

    pub fn resume_session_epoch(&self, session_id: &str, expected: u64) -> bool {
        let conn = self.conn.lock().unwrap();
        let prior = match conn
            .query_row(
                "SELECT data FROM records WHERE kind='session_cancel' AND id=?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        {
            Ok(prior) => prior,
            Err(_) => return false,
        };
        let mut data = prior
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or_else(|| json!({"id":session_id,"generation":0}));
        if data["generation"].as_u64().unwrap_or(0) != expected {
            return false;
        }
        data["cancelled"] = json!(false);
        conn.execute("INSERT INTO records(kind,id,data) VALUES('session_cancel',?1,?2) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')",params![session_id,data.to_string()]).is_ok()
    }

    pub fn clear_messages(&self, session_id: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM messages WHERE session_id=?1", [session_id]);
    }

    /// Durable, ordered event log backing `/api/events`. Bounded by trimming
    /// old rows in the background of normal writes so a slow or disconnected
    /// client cannot lose ordering entirely or grow the database forever.
    /// Cross-restart run lease. The owner instance must keep renewing it; a
    /// crashed process leaves a lease that startup recovery releases.
    pub fn acquire_lease(&self, id: &str, owner: &str, ttl_ms: i64) -> Option<Value> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().ok()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT data FROM records WHERE kind='run_lease' AND id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .ok()?;
        if let Some(text) = existing {
            let value: Value = serde_json::from_str(&text).ok()?;
            let active = value["status"].as_str() == Some("active");
            let same_owner = value["owner"].as_str() == Some(owner);
            let expires = value["expiresAtMs"].as_i64().unwrap_or(0);
            if active && !same_owner && expires > now {
                return None;
            }
        }
        let lease = json!({
            "id": id,
            "owner": owner,
            "status": "active",
            "acquiredAtMs": now,
            "heartbeatAtMs": now,
            "expiresAtMs": now.saturating_add(ttl_ms),
        });
        upsert_tx(&tx, "run_lease", &lease).ok()?;
        tx.commit().ok()?;
        Some(lease)
    }

    pub fn renew_lease(&self, id: &str, owner: &str, ttl_ms: i64) -> Option<Value> {
        let now = chrono::Utc::now().timestamp_millis();
        self.patch_if(
            "run_lease",
            id,
            "owner",
            &json!(owner),
            &json!({
                "status": "active",
                "heartbeatAtMs": now,
                "expiresAtMs": now.saturating_add(ttl_ms),
            }),
        )
    }

    pub fn release_lease(&self, id: &str, owner: &str) -> bool {
        let patched = self
            .patch_if(
                "run_lease",
                id,
                "owner",
                &json!(owner),
                &json!({"status": "released"}),
            )
            .is_some();
        if patched {
            self.remove("run_lease", id);
        }
        patched
    }

    /// Startup recovery. This process owns the database (duplicate launches are
    /// rejected before migrations), so any other owner or expired lease is dead.
    pub fn release_stale_leases(&self, current_owner: &str) -> usize {
        let now = chrono::Utc::now().timestamp_millis();
        let mut released = 0;
        for lease in self.all("run_lease") {
            let Some(id) = lease["id"].as_str() else {
                continue;
            };
            let owner = lease["owner"].as_str().unwrap_or("");
            let expires = lease["expiresAtMs"].as_i64().unwrap_or(0);
            if owner != current_owner || expires <= now {
                if self.remove("run_lease", id) {
                    released += 1;
                }
            }
        }
        released
    }

    #[allow(dead_code)] // used by lease tests and operational inspection
    pub fn active_lease(&self, id: &str) -> Option<Value> {
        self.get("run_lease", id)
            .filter(|lease| lease["status"].as_str() == Some("active"))
    }

    /// Replaces the live database with a consistent snapshot. Callers must put
    /// the engine into maintenance mode first; the connection is swapped under
    /// the same mutex so no query observes a half-written file.
    pub fn replace_database(&self, bytes: &[u8]) -> Result<(), String> {
        let temp = self.path.with_extension("restore.tmp");
        std::fs::write(&temp, bytes).map_err(|error| error.to_string())?;
        let mut guard = self.conn.lock().map_err(|error| error.to_string())?;
        let _ = guard.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        // Close the old handle before copying over the database file.
        *guard = Connection::open_in_memory().map_err(|error| error.to_string())?;
        let wal = PathBuf::from(format!("{}-wal", self.path.display()));
        let shm = PathBuf::from(format!("{}-shm", self.path.display()));
        let _ = std::fs::remove_file(wal);
        let _ = std::fs::remove_file(shm);
        std::fs::copy(&temp, &self.path).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_file(&temp);
        *guard = Connection::open(&self.path).map_err(|error| error.to_string())?;
        let _ = guard.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        );
        Ok(())
    }

    /// Consistent online snapshot for a restore drill. `VACUUM INTO` includes
    /// committed WAL content and never exposes a half-written database.
    pub fn backup_to(&self, path: &Path) -> Result<u64, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let _ = std::fs::remove_file(path);
        let target = path.to_string_lossy().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM INTO ?1", params![target])
            .map_err(|error| format!("backup failed: {error}"))?;
        drop(conn);
        std::fs::metadata(path)
            .map(|meta| meta.len())
            .map_err(|error| error.to_string())
    }

    pub fn append_event(&self, seq: u64, value: &Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO event_log(seq, data) VALUES(?1, ?2)",
            params![seq as i64, value.to_string()],
        );
        if seq % 256 == 0 {
            let cutoff = seq.saturating_sub(2_000) as i64;
            if cutoff > 0 {
                let _ = conn.execute(
                    "DELETE FROM event_log WHERE seq <= ?1",
                    params![cutoff],
                );
            }
        }
    }

    pub fn events_after(&self, after: u64, limit: usize) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT seq, data FROM event_log WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(
            params![after as i64, limit.min(1_000) as i64],
            |row| {
                let seq: i64 = row.get(0)?;
                let data: String = row.get(1)?;
                Ok((seq, data))
            },
        );
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|row| {
            let (seq, data) = row.ok()?;
            let mut value: Value = serde_json::from_str(&data).ok()?;
            value["seq"] = json!(seq as u64);
            Some(value)
        })
        .collect()
    }

    pub fn last_event_seq(&self) -> u64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM event_log", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| value.max(0) as u64)
        .unwrap_or(0)
    }

    pub fn audit(&self, event: &str, detail: Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO audit(id, event, detail) VALUES(?1, ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), event, detail.to_string()],
        );
    }

    /// Aggregates recorded model usage by provider/model/purpose. Streaming
    /// calls may only know a total; `estimatedCalls` keeps that distinction.
    pub fn usage_summary(&self) -> Value {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT detail FROM audit WHERE event='model.usage' ORDER BY rowid DESC LIMIT 5000",
        ) else {
            return json!({"calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,"groups":[]});
        };
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        let Ok(rows) = rows else {
            return json!({"calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0,"groups":[]});
        };
        let mut groups: std::collections::HashMap<String, Value> =
            std::collections::HashMap::new();
        let mut calls = 0u64;
        let mut prompt = 0u64;
        let mut completion = 0u64;
        let mut total = 0u64;
        let mut estimated = 0u64;
        for row in rows.flatten() {
            let Ok(detail) = serde_json::from_str::<Value>(&row) else {
                continue;
            };
            let provider = detail["providerId"].as_str().unwrap_or("");
            let model = detail["model"].as_str().unwrap_or("");
            let purpose = detail["purpose"].as_str().unwrap_or("");
            let key = format!("{provider}\u{1f}{model}\u{1f}{purpose}");
            let entry = groups.entry(key).or_insert_with(|| {
                json!({
                    "providerId": provider,
                    "model": model,
                    "purpose": purpose,
                    "calls": 0,
                    "promptTokens": 0,
                    "completionTokens": 0,
                    "totalTokens": 0,
                    "estimatedCalls": 0,
                })
            });
            calls += 1;
            let prompt_tokens = detail["promptTokens"].as_u64().unwrap_or(0);
            let completion_tokens = detail["completionTokens"].as_u64().unwrap_or(0);
            let total_tokens = detail["totalTokens"].as_u64().unwrap_or(0);
            prompt += prompt_tokens;
            completion += completion_tokens;
            total += total_tokens;
            entry["calls"] = json!(entry["calls"].as_u64().unwrap_or(0) + 1);
            entry["promptTokens"] =
                json!(entry["promptTokens"].as_u64().unwrap_or(0) + prompt_tokens);
            entry["completionTokens"] =
                json!(entry["completionTokens"].as_u64().unwrap_or(0) + completion_tokens);
            entry["totalTokens"] =
                json!(entry["totalTokens"].as_u64().unwrap_or(0) + total_tokens);
            if detail["estimated"].as_bool() == Some(true) {
                estimated += 1;
                entry["estimatedCalls"] =
                    json!(entry["estimatedCalls"].as_u64().unwrap_or(0) + 1);
            }
        }
        let mut groups = groups.into_values().collect::<Vec<_>>();
        groups.sort_by(|a, b| {
            b["totalTokens"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["totalTokens"].as_u64().unwrap_or(0))
                .then_with(|| a["providerId"].as_str().cmp(&b["providerId"].as_str()))
        });
        json!({
            "calls": calls,
            "promptTokens": prompt,
            "completionTokens": completion,
            "totalTokens": total,
            "estimatedCalls": estimated,
            "groups": groups,
        })
    }

    /// Token totals for one session, including calls made before usage was
    /// attached to messages. Audit is the durable source of truth.
    pub fn session_model_routes(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT DISTINCT json_extract(detail,'$.providerId'), json_extract(detail,'$.model') FROM audit WHERE event='model.usage' AND json_extract(detail,'$.sessionId')=?1").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([session_id],|row|Ok(json!({"providerId":row.get::<_,String>(0)?,"model":row.get::<_,String>(1)?}))).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }

    /// Original, append-only provider+model audit. A model choice can earn
    /// downstream RSI credit only when this EXACT route actually served the
    /// generation session. Zero-token and mixed-route sessions cannot win.
    pub fn api_route_usage(&self,session_id:&str,provider:&str,model:&str)->Value {
        let conn=self.conn.lock().unwrap();
        let result=conn.query_row(
            "SELECT COUNT(*),
                COALESCE(SUM(CAST(json_extract(detail,'$.totalTokens') AS INTEGER)),0),
                COALESCE(SUM(CASE WHEN json_extract(detail,'$.estimated')=1
                    THEN 1 ELSE 0 END),0)
             FROM audit WHERE event='model.usage'
                AND json_extract(detail,'$.sessionId')=?1
                AND json_extract(detail,'$.providerId')=?2
                AND json_extract(detail,'$.model')=?3
                AND CAST(json_extract(detail,'$.totalTokens') AS INTEGER)>0",
            params![session_id,provider,model],|row|{
                Ok((row.get::<_,i64>(0)?,row.get::<_,i64>(1)?,
                    row.get::<_,i64>(2)?))
            });
        match result{
            Ok((calls,tokens,estimated))=>json!({
                "calls":calls.max(0),"totalTokens":tokens.max(0),
                "estimatedCalls":estimated.max(0),
                "matchedExactProviderModelSession":calls>0 && tokens>0
            }),
            Err(_)=>json!({"calls":0,"totalTokens":0,"estimatedCalls":0,
                "matchedExactProviderModelSession":false})
        }
    }

    pub fn session_usage(&self, session_id: &str) -> Value {
        let conn = self.conn.lock().unwrap();
        let summary = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CAST(json_extract(detail,'$.promptTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.completionTokens') AS INTEGER)),0),
                    COALESCE(SUM(CAST(json_extract(detail,'$.totalTokens') AS INTEGER)),0),
                    COALESCE(SUM(CASE WHEN json_extract(detail,'$.estimated')=1 THEN 1 ELSE 0 END),0)
             FROM audit
             WHERE event='model.usage' AND json_extract(detail,'$.sessionId')=?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        );
        match summary {
            Ok((calls, prompt, completion, total, estimated)) => json!({
                "calls": calls.max(0),
                "promptTokens": prompt.max(0),
                "completionTokens": completion.max(0),
                "totalTokens": total.max(0),
                "estimatedCalls": estimated.max(0),
            }),
            Err(_) => json!({
                "calls":0,"promptTokens":0,"completionTokens":0,"totalTokens":0,"estimatedCalls":0
            }),
        }
    }

    pub fn recent_audit(&self, limit: usize) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn
            .prepare("SELECT id, event, detail, created_at FROM audit ORDER BY rowid DESC LIMIT ?1")
        {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([limit.min(500) as i64], |row| {
            let detail: String = row.get(2)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "event": row.get::<_, String>(1)?,
                "detail": serde_json::from_str::<Value>(&detail).unwrap_or(Value::Null),
                "createdAt": row.get::<_, String>(3)?,
            }))
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    pub fn set_secret(&self, name: &str, sealed: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = upsert_secret(&conn, name, sealed);
    }

    pub fn get_secret(&self, name: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM secrets WHERE name=?1", [name], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn all_secrets(&self) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT name, value FROM secrets ORDER BY name") else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        });
        rows.map(|rows| rows.filter_map(|row| row.ok()).collect())
            .unwrap_or_default()
    }

    pub fn has_secret(&self, name: &str) -> bool {
        self.get_secret(name).is_some()
    }

    pub fn remove_secret(&self, name: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM secrets WHERE name=?1", [name]);
    }
}

fn insert_message(conn: &Connection, message: &Value) -> Option<String> {
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let session_id = message.get("session_id")?.as_str()?;
    let role = message.get("role")?.as_str()?;
    let calls = message
        .get("tool_calls")
        .filter(|value| !value.is_null())
        .map(Value::to_string);
    let metadata = message
        .get("metadata")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    conn.execute("INSERT INTO messages(id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,metadata) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![
        id,session_id,role,message["content"].as_str().unwrap_or(""),message["reasoning"].as_str().unwrap_or(""),message["agent_id"].as_str(),calls,message["tool_call_id"].as_str(),metadata.to_string()
    ]).ok()?;
    Some(id)
}

fn query_message(conn: &Connection, id: &str) -> Option<Value> {
    conn.query_row("SELECT id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id,created_at,metadata FROM messages WHERE id=?1",[id],message_from_row).ok()
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let calls: Option<String> = row.get(6)?;
    let metadata: String = row.get(9)?;
    Ok(
        json!({"id":row.get::<_,String>(0)?,"session_id":row.get::<_,String>(1)?,"role":row.get::<_,String>(2)?,"content":row.get::<_,String>(3)?,"reasoning":row.get::<_,String>(4)?,"agent_id":row.get::<_,Option<String>>(5)?,"tool_calls":calls.and_then(|value|serde_json::from_str::<Value>(&value).ok()),"tool_call_id":row.get::<_,Option<String>>(7)?,"created_at":row.get::<_,String>(8)?,"metadata":serde_json::from_str::<Value>(&metadata).unwrap_or_else(|_|json!({}))}),
    )
}

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let data: String = row.get(0)?;
    let id: String = row.get(1)?;
    let created_at: String = row.get(2)?;
    let updated_at: String = row.get(3)?;
    let mut value: Value = serde_json::from_str(&data).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut map) = value {
        map.insert("id".into(), json!(id));
        map.insert("createdAt".into(), json!(created_at));
        map.insert("updatedAt".into(), json!(updated_at));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "db/tests.rs"]
mod tests;

