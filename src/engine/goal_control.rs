//! A durable goal keeps the cancellation epoch it started with. A later owner
//! message may resume the session, but cannot resume an older goal invocation.
use crate::engine::{jobs::JobContext, llm};
use std::time::Duration;

#[derive(Clone)]
pub struct GoalControl {
    context: JobContext,
    goal_id: String,
    session_id: String,
    generation: u64,
}
impl GoalControl {
    pub fn new(context: &JobContext, goal_id: &str, session_id: &str) -> Self {
        Self {
            context: context.clone(),
            goal_id: goal_id.into(),
            session_id: session_id.into(),
            generation: context.state.cancellation_generation(session_id),
        }
    }
    pub fn is_cancelled(&self) -> bool {
        let state = &self.context.state;
        if state.cancellation_generation(&self.session_id) != self.generation
            || state.is_cancelled(&self.session_id)
        {
            return true;
        }
        let Some(job) = state.store.get("job", &self.context.job_id) else {
            return true;
        };
        if job["cancelRequested"].as_bool() == Some(true) || job["status"] == "cancelled" {
            return true;
        }
        // Deleting the goal also removes authority for its pending model/tool
        // phases. It must not keep working with an orphaned in-memory request.
        state
            .store
            .get("goal", &self.goal_id)
            .is_none_or(|goal| goal["status"] == "cancelled")
    }
    pub async fn complete(
        &self,
        provider: &str,
        model: &str,
        system: &str,
        prompt: &str,
    ) -> Result<String, String> {
        if self.is_cancelled() {
            return Err(cancelled());
        }
        // Planner, reviewer and replanner use the SAME durable goal-session
        // ledger as the tool-capable executor. Previously their model calls
        // had no session attribution and could bypass the owner's budget.
        // This is admission based on observed usage, not a pre-billed hard cap:
        // one in-flight call can exceed the remaining tokens, and providers
        // that omit usage cannot be measured exactly.
        let goal = self.context.state.store.get("goal", &self.goal_id)
            .ok_or_else(cancelled)?;
        let token_budget = goal["tokenBudget"].as_u64().unwrap_or(150_000);
        if token_budget > 0 {
            let observed = self.context.state.store.session_usage(&self.session_id)["totalTokens"]
                .as_u64().unwrap_or(0);
            if observed >= token_budget {
                return Err(format!(
                    "목표 토큰 예산({token_budget})에 도달하여 추가 계획/검토 모델 호출을 중단했습니다 (기록: {observed}토큰)."
                ));
            }
        }
        let completion = llm::complete_for_session(
            &self.context.state, provider, model, system, prompt, &self.session_id,
        );
        tokio::pin!(completion);
        loop {
            tokio::select! {
                result=&mut completion=>return if self.is_cancelled() {Err(cancelled())} else {result},
                _=tokio::time::sleep(Duration::from_millis(50))=>if self.is_cancelled() {return Err(cancelled());},
            }
        }
    }
}
fn cancelled() -> String {
    "사용자 요청으로 목표 실행이 중단되었습니다.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::{path::PathBuf, sync::Arc};
    fn state() -> (crate::state::AppState, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dominion-goal-control-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s"}));
        state.store.put(
            "goal",
            &json!({"id":"g","goal":"a real task","sessionId":"s","status":"running"}),
        );
        state.store.put(
            "job",
            &json!({"id":"j","kind":"goal","status":"running","sessionId":"s"}),
        );
        (state, dir)
    }
    #[tokio::test]
    async fn stalled_review_cannot_revive_after_stop_and_a_new_session_turn() {
        use axum::{extract::State, routing::post, Json, Router};
        let (state, dir) = state();
        let started = Arc::new(tokio::sync::Notify::new());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router=Router::new().route("/v1/chat/completions",post(|State(started):State<Arc<tokio::sync::Notify>>,Json(_):Json<Value>|async move {started.notify_one();std::future::pending::<Json<Value>>().await})).with_state(started.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        state.store.put("provider",&json!({"id":"p","baseUrl":format!("http://127.0.0.1:{port}/v1"),"model":"mock","enabled":true}));
        let context = JobContext::new(state.clone(), "j".into());
        let control = GoalControl::new(&context, "g", "s");
        let pending = control.clone();
        let task = tokio::spawn(async move {
            pending
                .complete("p", "mock", "strict reviewer", "review the result")
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        state.cancel_session("s");
        state.clear_cancel("s");
        // Even resetting the visible run flags for a newly authorized request
        // cannot authorize the older invocation's captured epoch.
        state
            .store
            .patch_if("goal", "g", "id", &json!("g"), &json!({"status":"running"}));
        state.store.patch_if(
            "job",
            "j",
            "id",
            &json!("j"),
            &json!({"status":"running","cancelRequested":false}),
        );
        assert!(!state.is_cancelled("s"));
        assert!(control.is_cancelled());
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.unwrap_err().contains("중단"));
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn planner_and_reviewer_share_executor_usage_budget_without_duplicate_calls() {
        use axum::{extract::State, routing::post, Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (state, dir) = state();
        let calls = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new()
            .route("/v1/chat/completions", post(
                |State(calls): State<Arc<AtomicUsize>>, Json(_): Json<Value>| async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"choices":[{"message":{"content":"plan or review"}}],
                        "usage":{"prompt_tokens":6,"completion_tokens":3,"total_tokens":9}}))
                }
            ))
            .with_state(calls.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        state.store.put("provider", &json!({
            "id":"p", "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"mock", "enabled":true
        }));
        state.store.patch_if("goal", "g", "id", &json!("g"),
            &json!({"tokenBudget":8}));
        let control = GoalControl::new(&JobContext::new(state.clone(), "j".into()), "g", "s");

        assert_eq!(control.complete("p", "mock", "planner", "plan").await.unwrap(),
            "plan or review");
        assert_eq!(state.store.session_usage("s")["totalTokens"], 9);
        assert_eq!(state.store.session_usage("s")["calls"], 1);
        let blocked = control.complete("p", "mock", "reviewer", "review").await.unwrap_err();
        assert!(blocked.contains("예산"));
        assert_eq!(calls.load(Ordering::SeqCst), 1,
            "a rejected helper must not send another provider request");

        // Existing zero=unlimited contract must remain intact.
        state.store.patch_if("goal", "g", "id", &json!("g"),
            &json!({"tokenBudget":0}));
        assert!(control.complete("p", "mock", "reviewer", "review").await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(state.store.session_usage("s")["totalTokens"], 18);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancelled_or_deleted_records_stop_work_without_requiring_session_flag() {
        let (state, dir) = state();
        let context = JobContext::new(state.clone(), "j".into());
        let control = GoalControl::new(&context, "g", "s");
        assert!(!control.is_cancelled());
        state.store.patch_if(
            "job",
            "j",
            "id",
            &json!("j"),
            &json!({"cancelRequested":true}),
        );
        assert!(control.is_cancelled());
        state.store.patch_if(
            "job",
            "j",
            "id",
            &json!("j"),
            &json!({"cancelRequested":false}),
        );
        state.store.remove("goal", "g");
        assert!(control.is_cancelled());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
