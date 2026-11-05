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
        let completion = llm::complete(&self.context.state, provider, model, system, prompt);
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
