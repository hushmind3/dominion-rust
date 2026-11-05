//! Root task episodes and per-attempt links.
//!
//! Ordinary chat, goals and rooms all execute through the same turn runtime.
//! A goal prompt and a room prompt are derived work, not new owner
//! instructions. This module keeps one root `task_episode` per owner utterance
//! and links each execution attempt to it, so a room with nine speakers still
//! produces one task outcome rather than nine independent task successes.
use crate::engine::adaptation::types::{revision_hash, TaskContract, TaskOutcome};
use crate::state::AppState;
use serde_json::{json, Value};

pub struct RootSpec<'a> {
    pub task_id: &'a str,
    pub intent: &'a str,
    pub family: &'a str,
    pub source: &'a str,
    pub source_message_id: Option<&'a str>,
    pub scope: Value,
    pub mandatory: Vec<Value>,
}

fn contract(spec: &RootSpec<'_>) -> TaskContract {
    TaskContract {
        task_id: spec.task_id.to_string(),
        revision: revision_hash(&json!({
            "schemaVersion": 1,
            "taskId": spec.task_id,
            "intent": spec.intent,
            "family": spec.family,
            "source": spec.source,
        })),
        source: spec.source.to_string(),
        scope: spec.scope.clone(),
        intent: spec.intent.to_string(),
        family: spec.family.to_string(),
        mandatory: spec.mandatory.clone(),
        bindings: json!({}),
        permissions: json!({}),
        budget: json!({}),
    }
}

