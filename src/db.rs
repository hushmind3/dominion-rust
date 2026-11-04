use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

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
             );",
        )?;
        // Older databases predate reasoning/tool columns.
        for (column, decl) in [
            ("reasoning", "TEXT NOT NULL DEFAULT ''"),
            ("agent_id", "TEXT"),
            ("tool_calls", "TEXT"),
            ("tool_call_id", "TEXT"),
        ] {
            let _ = conn.execute(
                &format!("ALTER TABLE messages ADD COLUMN {column} {decl}"),
                [],
            );
        }
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

    pub fn count(&self, kind: &str) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM records WHERE kind=?1", [kind], |row| {
            row.get::<_, i64>(0)
        })
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
        let id = message
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let session_id = message.get("session_id")?.as_str()?.to_string();
        let role = message.get("role")?.as_str()?.to_string();
        let content = message
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let reasoning = message
            .get("reasoning")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let agent_id = message
            .get("agent_id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let tool_calls = message
            .get("tool_calls")
            .filter(|value| !value.is_null())
            .map(|value| value.to_string());
        let tool_call_id = message
            .get("tool_call_id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO messages(id, session_id, role, content, reasoning, agent_id, tool_calls, tool_call_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, session_id, role, content, reasoning, agent_id, tool_calls, tool_call_id],
            )
            .ok()?;
        }
        self.messages(&session_id)
            .into_iter()
            .find(|item| item.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
    }

    pub fn messages(&self, session_id: &str) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, session_id, role, content, reasoning, agent_id, tool_calls, tool_call_id, created_at
             FROM messages WHERE session_id=?1 ORDER BY created_at ASC, rowid ASC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([session_id], |row| {
            let tool_calls: Option<String> = row.get(6)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "session_id": row.get::<_, String>(1)?,
                "role": row.get::<_, String>(2)?,
                "content": row.get::<_, String>(3)?,
                "reasoning": row.get::<_, String>(4)?,
                "agent_id": row.get::<_, Option<String>>(5)?,
                "tool_calls": tool_calls.and_then(|text| serde_json::from_str::<Value>(&text).ok()),
                "tool_call_id": row.get::<_, Option<String>>(7)?,
                "created_at": row.get::<_, String>(8)?,
            }))
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    pub fn clear_messages(&self, session_id: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM messages WHERE session_id=?1", [session_id]);
    }

    pub fn audit(&self, event: &str, detail: Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO audit(id, event, detail) VALUES(?1, ?2, ?3)",
            params![
                uuid::Uuid::new_v4().to_string(),
                event,
                detail.to_string()
            ],
        );
    }

    pub fn recent_audit(&self, limit: usize) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, event, detail, created_at FROM audit ORDER BY rowid DESC LIMIT ?1",
        ) {
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
            .put("provider", &json!({ "name": "Local", "baseUrl": "http://127.0.0.1:4399/v1" }))
            .expect("saved");
        let id = saved["id"].as_str().unwrap().to_string();
        assert_eq!(saved["name"], "Local");
        assert!(saved.get("createdAt").is_some());

        let fetched = store.get("provider", &id).unwrap();
        assert_eq!(fetched["baseUrl"], "http://127.0.0.1:4399/v1");

        let updated = store
            .put("provider", &json!({ "id": id, "name": "Renamed", "baseUrl": "http://x" }))
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
}
