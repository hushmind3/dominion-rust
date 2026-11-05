//! Compatibility migrations run only after this process owns the listener.
//! Writes are verified before legacy records are removed; newer records win.
use crate::state::AppState;
use serde_json::{json, Value};

pub fn run(state: &AppState) -> Result<(), String> {
    provider_secrets(state)?;
    collection_names(state)?;
    interrupted_approvals(state)
}
fn provider_secrets(state: &AppState) -> Result<(), String> {
    for provider in state.store.all("provider") {
        let Some(plaintext) = provider["apiKey"]
            .as_str()
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(id) = provider["id"].as_str() else {
            continue;
        };
        let name = format!("provider:{id}");
        if !state.store.has_secret(&name) {
            state.save_secret(&name, plaintext);
            if state.read_secret(&name).as_deref() != Some(plaintext) {
                return Err(format!(
                    "Cannot persist encrypted provider key for {id}; original record was preserved"
                ));
            }
        } else if state
            .read_secret(&name)
            .filter(|key| !key.is_empty())
            .is_none()
        {
            return Err(format!(
                "Cannot decrypt the existing provider key for {id}; original record was preserved"
            ));
        }
        let mut clean = provider.clone();
        clean
            .as_object_mut()
            .ok_or("Invalid provider record")?
            .remove("apiKey");
        state
            .store
            .put("provider", &clean)
            .ok_or_else(|| format!("Cannot remove the legacy plaintext provider field for {id}"))?;
        state
            .store
            .audit("migration.provider_key", json!({"providerId":id}));
    }
    Ok(())
}
fn payload(mut record: Value) -> Value {
    if let Some(map) = record.as_object_mut() {
        map.remove("createdAt");
        map.remove("updatedAt");
    }
    record
}
fn collection_names(state: &AppState) -> Result<(), String> {
    for (plural, singular) in [
        ("memories", "memory"),
        ("skills", "skill"),
        ("automations", "automation"),
        ("workflows", "workflow"),
        ("rooms", "room"),
        ("channels", "channel"),
        ("jobs", "job"),
        ("executions", "execution"),
        ("goals", "goal"),
    ] {
        for record in state.store.all(plural) {
            let Some(id) = record["id"].as_str() else {
                continue;
            };
            if let Some(current) = state.store.get(singular, id) {
                if payload(current) != payload(record.clone()) {
                    state.store.audit("migration.collection_conflict",json!({"legacyKind":plural,"currentKind":singular,"id":id,"legacyPreserved":true}));
                    continue;
                }
            } else {
                state.store.put(singular, &record).ok_or_else(|| {
                    format!("Cannot migrate {plural}/{id}; original record was preserved")
                })?;
            }
            if !state.store.remove(plural, id) && state.store.get(plural, id).is_some() {
                return Err(format!("Cannot finish migration of {plural}/{id}"));
            }
        }
    }
    Ok(())
}
fn interrupted_approvals(state: &AppState) -> Result<(), String> {
    for mut approval in state.store.all("approval") {
        if approval["status"] != "pending" {
            continue;
        }
        approval["status"] = json!("interrupted");
        approval["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
        state
            .store
            .put("approval", &approval)
            .ok_or("Cannot recover an interrupted approval")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (AppState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("dominion-migrations-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), root)
    }
    #[test]
    fn migrations_preserve_newer_records_and_keep_legacy_conflicts_for_recovery() {
        let (state, dir) = state();
        state
            .store
            .put("memories", &json!({"id":"same","text":"old"}));
        state
            .store
            .put("memory", &json!({"id":"same","text":"new"}));
        state
            .store
            .put("memories", &json!({"id":"only-legacy","text":"retained"}));
        run(&state).unwrap();
        assert_eq!(state.store.get("memory", "same").unwrap()["text"], "new");
        assert_eq!(state.store.get("memories", "same").unwrap()["text"], "old");
        assert!(state.store.get("memories", "only-legacy").is_none());
        assert_eq!(
            state.store.get("memory", "only-legacy").unwrap()["text"],
            "retained"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn provider_migration_retains_existing_vault_value_and_is_idempotent() {
        let (state, dir) = state();
        state
            .store
            .put("provider", &json!({"id":"p","apiKey":"old-key"}));
        state.save_secret("provider:p", "current-key");
        state
            .store
            .put("approval", &json!({"id":"a","status":"pending"}));
        run(&state).unwrap();
        run(&state).unwrap();
        assert_eq!(
            state.read_secret("provider:p").as_deref(),
            Some("current-key")
        );
        assert!(state
            .store
            .get("provider", "p")
            .unwrap()
            .get("apiKey")
            .is_none());
        assert_eq!(
            state.store.get("approval", "a").unwrap()["status"],
            "interrupted"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
