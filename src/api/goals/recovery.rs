//! HTTP adapter for the engine-owned crash-safe, owner-reviewed goal resume.
use super::*;

/// Uses the same atomic resume, reconciliation and audit logic as native SDK.
pub async fn resume_goal(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>,
) -> Response {
    match engine::goals::resume_goal(&state, &id, &body) {
        Ok(result) => (StatusCode::ACCEPTED, Json(result)).into_response(),
        Err(engine::goals::GoalResumeError::Missing(message)) =>
            error(StatusCode::NOT_FOUND, &message),
        Err(engine::goals::GoalResumeError::Invalid(message)) =>
            error(StatusCode::BAD_REQUEST, &message),
        Err(engine::goals::GoalResumeError::Conflict(message)) =>
            error(StatusCode::CONFLICT, &message),
        Err(engine::goals::GoalResumeError::Storage(message)) =>
            error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    }
}
