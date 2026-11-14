//! Rust-native, evidence-directed workflow repair. No external agent SDK.
//! This is a bounded continuation of the owner's original step, not a policy
//! which judges whether the request should be honored.
use serde_json::Value;

/// Verified steps get time to repair their artifacts. Legacy conversation
/// steps retain the original single-turn behavior.
pub(super) fn max_attempts(step: &Value, verified: bool) -> usize {
    if !verified { return 1; }
    step["maxAttempts"].as_u64().unwrap_or(3).clamp(1, 4) as usize
}

/// Preserve the complete original instruction. Only concrete verification and
/// tool errors are appended; a model cannot rewrite the owner's objective.
pub(super) fn repair_prompt(
    original: &str,
    attempt: usize,
    maximum: usize,
    failures: &[String],
) -> String {
    if attempt == 1 { return original.to_string(); }
    let notes = failures.iter().take(8).map(|gap| {
        let excerpt = gap.chars().take(450).collect::<String>();
        format!("- {excerpt}")
    }).collect::<Vec<_>>().join("\n");
    format!(
        "{original}\n\n[워크플로 자동 수정 · 시도 {attempt}/{maximum}]\n\
        직전 실행의 실제 파일·도구 검증에서 아래 항목이 남았습니다.\n\
        {notes}\n\
        원래 사용자 요청을 그대로 완수하세요. 이미 올바르게 생성한 파일은 유지하고,\n\
        누락·오류가 있는 부분을 조사해 도구로 수정한 후 실제 결과를 다시 확인하세요.\n\
        기존 성공 주장만 반복하지 말고 필요한 작업을 수행하세요."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn explicit_retry_budget_is_bounded_without_blocking_owner_objective() {
        assert_eq!(max_attempts(&json!({}), true), 3);
        assert_eq!(max_attempts(&json!({"maxAttempts":1}), true), 1);
        assert_eq!(max_attempts(&json!({"maxAttempts":4}), true), 4);
        assert_eq!(max_attempts(&json!({"maxAttempts":99}), true), 4);
        assert_eq!(max_attempts(&json!({"maxAttempts":4}), false), 1);
    }
    #[test]
    fn feedback_is_bounded_and_does_not_replace_original_instruction() {
        let original = "특정 파일 2개를 생성하세요. 모든 세부 요구는 유지하세요.";
        let failure = vec!["실제 파일 내용 불일치".repeat(200)];
        let next = repair_prompt(original, 2, 3, &failure);
        assert!(next.starts_with(original));
        assert!(next.contains("워크플로 자동 수정"));
        assert!(next.contains("실제 파일 내용 불일치"));
        assert!(next.chars().count() < 1000);
        assert_eq!(repair_prompt(original,1,3,&failure),original);
    }
}
