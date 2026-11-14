//! Durable goal-session accounting across delegated descendants.
//! The underlying Rust execution engine (not a client SDK) owns these totals.
//! This is observed provider usage, not a prepaid reservation or billing cap.
use super::*;
use std::collections::HashSet;

impl Store {
    /// Resolve the owner goal through persisted parent-session edges. A job's
    /// model-provided payload must not be trusted to declare a different goal.
    pub fn enclosing_goal(&self, session_id: &str) -> Option<(String, Value)> {
        let mut cursor = session_id.to_owned();
        let mut seen = HashSet::new();
        for _ in 0..32 {
            if cursor.is_empty() || !seen.insert(cursor.clone()) {
                return None;
            }
            let session = self.get("session", &cursor)?;
            if let Some(goal_id) = session["goalId"].as_str().filter(|id| !id.is_empty()) {
                let goal = self.get("goal", goal_id)?;
                // The goal must still own the encountered root; a stale or
                // forged session pointer must not give it another budget.
                if goal["sessionId"].as_str() == Some(cursor.as_str()) {
                    return Some((cursor, goal));
                }
                return None;
            }
            cursor = session["parentSessionId"].as_str()?.to_owned();
        }
        None
    }

    /// One SQLite snapshot for root + all durable delegated descendant
    /// sessions. The recursion has cycle detection and a 32-edge bound
    /// (delegation itself is limited to 4). Unrelated sessions are excluded.
    /// Errors are surfaced: a database failure must not silently look like
    /// zero token usage to the budget controller.
    pub fn session_tree_usage(&self, root_session_id: &str) -> Result<Value, String> {
        if root_session_id.is_empty() {
            return Err("goal session id is empty".into());
        }
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let (calls, prompt, completion, total, estimated, cached, eligible, measured, sessions):
            (i64, i64, i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
            r#"WITH RECURSIVE family(id, depth, path) AS (
                 SELECT CAST(?1 AS TEXT), 0, '|' || CAST(?1 AS TEXT) || '|'
                 UNION ALL
                 SELECT child.id, family.depth + 1, family.path || child.id || '|'
                   FROM records child JOIN family
                     ON child.kind='session'
                    AND json_extract(child.data,'$.parentSessionId')=family.id
                  WHERE family.depth < 32
                    AND instr(family.path, '|' || child.id || '|')=0
               )
               SELECT COUNT(*),
                      COALESCE(SUM(CAST(json_extract(detail,'$.promptTokens') AS INTEGER)),0),
                      COALESCE(SUM(CAST(json_extract(detail,'$.completionTokens') AS INTEGER)),0),
                      COALESCE(SUM(CAST(json_extract(detail,'$.totalTokens') AS INTEGER)),0),
                      COALESCE(SUM(CASE WHEN json_extract(detail,'$.estimated')=1 THEN 1 ELSE 0 END),0),
                      COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens') IN ('integer','real')
                         AND COALESCE(json_extract(detail,'$.estimated'),0)!=1
                         THEN CAST(json_extract(detail,'$.cachedPromptTokens') AS INTEGER) ELSE 0 END),0),
                      COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens') IN ('integer','real')
                         AND COALESCE(json_extract(detail,'$.estimated'),0)!=1
                         THEN CAST(json_extract(detail,'$.promptTokens') AS INTEGER) ELSE 0 END),0),
                      COALESCE(SUM(CASE WHEN json_type(detail,'$.cachedPromptTokens') IN ('integer','real')
                         AND COALESCE(json_extract(detail,'$.estimated'),0)!=1 THEN 1 ELSE 0 END),0),
                      (SELECT COUNT(DISTINCT id) FROM family)
                 FROM audit
                WHERE event='model.usage'
                  AND json_extract(detail,'$.sessionId') IN (SELECT id FROM family)"#,
            [root_session_id],
            |row| Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?,
            )),
        ).map_err(|error| error.to_string())?;
        Ok(json!({
            "scope": "goal_session_tree",
            "sessions": sessions.max(0),
            "calls": calls.max(0),
            "promptTokens": prompt.max(0),
            "completionTokens": completion.max(0),
            "totalTokens": total.max(0),
            "estimatedCalls": estimated.max(0),
            "cachedPromptTokens": cached.max(0),
            "cacheEligiblePromptTokens": eligible.max(0),
            "cacheReportedCalls": measured.max(0),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendant_spend_is_counted_once_and_unrelated_spend_is_excluded() {
        let dir = std::env::temp_dir().join(format!(
            "dominion-tree-usage-{}", uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("usage.sqlite")).unwrap();
        store.put("goal", &json!({"id":"g","sessionId":"root"})).unwrap();
        store.put("session", &json!({"id":"root","goalId":"g"})).unwrap();
        store.put("session", &json!({"id":"child","parentSessionId":"root"})).unwrap();
        store.put("session", &json!({"id":"grand","parentSessionId":"child"})).unwrap();
        store.put("session", &json!({"id":"other"})).unwrap();
        for (session, tokens) in [
            ("root", 10), ("child", 30), ("grand", 50), ("other", 100)
        ] {
            store.audit("model.usage", json!({
                "sessionId":session,"promptTokens":tokens-2,
                "completionTokens":2,"totalTokens":tokens,
                "cachedPromptTokens":4
            }));
        }
        let usage = store.session_tree_usage("root").unwrap();
        assert_eq!(usage["scope"], "goal_session_tree");
        assert_eq!(usage["sessions"], 3);
        assert_eq!(usage["calls"], 3);
        assert_eq!(usage["totalTokens"], 90);
        assert_eq!(usage["promptTokens"], 84);
        assert_eq!(usage["completionTokens"], 6);
        assert_eq!(usage["cachedPromptTokens"], 12);
        assert_eq!(store.session_usage("root")["totalTokens"], 10);
        assert_eq!(store.enclosing_goal("grand").unwrap().0, "root");
        assert!(store.enclosing_goal("other").is_none());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_parent_cycle_cannot_double_count_usage() {
        let dir = std::env::temp_dir().join(format!(
            "dominion-tree-cycle-{}", uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("usage.sqlite")).unwrap();
        store.put("session", &json!({"id":"a","parentSessionId":"b"})).unwrap();
        store.put("session", &json!({"id":"b","parentSessionId":"a"})).unwrap();
        store.audit("model.usage", json!({"sessionId":"a","totalTokens":7}));
        store.audit("model.usage", json!({"sessionId":"b","totalTokens":9}));
        let usage = store.session_tree_usage("a").unwrap();
        assert_eq!(usage["sessions"], 2);
        assert_eq!(usage["totalTokens"], 16);
        assert!(store.enclosing_goal("a").is_none());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