/// Creates the root episode once. Existing episodes are never overwritten:
/// a restart or a second speaker must reuse the frozen contract revision.
pub fn start_root(state: &AppState, spec: RootSpec<'_>) -> Option<Value> {
    if let Some(existing) = state.store.get("task_episode", spec.task_id) {
        return Some(existing);
    }
    let contract = contract(&spec);
    if contract.validate().is_err() {
        return None;
    }
    let episode = json!({
        "id": spec.task_id,
        "contract": contract,
        "sourceMessageId": spec.source_message_id,
        "ownerText": spec.intent,
        "attempts": [],
        "rootOutcome": "running",
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    state.store.put("task_episode", &episode)
}

/// Attaches the persisted owner message to a root that was created before the
/// runtime stored that message.
pub fn attach_source(
    state: &AppState,
    task_id: &str,
    source_message_id: &str,
    intent: &str,
) -> Option<Value> {
    let mut episode = state.store.get("task_episode", task_id)?;
    if episode["sourceMessageId"].as_str().is_some_and(|id| !id.is_empty()) {
        return Some(episode);
    }
    episode["sourceMessageId"] = json!(source_message_id);
    episode["ownerText"] = json!(intent);
    state.store.put("task_episode", &episode)
}

/// Links one runtime execution to the root task. Re-linking the same attempt
/// is idempotent.
pub fn link_attempt(
    state: &AppState,
    task_id: &str,
    execution_id: &str,
    agent_id: Option<&str>,
    verification_owner: &str,
) -> Option<Value> {
    let mut episode = state.store.get("task_episode", task_id)?;
    let attempts = episode["attempts"].as_array_mut()?;
    if attempts
        .iter()
        .any(|attempt| attempt["executionId"].as_str() == Some(execution_id))
    {
        return Some(episode);
    }
    attempts.push(json!({
        "executionId": execution_id,
        "agentId": agent_id,
        "verificationOwner": verification_owner,
        "status": "running",
        "startedAt": chrono::Utc::now().to_rfc3339(),
    }));
    state.store.put("task_episode", &episode)
}

fn fold_root_outcome(attempts: &[Value]) -> &'static str {
    let outcome = |name: &str| {
        attempts
            .iter()
            .any(|attempt| attempt["status"].as_str() == Some(name))
    };
    if outcome("succeeded") {
        return "succeeded";
    }
    if outcome("unknown") {
        return "unknown";
    }
    if outcome("running") {
        return "running";
    }
    if outcome("failed") {
        return "failed";
    }
    if attempts.is_empty() {
        return "running";
    }
    if attempts
        .iter()
        .all(|attempt| attempt["status"].as_str() == Some("cancelled"))
    {
        return "cancelled";
    }
    "answered_unverified"
}

/// Records the terminal outcome of one attempt and recomputes the root outcome
/// unless an explicit owner (goal lifecycle) already froze it.
pub fn complete_attempt(
    state: &AppState,
    task_id: &str,
    execution_id: &str,
    outcome: TaskOutcome,
) -> Option<Value> {
    let mut episode = state.store.get("task_episode", task_id)?;
    let status = serde_json::to_value(outcome)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into());
    if let Some(attempts) = episode["attempts"].as_array_mut() {
        if let Some(attempt) = attempts.iter_mut().find(|attempt| {
            attempt["executionId"].as_str() == Some(execution_id)
        }) {
            attempt["status"] = json!(status);
            attempt["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        }
    }
    if episode["rootFinal"].as_bool() != Some(true) {
        let folded = fold_root_outcome(
            episode["attempts"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        if episode["rootOutcome"].as_str() != Some(folded) {
            episode["rootOutcome"] = json!(folded);
            episode["rootOutcomeAt"] = json!(chrono::Utc::now().to_rfc3339());
        }
    }
    state.store.put("task_episode", &episode)
}

/// Freezes the root outcome for lifecycles that own the final judgment
/// (a goal can still be `needs_review` even when one attempt succeeded).
pub fn set_root_outcome(
    state: &AppState,
    task_id: &str,
    outcome: &str,
    reason: &[String],
) -> Option<Value> {
    let mut episode = state.store.get("task_episode", task_id)?;
    episode["rootOutcome"] = json!(outcome);
    episode["rootReason"] = json!(reason);
    episode["rootFinal"] = json!(true);
    episode["rootOutcomeAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put("task_episode", &episode)
}

pub fn goal_outcome(status: &str) -> &'static str {
    match status {
        "completed" => "succeeded",
        "cancelled" => "cancelled",
        "needs_review" => "unknown",
        _ => "failed",
    }
}

/// A verified success belongs to the root task, not to each room speaker or
/// goal iteration that reused the same evidence.
pub fn success_marker_id(task_id: &str) -> String {
    format!("success-experience:{task_id}")
}

pub fn contract_revision(episode: &Value) -> Option<&str> {
    episode["contract"]["revision"].as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::PathBuf;

    fn state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-episode-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn root(state: &AppState) -> Value {
        start_root(
            state,
            RootSpec {
                task_id: "task-room-1",
                intent: "보고서를 작성해",
                family: "room",
                source: "owner",
                source_message_id: Some("message-1"),
                scope: json!({"partition": "local", "sessionId": "room-session"}),
                mandatory: vec![json!({"type": "file_exists", "path": "report.md"})],
            },
        )
        .unwrap()
    }

    #[test]
    fn one_root_links_many_attempts_and_counts_success_once() {
        let (state, dir) = state();
        root(&state);
        // A restart or a second speaker must not replace the frozen contract.
        let again = root(&state);
        assert_eq!(again["id"], "task-room-1");
        assert_eq!(state.store.count("task_episode"), 1);

        for (attempt, agent) in [("exec-a", "a"), ("exec-b", "b"), ("exec-c", "c")] {
            link_attempt(&state, "task-room-1", attempt, Some(agent), "self").unwrap();
        }
        complete_attempt(&state, "task-room-1", "exec-a", TaskOutcome::Failed).unwrap();
        complete_attempt(&state, "task-room-1", "exec-b", TaskOutcome::Succeeded).unwrap();
        complete_attempt(&state, "task-room-1", "exec-c", TaskOutcome::Failed).unwrap();
        let episode = state.store.get("task_episode", "task-room-1").unwrap();
        assert_eq!(episode["attempts"].as_array().unwrap().len(), 3);
        assert_eq!(episode["rootOutcome"], "succeeded");
        assert_eq!(
            episode["attempts"][1]["status"],
            "succeeded",
            "attempt-level evidence stays visible"
        );
        assert_eq!(
            success_marker_id("task-room-1"),
            "success-experience:task-room-1"
        );

        // Replaying the same attempt is idempotent.
        link_attempt(&state, "task-room-1", "exec-a", Some("a"), "self").unwrap();
        assert_eq!(
            state.store.get("task_episode", "task-room-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_goal_outcome_is_not_recomputed_from_attempts() {
        let (state, dir) = state();
        root(&state);
        link_attempt(&state, "task-room-1", "exec-a", Some("a"), "parent").unwrap();
        complete_attempt(&state, "task-room-1", "exec-b", TaskOutcome::Succeeded).unwrap();
        set_root_outcome(&state, "task-room-1", "unknown", &["확인 필요".into()]).unwrap();
        let episode = state.store.get("task_episode", "task-room-1").unwrap();
        assert_eq!(episode["rootOutcome"], "unknown");
        assert_eq!(episode["rootFinal"], true);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_root_is_never_created_by_a_child_attempt() {
        let (state, dir) = state();
        assert!(link_attempt(&state, "missing", "exec", None, "self").is_none());
        assert!(complete_attempt(
            &state,
            "missing",
            "exec",
            TaskOutcome::Succeeded
        )
        .is_none());
        assert_eq!(state.store.count("task_episode"), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
