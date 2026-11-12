//! Deterministic improvement checks.
//!
//! The RSI campaign is model-driven and expensive. Before spending a single
//! token, the harness can point at concrete evidence it already has: failed
//! executions, jobs that died, campaigns that burned budget without a
//! candidate, and model turns that never changed source. These signals are
//! not proposals and do not modify anything; they are the raw material for
//! the persistent improvement basket.
use crate::{engine::memory, state::AppState};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn signal(
    kind: &str,
    fingerprint_key: &str,
    title: &str,
    detail: String,
    evidence: Vec<Value>,
    suggested_request: String,
    target_files: &[&str],
) -> Value {
    let fingerprint = memory::stable_id(&[kind, fingerprint_key]);
    let evidence_fingerprint =
        memory::stable_id(&[kind, &Value::Array(evidence.clone()).to_string()]);
    json!({
        "id": format!("rsi-signal-{fingerprint}"),
        "fingerprint": fingerprint,
        "evidenceFingerprint": evidence_fingerprint,
        "kind": kind,
        "title": title,
        "detail": detail,
        "evidence": evidence,
        "suggestedRequest": suggested_request,
        "targetFiles": target_files,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    })
}

fn job_targets(kind: &str) -> Vec<&'static str> {
    match kind {
        "room" | "room_turn" => vec!["src/engine/rooms.rs", "src/engine/chat/runtime.rs"],
        "prompt" | "chat" | "chat_turn" => vec!["src/engine/chat/runtime.rs"],
        "rsi_compare" => vec!["src/engine/rsi_worktree.rs"],
        "workflow" => vec!["src/engine/workflows.rs"],
        "goal" => vec!["src/engine/goals.rs"],
        "learning" => vec!["src/engine/learning.rs", "src/engine/memory.rs"],
        _ => vec!["src/engine/jobs.rs"],
    }
}

fn execution_targets(failure_class: &str) -> Vec<&'static str> {
    match failure_class {
        "timeout" | "network" => vec!["src/engine/gateway.rs", "src/engine/chat/runtime.rs"],
        "provider" | "model" | "auth" => vec!["src/engine/gateway.rs", "src/engine/models.rs"],
        "tool" | "verification" => vec!["src/engine/tools.rs", "src/engine/chat/runtime.rs"],
        _ => vec!["src/engine/chat/runtime.rs"],
    }
}

/// Age in seconds for a job record. Jobs store RFC3339 `queuedAt` and the
/// store adds a SQLite `updatedAt`; accept both without making the scanner
/// depend on one timestamp format.
fn age_seconds(job: &Value) -> Option<i64> {
    let raw = job["queuedAt"]
        .as_str()
        .or_else(|| job["updatedAt"].as_str())?;
    let then = chrono::DateTime::parse_from_rfc3339(raw)
        .map(|value| value.with_timezone(&chrono::Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .map(|value| value.and_utc())
        })
        .ok()?;
    Some((chrono::Utc::now() - then).num_seconds().max(0))
}

fn short_error(value: &Value) -> String {
    value["error"]
        .as_str()
        .or_else(|| value["result"]["rsi"]["modelError"].as_str())
        .or_else(|| value["result"]["rsi"]["captureError"].as_str())
        .unwrap_or("")
        .chars()
        .take(180)
        .collect()
}

