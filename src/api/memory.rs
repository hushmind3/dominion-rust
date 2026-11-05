use crate::state::AppState;
use axum::{extract::{State},Json};
use serde_json::{json,Value};

pub async fn memory_overview(State(state): State<AppState>) -> Json<Value> {
    let memories = state.store.all("memory");
    let mut by_type = std::collections::BTreeMap::<String, usize>::new();
    let mut by_source = std::collections::BTreeMap::<String, usize>::new();
    let mut by_status = std::collections::BTreeMap::<String, usize>::new();
    let mut by_scope = std::collections::BTreeMap::<String, usize>::new();
    for memory in &memories {
        for (map, key) in [
            (&mut by_type, memory["type"].as_str().unwrap_or("fact")),
            (&mut by_source, memory["source"].as_str().unwrap_or("owner")),
            (
                &mut by_status,
                memory["status"].as_str().unwrap_or("active"),
            ),
            (&mut by_scope, memory["scope"].as_str().unwrap_or("shared")),
        ] {
            *map.entry(key.to_string()).or_default() += 1;
        }
    }
    let jobs=state.store.all("job").into_iter().filter(|job|job["kind"]=="learn").take(50).map(|job|json!({"id":job["id"],"status":job["status"],"sessionId":job["input"]["sessionId"],"agentId":job["input"]["agentId"],"result":job["result"],"error":job["error"],"throughMessageId":job["input"]["throughMessageId"],"queuedAt":job["queuedAt"],"finishedAt":job["finishedAt"]})).collect::<Vec<_>>();
    let drafts = state
        .store
        .all("skill")
        .into_iter()
        .filter(|skill| {
            skill["source"] == "learn" && skill["enabled"] == false && skill["status"] == "draft"
        })
        .take(20)
        .collect::<Vec<_>>();
    let path = state.config.db_path();
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    Json(
        json!({"total":memories.len(),"path":path,"table":"records (kind=memory)","byType":by_type,"bySource":by_source,"byStatus":by_status,"scopes":by_scope,"byScope":by_scope,
        "learningJobs":jobs,"checkpoints":state.store.all("learning_checkpoint").into_iter().take(50).collect::<Vec<_>>(),
        "feedback":state.store.all("learning_feedback").into_iter().take(30).collect::<Vec<_>>(),"toolReliability":state.store.all("tool_stat"),"draftSkills":drafts,
        "capabilities":{"storage":"SQLite WAL","retrieval":"local lexical words and Korean bigrams","evidence":"exact source quotes, successful tool provenance","corrections":"scoped stable keys with superseded history","modelWeightTraining":false,"automaticSkillActivation":false},
        "updatedAt":chrono::Utc::now().to_rfc3339()}),
    )
}
