use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    use std::{path::Path, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(output.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn continuing_uses_the_saved_goal_and_refuses_a_running_campaign(){
        let root=std::env::temp_dir().join(format!("dominion-rsi-resume-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let mut prior=json!({"id":"old","kind":"rsi_evolve","status":"completed","input":{
            "rsiObjective":"retain the actual user goal","rsiFocusFiles":["src/engine/memory.rs"],"providerId":"p","model":"m"}});
        state.store.put("job",&prior);
        let resumed=continued_request(&state,json!({"continueFrom":"old"})).unwrap();
        assert_eq!(resumed["request"],"retain the actual user goal");assert_eq!(resumed["providerId"],"p");
        assert_eq!(resumed["focusFiles"],json!(["src/engine/memory.rs"]));
        prior["status"]=json!("running");state.store.put("job",&prior);
        assert!(continued_request(&state,json!({"continueFrom":"old"})).is_err());
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn natural_language_start_queues_selected_model_only_on_clean_sub_branch() {
        let root = std::env::temp_dir().join(format!("dominion-rsi-start-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let data = root.join("data");
        std::fs::create_dir_all(checkout.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(checkout.join("Cargo.toml"), "[package]\nname = \"dominion\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi.rs"), "// source\n").unwrap();
        git(&checkout, &["init", "-q"]);
        git(&checkout, &["checkout", "-b", "dominion-sub4"]);
        git(&checkout, &["add", "."]);
        git(&checkout, &["-c", "user.email=ci@example.com", "-c", "user.name=CI", "commit", "-qm", "fixture"]);
        let cfg = Config { port:0, data_dir:data.clone(), web_dir:data.join("web"), workspace:data.clone() };
        let state = AppState::new(cfg.clone(), Store::open(&cfg.db_path()).unwrap(), Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state, &json!({"sourceDir":checkout.to_str().unwrap()}));
        state.store.put("provider", &json!({"id":"test-provider","name":"Local","enabled":true,
            "kind":"openai","model":"sample-model","baseUrl":"http://127.0.0.1:1/v1"}));
        let body = json!({"providerId":"test-provider","model":"sample-model",
            "request":"100턴에서 장기기억이 사라지는 문제를 찾아 고쳐 주세요."});
        let response = start(State(state.clone()), Json(body.clone())).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let jobs = state.store.all("job");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["input"]["providerId"], "test-provider");
        assert_eq!(jobs[0]["input"]["model"], "sample-model");
        assert_eq!(jobs[0]["input"]["rsiSourceDir"], checkout.canonicalize().unwrap().to_str().unwrap());
        assert_eq!(jobs[0]["input"]["rsiSourceBranch"], "dominion-sub4");
        assert!(jobs[0]["input"]["rsiBaseCommit"].as_str().is_some_and(|v|v.len()==40));
        let cases=jobs[0]["input"]["rsiFrozenCases"].as_array().unwrap();
        assert_eq!(cases.len(),14);
        assert!(cases.iter().any(|case|case["id"]=="planning-development" && case["holdout"]==false));
        assert!(cases.iter().any(|case|case["id"]=="planning-quality" && case["holdout"]==true));
        assert!(jobs[0]["input"]["prompt"].as_str().unwrap().contains("workspace_write"));
        assert_eq!(jobs[0]["input"]["rsiAutonomous"], false);
        let auto_response = start(State(state.clone()), Json(json!({
            "providerId":"test-provider","model":"sample-model"
        }))).await;
        assert_eq!(auto_response.status(), StatusCode::ACCEPTED);
        let queued = state.store.all("job");
        assert_eq!(queued.len(), 2);
        let automated = queued.iter().find(|job| job["input"]["rsiAutonomous"] == true).unwrap();
        assert!(automated["input"]["prompt"].as_str().unwrap().contains("전수조사 기반 자율 개선"));
        assert!(automated["input"]["rsiObjective"].as_str().unwrap().contains("실제 작업에 필요한 능력"));
        assert!(automated["input"]["rsiObjective"].as_str().unwrap().contains("변경 없이"));
        std::fs::write(checkout.join("uncommitted.txt"), "owner edits").unwrap();
        let refused = start(State(state.clone()), Json(body)).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.store.all("job").len(), 2);
        // A non-developer can start from an unchanged copy of their current
        // source without committing unrelated files or switching Git branches.
        git(&checkout, &["checkout","-b","ordinary-owner-branch"]);
        let continuous=json!({"providerId":"test-provider","model":"sample-model",
            "mode":"continuous","confirmation":"START_RSI_EVOLUTION","selectionPolicy":"manual"});
        let response=start(State(state.clone()),Json(continuous.clone())).await;
        assert_eq!(response.status(),StatusCode::ACCEPTED);
        let queued=state.store.all("job");
        let campaign=queued.iter().find(|job|job["kind"]=="rsi_evolve").unwrap();
        assert_eq!(campaign["input"]["rsiNativeApproved"],true);
        assert_eq!(campaign["input"]["selectionPolicy"],"manual");
        assert!(campaign["input"]["limits"]["tokenBudget"].is_null());
        assert!(campaign["input"]["limits"]["timeBudgetSeconds"].is_null());
        assert_eq!(start(State(state.clone()),Json(continuous)).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(std::fs::read_to_string(checkout.join("uncommitted.txt")).unwrap(),"owner edits");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn full_inspection_reuses_the_index_and_has_no_token_cap() {
        let root = std::env::temp_dir().join(format!("dominion-rsi-index-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let data = root.join("data");
        std::fs::create_dir_all(checkout.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(checkout.join("Cargo.toml"), "[package]\nname = \"dominion\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi.rs"), "// source\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi_evolve.rs"),
            "pub fn run_loop() {\n    // TODO: reuse the index\n}\n").unwrap();
        git(&checkout, &["init", "-q"]);
        git(&checkout, &["checkout", "-b", "dominion-sub"]);
        git(&checkout, &["add", "."]);
        git(&checkout, &["-c", "user.email=ci@example.com", "-c", "user.name=CI", "commit", "-qm", "fixture"]);
        let cfg = Config { port:0, data_dir:data.clone(), web_dir:data.join("web"), workspace:data.clone() };
        let state = AppState::new(cfg.clone(), Store::open(&cfg.db_path()).unwrap(), Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state, &json!({"sourceDir":checkout.to_str().unwrap()}));
        state.store.put("provider", &json!({"id":"test-provider","name":"Local","enabled":true,
            "kind":"openai","model":"sample-model","baseUrl":"http://127.0.0.1:1/v1"}));
        let body = json!({"providerId":"test-provider","model":"sample-model",
            "mode":"continuous","confirmation":"START_RSI_EVOLUTION","selectionPolicy":"manual",
            "focusFiles":["src/engine/rsi_evolve.rs"],"maxAttempts":1});
        let response = start(State(state.clone()), Json(body)).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let jobs = state.store.all("job");
        let campaign = jobs.iter().find(|job|job["kind"]=="rsi_evolve").unwrap();
        assert_eq!(campaign["input"]["limits"]["maxAttempts"], 1);
        assert!(campaign["input"]["limits"]["tokenBudget"].is_null());
        assert!(campaign["input"]["limits"]["timeBudgetSeconds"].is_null());
        assert!(campaign["input"]["rsiIndexFiles"].as_u64().unwrap_or(0) >= 3);
        assert_eq!(campaign["input"]["rsiFocusFiles"][0], "src/engine/rsi_evolve.rs");
        let prompt = campaign["input"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("저장된 코드 인덱스"));
        assert!(prompt.contains("src/engine/rsi_evolve.rs"));
        assert!(prompt.contains("run_loop") || prompt.contains("TODO"));
        assert!(!prompt.contains("// TODO: reuse the index"),"source belongs in indexed retrieval, not every prompt");
        assert!(prompt.chars().count()<6000);
        assert!(state.store.all("code_index").len() >= 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn capability_requires_three_consecutive_accepts() {
        let partial = json!({"history":[
            {"accepted":true,"evaluation":{}},
            {"accepted":true,"evaluation":{}},
            {"accepted":false,"evaluation":{}},
            {"accepted":true,"evaluation":{}}
        ]});
        assert_eq!(accepted_streak(&partial), (1, 2));
        let full = json!({"history":[
            {"accepted":true},{"accepted":true},{"accepted":true}
        ]});
        assert_eq!(accepted_streak(&full),(3,3));
        let research_break = json!({"history":[
            {"accepted":true},{"accepted":false},{"accepted":true}
        ]});
        assert_eq!(accepted_streak(&research_break),(1,1));
        // Three boolean flags only measure a streak; the independent lineage
        // oracle is enforced by apply_capability and tested in engine::rsi.
    }