/// Reads only local records; it never calls a model and never starts a job.
/// The returned `checks` list proves which categories were inspected even
/// when they produced no signal.
pub fn report(state: &AppState) -> Value {
    let mut signals = Vec::new();
    let campaigns = state.store.all("rsi_campaign");
    let generations = state.store.all("rsi_generation");
    let executions = state
        .store
        .all("execution")
        .into_iter()
        .take(50)
        .collect::<Vec<_>>();
    let jobs = state.store.all("job").into_iter().take(80).collect::<Vec<_>>();
    let candidates = state.store.count("rsi_candidate");

    if let Some(campaign) = campaigns.first() {
        let tokens = campaign["tokens"].as_u64().unwrap_or(0);
        let accepted = campaign["acceptedGenerations"].as_u64().unwrap_or(0);
        let attempts = campaign["attempts"].as_u64().unwrap_or(0);
        if tokens >= 10_000 && accepted == 0 {
            let campaign_id = campaign["id"].as_str().unwrap_or("unknown");
            signals.push(signal(
                "rsi_token_waste",
                campaign_id,
                "RSI 캠페인이 토큰만 쓰고 후보를 만들지 못했습니다",
                format!(
                    "최근 캠페인: {attempts}회 시도, {tokens}토큰 사용, 채택 {accepted}회."
                ),
                vec![json!({
                    "campaignId": campaign_id,
                    "attempts": attempts,
                    "tokens": tokens,
                    "acceptedGenerations": accepted,
                    "stopReason": campaign["stopReason"],
                    "error": campaign["error"],
                })],
                "최근 RSI 캠페인이 후보를 만들지 못한 원인을 확인하고, 조사에 예산을 모두 쓰지 말고 대상 파일 하나를 실제로 수정하세요.".into(),
                &["src/engine/rsi_evolve.rs", "src/api/rsi.rs"],
            ));
        }
        if campaign["error"]
            .as_str()
            .is_some_and(|error| error.contains("No such file or directory"))
        {
            let campaign_id = campaign["id"].as_str().unwrap_or("unknown");
            signals.push(signal(
                "rsi_environment",
                campaign_id,
                "RSI 작업이 외부 명령을 찾지 못했습니다",
                "캠페인이 모델 호출 전에 실행 환경 오류로 끝났습니다.".into(),
                vec![json!({"campaignId": campaign_id, "error": campaign["error"]})],
                "RSI 작업이 사용하는 cargo/npm 실행 경로를 확인하고, 서비스 환경에서도 동작하도록 고치세요.".into(),
                &["src/engine/rsi_worktree.rs"],
            ));
        }
    }

    let recent_generations = generations.iter().take(10).collect::<Vec<_>>();
    let no_edit = recent_generations
        .iter()
        .take_while(|generation| {
            generation["accepted"].as_bool() != Some(true)
                && (generation["diffBytes"].as_u64() == Some(0)
                    || generation["error"].as_str().is_some_and(|error| {
                        error.contains("변경")
                            || error.contains("model")
                            || error.contains("토큰")
                            || error.contains("예산")
                    }))
        })
        .count();
    if no_edit >= 2 {
        signals.push(signal(
            "rsi_no_edit",
            "all",
            "모델이 코드를 수정하지 않은 시도가 반복됐습니다",
            format!("최근 RSI 세대 시도 {no_edit}건이 실제 소스 변경 없이 끝났습니다."),
            recent_generations
                .iter()
                .take(3)
                .map(|generation| {
                    json!({
                        "generationId": generation["id"],
                        "error": generation["error"],
                        "tokens": generation["tokens"],
                    })
                })
                .collect(),
            "모델이 조사만 하다 끝나지 않도록, 대상 파일과 실패 증거를 좁혀 한 번에 수정하는 개선을 수행하세요.".into(),
            &["src/api/rsi.rs", "src/engine/rsi_worktree.rs"],
        ));
    }

    let mut failed_groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for execution in executions
        .iter()
        .filter(|execution| execution["taskOutcome"].as_str() == Some("failed"))
    {
        let class = execution["failureClass"].as_str().unwrap_or("unknown");
        let error = short_error(execution);
        let key = format!(
            "{class}|{}",
            error.chars().take(60).collect::<String>().to_lowercase()
        );
        failed_groups.entry(key).or_default().push(execution);
    }
    for (key, rows) in failed_groups.into_iter().take(3) {
        let class = rows
            .first()
            .and_then(|row| row["failureClass"].as_str())
            .unwrap_or("unknown");
        signals.push(signal(
            "execution_failures",
            &key,
            "같은 원인으로 실패한 실행이 있습니다",
            format!("실패 실행 {}건이 같은 오류로 끝났습니다.", rows.len()),
            rows.iter()
                .take(5)
                .map(|execution| {
                    json!({
                        "executionId": execution["id"],
                        "providerId": execution["providerId"],
                        "model": execution["model"],
                        "error": short_error(execution),
                        "failureClass": execution["failureClass"],
                    })
                })
                .collect(),
            "최근 실패 실행의 공통 원인을 찾아 재발을 막는 가장 작은 수정을 하세요.".into(),
            &execution_targets(class),
        ));
    }

    let failed_jobs = jobs
        .iter()
        .filter(|job| {
            matches!(
                job["status"].as_str(),
                Some("failed" | "needs_review")
            )
        })
        .filter(|job| !matches!(job["kind"].as_str(), Some("rsi_evolve" | "rsi_generation")))
        .filter(|job| !short_error(job).is_empty())
        .collect::<Vec<_>>();
    let mut job_groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for job in failed_jobs {
        job_groups
            .entry(job["kind"].as_str().unwrap_or("job").to_string())
            .or_default()
            .push(job);
    }
    for (kind, rows) in job_groups.into_iter().take(3) {
        let latest = rows.first().copied().unwrap_or(&Value::Null);
        signals.push(signal(
            "failed_jobs",
            &kind,
            "최근 작업이 실제 오류로 끝났습니다",
            format!(
                "{kind} 작업 {}건이 실패 상태로 남아 있습니다. 최근 오류: {}",
                rows.len(),
                short_error(latest)
            ),
            rows.iter()
                .take(5)
                .map(|job| {
                    json!({
                        "jobId": job["id"],
                        "kind": job["kind"],
                        "status": job["status"],
                        "error": short_error(job),
                        "queuedAt": job["queuedAt"],
                    })
                })
                .collect(),
            "실패한 작업의 실제 오류를 재현하고 원인을 고치는 가장 작은 수정을 하세요.".into(),
            &job_targets(&kind),
        ));
    }

    let stale = jobs
        .iter()
        .filter(|job| job["status"].as_str() == Some("running"))
        .filter(|job| age_seconds(job).is_some_and(|age| age >= 3600))
        .take(5)
        .collect::<Vec<_>>();
    if !stale.is_empty() {
        signals.push(signal(
            "stale_running_jobs",
            "running",
            "오래 끝나지 않은 실행 작업이 있습니다",
            format!("1시간 넘게 running 상태인 작업 {}건을 찾았습니다.", stale.len()),
            stale
                .iter()
                .map(|job| {
                    json!({
                        "jobId": job["id"],
                        "kind": job["kind"],
                        "queuedAt": job["queuedAt"],
                        "ageSeconds": age_seconds(job),
                    })
                })
                .collect(),
            "중단된 실행 작업이 복구되지 않는 원인을 찾아, 재시작 뒤에도 안전하게 정리되도록 고치세요.".into(),
            &["src/engine/jobs.rs"],
        ));
    }

    if !campaigns.is_empty() && candidates == 0 {
        signals.push(signal(
            "rsi_no_candidate",
            "all",
            "RSI 후보가 한 번도 생성되지 않았습니다",
            "캠페인은 기록됐지만 검증 가능한 후보가 없습니다.".into(),
            vec![json!({"campaigns": campaigns.len(), "candidates": candidates})],
            "RSI가 후보를 만들지 못하는 원인을 확인하고, 한 번의 실행으로 검증 가능한 작은 변경을 제안하세요.".into(),
            &["src/engine/rsi_evolve.rs", "src/engine/rsi_worktree.rs"],
        ));
    }

    signals.truncate(12);
    let checks = vec![
        json!({"id":"rsi_campaigns","title":"RSI 캠페인 기록","inspected":campaigns.len()}),
        json!({"id":"rsi_generations","title":"RSI 세대 시도","inspected":generations.len()}),
        json!({"id":"executions","title":"최근 실행 결과","inspected":executions.len()}),
        json!({"id":"jobs","title":"작업 큐 상태","inspected":jobs.len()}),
        json!({"id":"candidates","title":"검증된 개선 후보","inspected":candidates}),
    ];
    json!({
        "signals": signals,
        "checks": checks,
        "checked": checks.len(),
        "generatedAt": chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_operator_stop_is_preserved_but_does_not_recommend_a_code_repair() {
        let root=std::env::temp_dir().join(format!("rsi-signal-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),crate::vault::Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"operator-stop","kind":"resume","status":"cancelled","error":"Stopped by user"}));
        assert!(report(&state)["signals"].as_array().unwrap().is_empty());
        state.store.put("job",&json!({"id":"real-failure","kind":"workflow","status":"failed","error":"Actual workflow invariant failed"}));
        let result=report(&state);
        let signal=result["signals"].as_array().unwrap().iter().find(|signal|signal["kind"]=="failed_jobs").unwrap();
        assert_eq!(signal["evidence"].as_array().unwrap().len(),1);
        assert_eq!(signal["evidence"][0]["jobId"],"real-failure");
        assert!(state.store.get("job","operator-stop").is_some());
        drop(state);std::fs::remove_dir_all(root).unwrap();
    }
}
