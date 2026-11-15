//! Explicit read/review/apply/discard endpoints for isolated goal Git changes.
use super::*;

/// Read the isolated diff and a digest over both tracked and untracked
/// file contents. The owner must submit that exact receipt before integration.
pub async fn changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match crate::engine::goal_git::changes(&state,&id).await {
        Ok(result)=>Json(result).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,&reason),
    }
}

/// Remove an unmerged goal checkout, never reset the user's active branch.
pub async fn discard_changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match crate::engine::goal_git::rollback(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,&reason),
    }
}

/// User-approved fast-forward only: no force, reset or unattended source edit.
pub async fn apply_changes(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let Some(digest)=body["reviewDigest"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"변경 미리보기 검토 영수증이 필요합니다.");
    };
    if body["acknowledgeReview"]!=true || body.as_object().is_none_or(|x|x.len()!=2){
        return error(StatusCode::BAD_REQUEST,"변경 검토 후 명시적으로 승인이 필요합니다.");
    }
    match crate::engine::goal_git::apply(&state,&id,digest).await {
        Ok(result)=>Json(result).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,&reason),
    }
}
