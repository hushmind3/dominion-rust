//! Deterministic improvement signals.
//!
//! The RSI campaign is model-driven and expensive. Before spending a single
//! token, the harness can point at concrete evidence it already has: failed
//! executions, campaigns that burned budget without a candidate, and model
//! turns that never changed source. These signals are not proposals and do
//! not modify anything; they are an owner-facing "what looks worth fixing"
//! list.
use crate::{engine::memory, state::AppState};
use serde_json::{json, Value};

fn signal(
    kind: &str,
    title: &str,
    detail: String,
    evidence: Vec<Value>,
    suggested_request: String,
) -> Value {
    json!({
        "id": format!("rsi-signal-{}", memory::stable_id(&[kind, &detail])),
        "kind": kind,
        "title": title,
        "detail": detail,
        "evidence": evidence,
        "suggestedRequest": suggested_request,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    })
}

/// Reads only local records; it never calls a model and never starts a job.
pub fn scan(state: &AppState) -> Vec<Value> {
    let mut signals = Vec::new();
    let campaigns = state.store.all("rsi_campaign");
    let generations = state.store.all("rsi_generation");
    let executions = state.store.all("execution");

    if let Some(campaign) = campaigns.first() {
        let tokens = campaign["tokens"].as_u64().unwrap_or(0);
        let accepted = campaign["acceptedGenerations"].as_u64().unwrap_or(0);
        let attempts = campaign["attempts"].as_u64().unwrap_or(0);
        if tokens >= 10_000 && accepted == 0 {
            signals.push(signal(
                "rsi_token_waste",
                "RSI 캠페인이 토큰만 쓰고 후보를 만들지 못했습니다",
                format!(
                    "최근 캠페인: {attempts}회 시도, {tokens}토큰 사용, 채택 {accepted}회."
                ),
                vec![json!({
                    "campaignId": campaign["id"],
                    "attempts": attempts,
                    "tokens": tokens,
                    "acceptedGenerations": accepted,
                    "stopReason": campaign["stopReason"],
                    "error": campaign["error"],
                })],
                "최근 RSI 캠페인이 후보를 만들지 못한 원인을 먼저 확인하고, 조사에 예산을 모두 쓰지 말고 한 파일을 실제로 수정하는 가장 작은 개선을 수행하세요.".into(),
            ));
        }
        if campaign["error"]
            .as_str()
            .is_some_and(|error| error.contains("No such file or directory"))
        {
            signals.push(signal(
                "rsi_environment",
                "RSI 작업이 외부 명령을 찾지 못했습니다",
                "캠페인이 모델 호출 전에 실행 환경 오류로 끝났습니다.".into(),
                vec![json!({"campaignId": campaign["id"], "error": campaign["error"]})],
                "RSI 작업이 사용하는 cargo/npm 실행 경로를 확인하고, 서비스 환경에서도 동작하도록 고치세요.".into(),
            ));
        }
    }

    let no_edit = generations
        .iter()
        .filter(|generation| generation["accepted"].as_bool() != Some(true))
        .filter(|generation| {
            generation["error"]
                .as_str()
                .is_some_and(|error| error.contains("변경") || error.contains("model"))
        })
        .count();
    if no_edit >= 3 {
        signals.push(signal(
            "rsi_no_edit",
            "모델이 코드를 수정하지 않은 시도가 반복됐습니다",
            format!("최근 RSI 세대 시도 {no_edit}건이 실제 소스 변경 없이 끝났습니다."),
            generations
                .iter()
                .rev()
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
        ));
    }

    let failed = executions
        .iter()
        .filter(|execution| execution["taskOutcome"].as_str() == Some("failed"))
        .take(5)
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        signals.push(signal(
            "execution_failures",
            "최근 실패 실행이 있습니다",
            format!("최근 실패 실행 {}건을 확인했습니다.", failed.len()),
            failed
                .iter()
                .map(|execution| {
                    json!({
                        "executionId": execution["id"],
                        "providerId": execution["providerId"],
                        "model": execution["model"],
                        "error": execution["error"],
                        "failureClass": execution["failureClass"],
                    })
                })
                .collect(),
            "최근 실패 실행의 공통 원인을 찾아 재발을 막는 가장 작은 수정을 하세요.".into(),
        ));
    }

    if !campaigns.is_empty() && state.store.count("rsi_candidate") == 0 {
        signals.push(signal(
            "rsi_no_candidate",
            "RSI 후보가 한 번도 생성되지 않았습니다",
            "캠페인은 기록됐지만 검증 가능한 후보가 없습니다.".into(),
            vec![json!({"campaigns": campaigns.len()})],
            "RSI가 후보를 만들지 못하는 원인을 확인하고, 한 번의 실행으로 검증 가능한 작은 변경을 제안하세요.".into(),
        ));
    }

    signals.truncate(6);
    signals
}
