//! Bounded parallel investigation. Child sessions are strictly read-only at
//! the model catalogue, the actual native tool gateway and MCP dispatch.
use super::*;

const MAX_RESEARCH:usize=4;

pub(super) fn allows(store:&Store,session_id:&str,name:&str)->bool{
    !store.get("session",session_id).is_some_and(|s|s["delegatedReadOnly"]==true)
        || matches!(name,
            "workspace_list"|"workspace_read"|"workspace_read_range"|
            "code_search"|"web_search"|"http_fetch"|"memory_search"|
            "task_status"|"task_result")
}
pub(super) async fn delegate_many(ctx:&ToolContext<'_>,args:&Value)->Result<Value,String>{
    let tasks=args["tasks"].as_array().ok_or("조사 작업 배열이 없습니다.")?;
    if tasks.is_empty()||tasks.len()>MAX_RESEARCH{
        return Err("병렬 조사에는 1~4개의 독립 작업이 필요합니다.".into());
    }
    if !ctx.tools_enabled||!ctx.tools_execution_enabled{
        return Err("도구 실행 권한이 없는 상위 세션입니다.".into());
    }
    if delegation_depth(ctx.store,ctx.session_id)>=MAX_DELEGATION_DEPTH{
        return Err("하위 에이전트 위임 깊이 제한에 도달했습니다.".into());
    }
    let mut jobs=Vec::new();
    for task in tasks{
        let agent=task["agentId"].as_str().filter(|s|!s.is_empty())
            .ok_or("조사할 에이전트 ID가 없습니다.")?;
        let prompt=task["prompt"].as_str().map(str::trim)
            .filter(|s|!s.is_empty()&&s.len()<=8000)
            .ok_or("조사 지시는 1~8000바이트여야 합니다.")?;
        if ctx.store.get("agent",agent).is_none(){
            return Err(format!("조사 에이전트를 찾지 못했습니다: {agent}"));
        }
        jobs.push((agent.to_owned(),prompt.to_owned()));
    }
    let active=ctx.store.all("job").iter().filter(|job|{
        job["input"]["parentSessionId"].as_str()==Some(ctx.session_id)
            && job["input"]["delegatedReadOnly"]==true
            && matches!(job["status"].as_str(),Some("queued"|"running"))
    }).count();
    if active+jobs.len()>MAX_RESEARCH{
        return Err("이미 실행 중인 조사를 수집한 뒤 다음 병렬 작업을 시작하세요.".into());
    }
    let mut spawned=Vec::new();
    for (agent,prompt) in jobs{
        match delegate(ctx,&json!({"agentId":agent,"prompt":prompt,"wait":false,"readOnly":true})).await{
            Ok(job)=>spawned.push(job),
            Err(error)=>return Ok(json!({"status":"partial","readOnly":true,
                "tasks":spawned,"error":error,"hint":"시작된 작업은 task_result로 수집하세요."}))
        }
    }
    if args["wait"]!=true{
        return Ok(json!({"status":"running","readOnly":true,
            "tasks":spawned,"hint":"병렬 조사는 조회 전용입니다. 수정은 부모 에이전트가 직렬로 수행하세요."}));
    }
    let seconds=args["timeoutSeconds"].as_u64().unwrap_or(90).clamp(5,300);
    let deadline=std::time::Instant::now()+Duration::from_secs(seconds);
    loop{
        if ctx.state.is_cancelled(ctx.session_id){
            return Ok(json!({"status":"cancelled","readOnly":true,"tasks":spawned}));
        }
        let mut complete=true;
        let mut results=Vec::new();
        for child in &spawned{
            let Some(id)=child["jobId"].as_str()else{continue};
            let result=task_payload(ctx,&json!({"jobId":id}),true)?;
            if matches!(result["status"].as_str(),Some("queued"|"running")){
                complete=false;
            }
            results.push(result);
        }
        if complete{return Ok(json!({"status":"finished","readOnly":true,"tasks":results}));}
        if std::time::Instant::now()>=deadline{
            return Ok(json!({"status":"running","readOnly":true,"tasks":results,
                "hint":"남은 조사 결과는 task_result로 확인하세요."}));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn only_observation_tools_are_authorized(){
        let good=["workspace_read","workspace_read_range","code_search","task_result"];
        let bad=["workspace_write","workspace_replace","shell_exec","file_write",
            "github_api","mcp__external__mutate","delegate_task","delegate_tasks","browser_control"];
        for name in good{assert!(matches!(name,"workspace_read"|"workspace_read_range"|"code_search"|"task_result"));}
        for name in bad{assert!(!matches!(name,"workspace_read"|"workspace_read_range"|"code_search"|"task_result"));}
    }
}
