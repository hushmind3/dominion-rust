use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::path::Path;
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

    pub fn audit(&self, event: &str, detail: Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO audit(id, event, detail) VALUES(?1, ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), event, detail.to_string()],
        );
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
        let _ = conn.execute(
            "INSERT INTO secrets(name, value) VALUES(?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value=excluded.value, updated_at=datetime('now')",
            params![name, sealed],
        );
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
mod tests {
    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("dominion.sqlite")).unwrap();
        (store, dir)
    }

    #[test]
    fn records_round_trip_with_client_fields() {
        let (store, dir) = temp_store();
        let saved = store
            .put(
                "provider",
                &json!({ "name": "Local", "baseUrl": "http://127.0.0.1:4399/v1" }),
            )
            .expect("saved");
        let id = saved["id"].as_str().unwrap().to_string();
        assert_eq!(saved["name"], "Local");
        assert!(saved.get("createdAt").is_some());

        let fetched = store.get("provider", &id).unwrap();
        assert_eq!(fetched["baseUrl"], "http://127.0.0.1:4399/v1");

        let updated = store
            .put(
                "provider",
                &json!({ "id": id, "name": "Renamed", "baseUrl": "http://x" }),
            )
            .unwrap();
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(store.all("provider").len(), 1);

        store.remove("provider", &id);
        assert!(store.get("provider", &id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn messages_preserve_reasoning_and_order() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi",
            "reasoning": "thought", "tool_calls": [{ "id": "c1" }]
        }));
        let messages = store.messages("s1");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["reasoning"], "thought");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        store.clear_messages("s1");
        assert!(store.messages("s1").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_workers_claim_a_job_only_once() {
        let (store, dir) = temp_store();
        store
            .put_if_absent("job", &json!({"id":"one","status":"queued"}))
            .unwrap();
        assert!(store
            .put_if_absent("job", &json!({"id":"one","status":"overwritten"}))
            .is_none());
        let workers: Vec<_> = (0..12)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .patch_if(
                            "job",
                            "one",
                            "status",
                            &json!("queued"),
                            &json!({"status":"running"}),
                        )
                        .is_some()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .filter_map(|worker| worker.join().ok())
                .filter(|claimed| *claimed)
                .count(),
            1
        );
        assert_eq!(store.get("job", "one").unwrap()["status"], "running");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_finalizations_merge_improvement_counters() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "improvement",
            &json!({
                "id":"imp-1","status":"adopted",
                "uses":5,"successes":5,"failures":0,"appliedExecutionIds":[]
            }),
        );
        for (execution_id, status) in [("exec-a", "validated"), ("exec-b", "adopted")] {
            let execution = json!({"id":execution_id});
            let marker = json!({
                "id": format!("execution:{execution_id}"),
                "payloadHash": format!("hash-{execution_id}"),
            });
            let improvement = json!({
                "id":"imp-1","uses":6,"successes":6,"failures":0,"status":status,
                "appliedExecutionIds":[execution_id],
                "__counterDelta":{"uses":1,"successes":1,"failures":0},
            });
            assert_eq!(
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap(),
                FinalizeOutcome::Applied
            );
        }
        let merged = store.get("improvement", "imp-1").unwrap();
        assert_eq!(merged["uses"], 7, "one observation was lost: {merged}");
        assert_eq!(merged["successes"], 7);
        assert_eq!(merged["appliedExecutionIds"].as_array().unwrap().len(), 2);
        assert_eq!(merged["status"], "validated");
        assert!(merged.get("__counterDelta").is_none());
    }

    #[test]
    fn owner_dispatch_is_atomic_and_metadata_is_preserved() {
        let (store, dir) = temp_store();
        let message = json!({"id":"m1","session_id":"room:r","role":"user","content":"내 지시","metadata":{"authorKind":"owner","roomId":"r"}});
        let job = json!({"id":"j1","kind":"room","status":"queued"});
        let (saved, _) = store
            .write_message_with_record(&message, "job", &job)
            .unwrap();
        assert_eq!(saved["metadata"]["authorKind"], "owner");
        assert_eq!(store.message("m1").unwrap()["content"], "내 지시");
        let mut conflicting = message.clone();
        conflicting["id"] = json!("m2");
        assert!(store
            .write_message_with_record(&conflicting, "job", &job)
            .is_none());
        assert!(
            store.message("m2").is_none(),
            "queue failure must roll back owner insertion"
        );
        assert_eq!(store.messages("room:r").len(), 1);
        store.put("agent", &json!({"id":"a","model":"old"}));
        assert!(store
            .patch_records(
                "agent",
                &["a".into(), "missing".into()],
                &json!({"model":"new"})
            )
            .is_none());
        assert_eq!(store.get("agent", "a").unwrap()["model"], "old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finalization_is_idempotent_and_conflict_aware() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-1","status":"completed"});
        let marker = json!({"id":"execution:exec-1","payloadHash":"hash-1"});
        let improvement = json!({"id":"imp-1","uses":0,"status":"adopted"});
        let observation = json!({"id":"obs-1"});
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::Applied
        );
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::AlreadyApplied
        );
        let conflicting = json!({"id":"execution:exec-1","payloadHash":"hash-2"});
        assert_eq!(
            store
                .finalize_once(&execution, &conflicting, &[], &[])
                .unwrap(),
            FinalizeOutcome::Conflict
        );
        assert_eq!(store.get("improvement", "imp-1").unwrap()["uses"], 0);
        assert_eq!(store.try_all("improvement").unwrap().len(), 1);
    }

    #[test]
    fn concurrent_finalizations_apply_once() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-2","status":"completed"});
        let marker = json!({"id":"execution:exec-2","payloadHash":"hash-2"});
        let improvement = json!({"id":"imp-2","uses":0});
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = store.clone();
            let execution = execution.clone();
            let marker = marker.clone();
            let improvement = improvement.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap()
            }));
        }
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.contains(&FinalizeOutcome::Applied));
        assert!(outcomes.contains(&FinalizeOutcome::AlreadyApplied));
        assert_eq!(store.try_all("execution_finalization").unwrap().len(), 1);
    }

    #[test]
    fn try_all_preserves_records_and_returns_them() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("memory", &json!({"id":"m1","text":"a"}));
        store.put("memory", &json!({"id":"m2","text":"b"}));
        let records = store.try_all("memory").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn broken_finalization_marker_is_an_error() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO records(kind,id,data) VALUES('execution_finalization','execution:broken','{bad')",
                [],
            )
            .unwrap();
        }
        let execution = json!({"id":"broken","status":"completed"});
        let marker = json!({"id":"execution:broken","payloadHash":"hash"});
        assert!(store.finalize_once(&execution, &marker, &[], &[]).is_err());
    }
}
