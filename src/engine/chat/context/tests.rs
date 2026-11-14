//! Split from src/engine/chat/context.rs; regression coverage only.
    use super::*;
    use serde_json::json;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-context-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        (state, dir)
    }

    #[test]
    fn assigned_active_skills_are_scoped_per_agent_in_chat_and_workflow_context(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s"})).unwrap();
        state.store.put("agent",&json!({"id":"researcher","name":"Researcher",
            "skillIds":["research"]})).unwrap();
        state.store.put("agent",&json!({"id":"editor","name":"Editor",
            "skillIds":["editing"]})).unwrap();
        for (id,active) in [("research",true),("editing",true),("disabled",false)]{
            state.store.put("skill",&json!({"id":id,"name":id,
                "instructions":format!("SKILL_{id}"),"enabled":active,
                "status":"active","scope":"global"})).unwrap();
        }
        let (research,items,_) = context_block_with_manifest(
            &state,"s",Some("researcher"),"Investigate",None,None).unwrap();
        assert!(research.contains("SKILL_research"));
        assert!(!research.contains("SKILL_editing"));
        assert_eq!(items.iter().filter(|v|v["kind"]=="skill").count(),1);
        let (editing,_,_) = context_block_with_manifest(
            &state,"s",Some("editor"),"Edit",None,None).unwrap();
        assert!(editing.contains("SKILL_editing"));
        assert!(!editing.contains("SKILL_research"));
        state.store.patch_if("agent","researcher","id",&json!("researcher"),
            &json!({"skillIds":[]})).unwrap();
        let context=context_block_with_manifest(
            &state,"s",Some("researcher"),"none",None,None);
        assert!(!context.as_ref().is_some_and(|(text,_,_)|text.contains("SKILL_research")));
        drop(state);std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn room_history_preserves_stored_reasoning_for_provider_specific_replay() {
        let (state, dir) = state();
        state
            .store
            .put("agent", &json!({"id":"a1","name":"아인즈"}));
        state.store.put(
            "session",
            &json!({"id":"room:t","roomId":"t","agentId":"a1"}),
        );
        state.store.write_message(&json!({
            "session_id":"room:t",
            "role":"user",
            "content":"안녕",
            "metadata":{"authorKind":"owner"},
        }));
        state.store.write_message(&json!({
            "session_id":"room:t",
            "role":"assistant",
            "agent_id":"a1",
            "content":"짧은 답",
            "reasoning":"SECRET-REASONING-TEXT",
            "metadata":{"providerReasoning":"SECRET-REASONING-TEXT"},
        }));
        let history = history(&state, "room:t", Some("a1"), None).unwrap();
        let serialized = serde_json::to_string(&history).unwrap();
        assert!(serialized.contains("SECRET-REASONING-TEXT"));
        assert!(history.iter().any(|message| message["role"] == "assistant"
            && message["reasoning_content"] == "SECRET-REASONING-TEXT"));
        let _ = std::fs::remove_dir_all(dir);
    }
