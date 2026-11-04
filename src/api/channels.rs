use crate::state::AppState;
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Channel registry shipped as JSON manifests. Runtime drivers are not ported
/// yet, so every entry reports `available: false` honestly instead of failing
/// silently when the user tries to connect.
fn manifests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/channels")
}

pub fn catalog() -> Vec<Value> {
    let mut entries = Vec::new();
    let Ok(dirs) = std::fs::read_dir(manifests()) else {
        return entries;
    };
    for dir in dirs.flatten() {
        let path = dir.path().join("manifest.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        value["available"] = json!(false);
        value["status"] = json!("unavailable");
        value["requiresPublicUrl"] = json!(
            value
                .get("mode")
                .and_then(|mode| mode.as_str())
                .map(|mode| mode == "webhook")
                .unwrap_or(false)
        );
        entries.push(value);
    }
    entries.sort_by(|a, b| {
        let left = (
            a.get("category").and_then(|v| v.as_str()).unwrap_or(""),
            a.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        let right = (
            b.get("category").and_then(|v| v.as_str()).unwrap_or(""),
            b.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        left.cmp(&right)
    });
    entries
}

pub fn connected_accounts(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("oauth_config")
        .into_iter()
        .filter(|record| record.get("connected").and_then(|v| v.as_bool()) == Some(true))
        .map(|record| {
            let id = record
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            json!({
                "id": id,
                "provider": id,
                "displayName": record.get("account").cloned().unwrap_or(json!("연결된 계정")),
                "scopes": record.get("scopes").cloned().unwrap_or(json!("")),
                "hasToken": state.store.has_secret(&format!("oauth:{id}:token")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_discord_and_is_honest() {
        let entries = catalog();
        assert!(entries.len() >= 20, "expected the full channel registry");
        let discord = entries
            .iter()
            .find(|entry| entry["id"] == "discord")
            .expect("discord entry");
        assert_eq!(discord["available"], false);
        assert!(discord["fields"].as_array().is_some());
    }
}

pub async fn catalog_handler() -> Json<Value> {
    Json(serde_json::json!(catalog()))
}

pub async fn connections(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!(connected_accounts(&state)))
}
