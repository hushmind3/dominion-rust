use super::{sessions::*,settings::*};
use axum::{extract::{Path,State},http::StatusCode,Json};
use crate::state::AppState;
use serde_json::json;
#[cfg(test)]
mod settings_tests {
    use super::*;
    fn state() -> AppState {
        let root =
            std::env::temp_dir().join(format!("dominion-core-settings-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root,
        };
        AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        )
    }
    #[tokio::test]
    async fn archive_preserves_transcript_and_creation_cannot_rebind_room() {
        let state = state();
        state
            .store
            .put("session", &json!({"id":"private","title":"보관할 대화"}));
        state.store.write_message(
            &json!({"session_id":"private","role":"user","content":"지워지면 안 되는 원문"}),
        );
        let response = archive_session(
            State(state.clone()),
            Path("private".into()),
            Json(json!({"archived":true})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            state.store.get("session", "private").unwrap()["archived"],
            true
        );
        assert_eq!(state.store.messages("private").len(), 1);
        archive_session(
            State(state.clone()),
            Path("private".into()),
            Json(json!({"archived":false})),
        )
        .await;
        assert_eq!(
            state.store.get("session", "private").unwrap()["archived"],
            false
        );
        state
            .store
            .put("session", &json!({"id":"bound","roomId":"real-room"}));
        let created = create_session(
            State(state.clone()),
            Json(json!({"id":"bound","roomId":"forged-room","title":"new chat"})),
        )
        .await;
        assert_eq!(created.status(), StatusCode::OK);
        let body = axum::body::to_bytes(created.into_body(), 8192).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_ne!(created["id"], "bound");
        assert!(created["roomId"].is_null());
        assert_eq!(
            state.store.get("session", "bound").unwrap()["roomId"],
            "real-room"
        );
        let response = save_assistant_settings(
            State(state.clone()),
            Json(json!({"instructions":"주인 공통 지침","enabled":true})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            assistant_settings(State(state.clone())).await.0["instructions"],
            "주인 공통 지침"
        );
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
}
