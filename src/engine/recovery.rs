//! H4 failure classification shared by finalization, goals and jobs.
//!
//! The classifier is intentionally conservative: it only uses recorded
//! evidence (status, operations, acceptance checks) and never blames a specific
//! model. The hint is fed back into the next planning/replan step.
use crate::state::AppState;
use serde_json::{json, Value};

pub const UNKNOWN_EXTERNAL: &str = "unknown_external_result";
pub const PERMISSION_DENIED: &str = "permission_denied";
pub const LIMIT_EXCEEDED: &str = "limit_exceeded";
pub const MODEL_ERROR: &str = "model_error";
pub const VERIFICATION_FAILED: &str = "verification_failed";
pub const TOOL_FAILED: &str = "tool_failed";
pub const CANCELLED: &str = "cancelled";
pub const EXECUTION_ERROR: &str = "execution_error";

fn clamped_contains(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub fn classify(execution: &Value, operations: &[Value]) -> &'static str {
    if execution["status"].as_str() == Some("cancelled") {
        return CANCELLED;
    }
    if operations.iter().any(|operation| {
        operation["outcomeUnknown"] == true
            || matches!(
                operation["status"].as_str(),
                Some("interrupted" | "running")
            )
    }) {
        return UNKNOWN_EXTERNAL;
    }
    let reason = execution["reason"].as_str().unwrap_or("");
    let lowered = reason.to_lowercase();
    if lowered.contains("승인")
        || lowered.contains("거부")
        || lowered.contains("권한")
        || lowered.contains("denied")
        || lowered.contains("permission")
    {
        return PERMISSION_DENIED;
    }
    if lowered.contains("예산")
        || lowered.contains("시간 제한")
        || lowered.contains("token")
        || lowered.contains("limit")
    {
        return LIMIT_EXCEEDED;
    }
    if lowered.contains("연결 실패")
        || lowered.contains("http")
        || lowered.contains("모델")
        || lowered.contains("provider")
    {
        return MODEL_ERROR;
    }
    if clamped_contains(&lowered, &["절차 위반", "검증", "verification", "필수 절차"]) {
        return VERIFICATION_FAILED;
    }
    if !execution["acceptanceText"].is_null()
        && execution["acceptancePassed"].as_bool() != Some(true)
    {
        return VERIFICATION_FAILED;
    }
    if operations
        .iter()
        .any(|operation| operation["status"].as_str() == Some("failed"))
    {
        return TOOL_FAILED;
    }
    EXECUTION_ERROR
}

/// What the next attempt/replan should change. Not a claim that a retry will
/// succeed; an unknown external effect is explicitly non-retryable.
pub fn hint(class: &str) -> &'static str {
    match class {
        UNKNOWN_EXTERNAL => {
            "외부 도구 결과가 확정되지 않았습니다. 자동 재실행하지 말고 실제 상태를 먼저 확인하세요."
        }
        PERMISSION_DENIED => {
            "권한/승인이 거부되었습니다. 같은 호출을 반복하지 말고 허용된 범위나 다른 도구를 선택하세요."
        }
        LIMIT_EXCEEDED => {
            "예산 또는 시간 한도에 도달했습니다. 작업을 더 작은 단계로 나누고 불필요한 호출을 줄이세요."
        }
        MODEL_ERROR => {
            "모델/제공자 호출이 실패했습니다. 입력을 줄이거나 다른 제공자·모델로 전환하세요."
        }
        VERIFICATION_FAILED => {
            "검증 조건이 충족되지 않았습니다. 주장을 반복하지 말고 누락된 실제 산출물을 확인해 보완하세요."
        }
        TOOL_FAILED => {
            "도구가 실패했습니다. 인자와 환경을 확인하고 같은 명령을 그대로 반복하지 마세요."
        }
        CANCELLED => "실행이 취소되었습니다.",
        _ => "실행이 실패했습니다. 마지막 오류와 실제 증거를 확인한 뒤 다른 접근을 선택하세요.",
    }
}

/// Latest recorded failure hint for a task, used by goal replanning.
pub fn latest_failure_hint(state: &AppState, task_id: &str) -> Option<String> {
    let latest = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| execution["taskId"].as_str() == Some(task_id))
        .filter(|execution| execution["failureClass"].as_str().is_some())
        .max_by_key(|execution| {
            execution["startedAt"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })?;
    let class = latest["failureClass"].as_str()?;
    Some(format!(
        "실행 실패 분류: {class} — {}",
        latest["recoveryHint"]
            .as_str()
            .unwrap_or_else(|| hint(class))
    ))
}

pub fn failure_record(execution: &Value, operations: &[Value]) -> Value {
    let class = classify(execution, operations);
    json!({"failureClass": class, "recoveryHint": hint(class)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_external_effect_is_never_a_retryable_tool_error() {
        let execution = json!({"status":"failed","reason":"tool failed"});
        let operations = vec![json!({"status":"interrupted","outcomeUnknown":true})];
        assert_eq!(classify(&execution, &operations), UNKNOWN_EXTERNAL);
        assert!(hint(UNKNOWN_EXTERNAL).contains("자동 재실행하지"));
    }

    #[test]
    fn permission_and_limits_are_distinguished() {
        assert_eq!(
            classify(&json!({"status":"failed","reason":"승인이 거부되었습니다."}), &[]),
            PERMISSION_DENIED
        );
        assert_eq!(
            classify(&json!({"status":"failed","reason":"실행 시간 제한(10분)을 초과했습니다."}), &[]),
            LIMIT_EXCEEDED
        );
        assert_eq!(
            classify(&json!({"status":"failed","reason":"모델 호출 실패: HTTP 500"}), &[]),
            MODEL_ERROR
        );
    }

    #[test]
    fn verification_beats_generic_execution_error() {
        let execution = json!({
            "status":"completed",
            "acceptanceText":"file exists",
            "acceptancePassed":false
        });
        assert_eq!(classify(&execution, &[]), VERIFICATION_FAILED);
        assert_eq!(
            classify(&json!({"status":"cancelled"}), &[]),
            CANCELLED
        );
        assert_eq!(
            classify(
                &json!({"status":"failed","reason":"필수 절차 위반으로 완료할 수 없습니다"}),
                &[]
            ),
            VERIFICATION_FAILED
        );
    }
}
