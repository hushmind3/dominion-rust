//! Parallel research delegation is deliberately read-only. The parent alone
//! applies edits after checking independently collected child evidence.
//! This permits parallel file and web exploration without concurrent writes.
use super::*;
// Shared parent functions: delegate() and task_payload().

const MAX_PARALLEL: usize = 4;

pub(crate) fn research_tool_allowed(name: &str) -> bool {
    matches!(name,
        "workspace_list" | "workspace_read" | "workspace_read_range" |
        "code_search" | "web_search" | "http_fetch" | "memory_search" |
        "task_status" | "task_result")
}

/// Used by BOTH model tool dispatch and deterministic tool execution.
pub(crate) fn delegation_allows(store: &Store, session_id: &str, name: &str) -> bool {
    !store.get("session",session_id).is_some_and(|s|s["delegatedReadOnly"]==true)
        || research_tool_allowed(name)
}

pub(crate) async fn delegate_batch(ctx: &ToolContext<'_>,args:&Value)->Result<Value,String>{
    // A batch is a parent-authored plan, never a recursively delegated write
    // authority. Validate every member before starting ANY child.
    let tasks=args["tasks"].as_array().ok_or("tasks 배열이 필요합니다.")?;
    if tasks.is_empty() || tasks.len()>MAX_PARALLEL {
        return Err(format!("독립 조사 작업은 1~{MAX_PARALLEL}개가 필요합니다."));
    }
    if !ctx.tools_enabled || !ctx.tools_execution_enabled {
        return Err("상위 에이전트가 도구 권한을 제공하지 않았습니다.".into());
    }
    let mut validated=Vec::new();
    for task in tasks {
        let id=task["agentId"].as_str().filter(|s|!s.is_empty())
            .ok_or("조사 에이전트 ID가 필요합니다.")?;
        let prompt=task["prompt"].as_str().map(str::trim)
            .filter(|s|!s.is_empty()&&s.len()<=8000)
            .ok_or("조사 지시는 1~8000바이트여야 합니다.")?;
        if ctx.store.get("agent",id).is_none() {
            return Err(format!("조사 에이전트를 찾을 수 없습니다: {id}"));
        }
        validated.push((id.to_owned(),prompt.to_owned()));
    }
    // No unbounded detached worker explosion when a model repeatedly asks
    // for batches. Session-wide running/queued child research is counted.
    let active=ctx.store.all("job").iter().filter(|job|{
        job["input"]["parentSessionId"].as_str()==Some(ctx.session_id)
          && job["input"]["delegatedReadOnly"]==true
          && matches!(job["status"].as_str(),Some("queued"|"running"))
    }).count();
    if active+validated.len()>MAX_PARALLEL {
        return Err("같은 세션에서 실행 중인 병렬 조사 4개를 먼저 수집하세요.".into());
    }
    let mut spawned=Vec::new();
    for (agent_id,prompt) in validated {
        // delegate handles per-child session, actual persistent job, detached
        // scheduling and inherited permissions. No write cap is widened.
        match delegate(ctx,&json!({"agentId":agent_id,
            "prompt":prompt,"wait":false,"readOnly":true})).await {
            Ok(created)=>spawned.push(created),
            Err(error)=>{
                return Ok(json!({"status":"partial","tasks":spawned,
                    "error":error,"hint":"이미 시작한 작업은 task_result로 확인하세요."}));
            }
        }
    }
    let wait=args["wait"].as_bool().unwrap_or(false);
    if !wait {return Ok(json!({"status":"running","tasks":spawned,
        "readOnly":true,"hint":"task_status/task_result로 각각 확인하고, 수정은 부모 작업에서 직렬로 수행하세요."}));}
    let timeout=args["timeoutSeconds"].as_u64().unwrap_or(90).clamp(5,300);
    let deadline=std::time::Instant::now()+Duration::from_secs(timeout);
    loop {
        if ctx.state.is_cancelled(ctx.session_id) {
            return Ok(json!({"status":"cancelled","tasks":spawned,
                "hint":"상위 작업 중단. 이미 시작된 하위 작업 결과는 확인 후 처리하세요."}));
        }
        let mut done=true;
        let mut results=Vec::new();
        for child in &spawned {
            let Some(job_id)=child["jobId"].as_str() else {continue};
            let payload=task_payload(ctx,&json!({"jobId":job_id}),true)?;
            if matches!(payload["status"].as_str(),Some("queued"|"running")) {
                done=false;
            }
            results.push(payload);
        }
        if done {return Ok(json!({"status":"finished","tasks":results,"readOnly":true}));}
        if std::time::Instant::now()>=deadline {
            return Ok(json!({"status":"running","tasks":results,
                "readOnly":true,"hint":"병렬 조사가 진행 중입니다. 나중에 task_result로 확인하세요."}));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_delegation_never_exposes_mutating_tools(){
        assert!(research_tool_allowed("workspace_read_range"));
        assert!(research_tool_allowed("code_search"));
        for tool in ["workspace_write","workspace_replace","shell_exec",
                     "file_read","github_api","browser_control","mcp__external__mutate",
                     "delegate_task","delegate_tasks","memory_store"] {
            assert!(!research_tool_allowed(tool),"{tool} must not mutate a research branch");
        }
    }
}
