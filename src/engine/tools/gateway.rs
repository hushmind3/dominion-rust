//! Tool execution gateway: enforce delegation and RSI authority before dispatch.
//! Tool definitions and implementations remain independently replaceable.
use super::*;

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &Value,
    run_id: &str,
) -> Result<Value, String> {
    if !delegation_allows(ctx.store,ctx.session_id,name){
        return Err("읽기 전용 조사 에이전트는 변경·실행·재위임 도구를 사용할 수 없습니다.".into());
    }
    let isolated_rsi = ctx.store.get("session",ctx.session_id)
        .is_some_and(|s|s["rsiRun"]==true);
    if isolated_rsi && !rsi_allowed(name) {
        return Err("분리 자기개선 작업에서는 작업공간 내부 도구만 허용됩니다.".into());
    }
    // Factor-isolated variants cannot change their comparison companion.
    // Enforce this at the actual gateway even if a model invokes a hidden tool.
    if isolated_rsi && matches!(name,
        "workspace_replace"|"workspace_replace_function"|"workspace_write") {
        if let Some(session)=ctx.store.get("session",ctx.session_id){
            let item=&session["rsiFactorialCurriculum"]["selected"];
            if session["rsiSystemEvolution"]==true
                && item["isolationRequired"]==true
                && args["path"]!=item["target"]["path"] {
                return Err("요인별 분리 실험은 선택된 1개 소스만 수정할 수 있습니다.".into());
            }
        }
    }
    // A policy inherited from an ADOPTED predecessor is enforced by the
    // tool gateway, not just by hiding a schema in the next model turn.
    // Only the isolated successor session may inherit this restriction.
    if isolated_rsi && name=="workspace_write"
        && ctx.store.get("session",ctx.session_id).is_some_and(|session|{
            let policy=if session["rsiSuccessorStage"].is_object(){
                &session["rsiSuccessorStage"]
            }else{&session["rsiMetaControlPolicy"]};
            session["rsiSystemEvolution"]==true
                && policy["toolPolicy"]=="patch_only"
                && !(session["rsiRecursiveScienceActive"]==true
                    && session["rsiRecursiveDiscovery"]["phase"]=="invent_new_intelligence"
                    && args["path"]==session["rsiRecursiveDiscovery"]["invention"]["newFile"])
        }) {
        return Err("계승된 후손 도구 정책(patch_only)은 신규 파일 생성을 허용하지 않습니다.".into());
    }
    if isolated_rsi && matches!(name, "workspace_write" | "workspace_replace" | "workspace_replace_function") {
        let path=args["path"].as_str().unwrap_or("");
        if !crate::engine::rsi::safe_path(path) {
            return Err("자기개선은 허용된 소스코드 경로만 수정할 수 있습니다.".into());
        }
        if name=="workspace_write" {
            let resolved=resolve_workspace(ctx.workspace,path)?;
            if tokio::fs::metadata(&resolved).await.is_ok() {
                return Err("RSI의 workspace_write는 새 파일 생성 전용입니다. 기존 소스는 workspace_read_range에서 startLine, endLine, sourceSha256를 확인하고 workspace_replace(startLine,endLine,expectedSha256,newText)로 부분 수정하세요.".into());
            }
        }
    }
    match name {
        "workspace_list" => {
            let path = resolve_workspace(ctx.workspace, string_arg(args, "path").unwrap_or("."))?;
            list_dir(&path).await
        }
        "workspace_read" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            observed_read(ctx, &path, run_id).await
        }
        "workspace_read_range" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            if isolated_rsi && crate::engine::rsi::sealed_measurement_path(&path.to_string_lossy()) {
                return Err("개선 세대는 동결 평가 구현을 읽을 수 없습니다.".into());
            }
            let start = args["startLine"].as_u64().unwrap_or(1).max(1);
            let requested_end = args["endLine"].as_u64().unwrap_or(start.saturating_add(199));
            let result = observed_read(ctx, &path, run_id).await?;
            let text = result["content"].as_str().unwrap_or_default();
            let lines = text.lines().collect::<Vec<_>>();
            let begin = (start as usize).min(lines.len());
            // RSI generations must leave budget for the edit. A 400-line read
            // can consume most of the remaining campaign budget on its own.
            let line_cap = if isolated_rsi { 120 } else { 400 };
            let end = (requested_end as usize).min(lines.len()).max(begin).min(begin.saturating_add(line_cap));
            let mut numbered = String::new();
            for (offset, line) in lines[begin.saturating_sub(1)..end].iter().enumerate() {
                numbered.push_str(&format!("{:>5} {}\n", begin + offset, line));
            }
            let mut output = json!({
                "path": result["path"],
                "startLine": begin.max(1),
                "endLine": end,
                "totalLines": lines.len(),
                "sourceSha256":format!("{:x}",Sha256::digest(text.as_bytes())),
                "content": numbered,
            });
            if let Some(observation) = result.get("fileObservation") {
                output["fileObservation"] = observation.clone();
            }
            Ok(output)
        }
        "code_search" => {
            let query = required_arg(args, "query")?;
            let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 50) as usize;
            search_code_index(ctx, &query, limit)
        }
        "workspace_replace" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            let new_text=required_arg(args,"newText")?;
            if let Some(old_text)=args["oldText"].as_str().filter(|text|!text.is_empty()) {
                replace_text(ctx,&path,old_text,&new_text,
                    args["expectedSha256"].as_str(),run_id).await
            } else if isolated_rsi {
                // The latest real-model whole-system episode authored a
                // design, then sent workspace_replace without a valid endLine.
                // Recover ONLY an unambiguous model-authored function or
                // explicit one-line edit, never invent a range of source lines.
                match rsi_unambiguous_edit_intent(args,&new_text) {
                    Some(RsiEditIntent::CompleteFunction(function)) => {
                        let indexed=rsi_pinned_source_sha(ctx,&path,args);
                        let expected=args["expectedSha256"].as_str()
                            .or(indexed.as_deref())
                            .ok_or("함수 수정에는 현재 소스 SHA-256이 필요합니다.")?;
                        let mut receipt=replace_compiled_rust_function(
                            ctx,&path,&function,&new_text,expected,run_id).await?;
                        receipt["editContractRecovered"]=json!(
                            "model_authored_complete_function");
                        receipt["sourceHashRecoveredFromIndex"]=json!(
                            indexed.is_some());
                        Ok(receipt)
                    }
                    Some(RsiEditIntent::SingleLine(start)) => {
                        let mut precise=args.clone();
                        precise["endLine"]=json!(start);
                        let mut receipt=replace_line_span(
                            ctx,&path,&precise,&new_text,run_id).await?;
                        receipt["editContractRecovered"]=json!(
                            "explicit_single_line");
                        Ok(receipt)
                    }
                    None => replace_line_span(ctx,&path,args,&new_text,run_id).await,
                }
            } else {
                replace_line_span(ctx,&path,args,&new_text,run_id).await
            }
        }
        "workspace_replace_function" => {
            let path=resolve_workspace(ctx.workspace,&required_arg(args,"path")?)?;
            replace_compiled_rust_function(ctx,&path,
                &required_arg(args,"functionName")?,
                &required_arg(args,"newText")?,
                &required_arg(args,"expectedSha256")?,
                run_id).await
        }
        "workspace_write" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            checked_write(
                ctx,
                &path,
                &required_arg(args, "content")?,
                run_id,
                name == "workspace_write",
                args["expectedSha256"].as_str(),
            )
            .await
        }
        "file_list" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            list_dir(&path).await
        }
        "file_read" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            observed_read(ctx, &path, run_id).await
        }
        "file_write" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            checked_write(
                ctx,
                &path,
                &required_arg(args, "content")?,
                run_id,
                name == "workspace_write",
                args["expectedSha256"].as_str(),
            )
            .await
        }
        "shell_exec" => {
            let command = required_arg(args, "command")?;
            let cwd = match string_arg(args, "cwd") {
                Some(dir) => PathBuf::from(dir),
                None => ctx.workspace.to_path_buf(),
            };
            shell(&command, &cwd).await
        }
        "terminal_exec" => {
            let command = required_arg(args, "command")?;
            let argv: Vec<String> = args
                .get("args")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            executable(&command, &argv, ctx.workspace).await
        }
        "browser_control" => {
            let action = required_arg(args, "action")?;
            crate::engine::browser::execute(
                &action,
                args,
                &ctx.state.config.data_dir,
                ctx.workspace,
                ctx.agent_id.unwrap_or(ctx.session_id),
            )
            .await
        }
        "web_search" => {
            let query = required_arg(args, "query")?;
            web_search(&query).await
        }
        "http_fetch" => {
            let url = required_arg(args, "url")?;
            http_fetch(&url).await
        }
        "memory_store" => {
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            crate::engine::memory::store_tool(ctx.store, &scope, args)
        }
        "memory_search" => {
            let query = required_arg(args, "query")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            Ok(json!({
                "results": crate::engine::memory::retrieve(ctx.store, &scope, &query, 10)
            }))
        }
        "skill_propose" => {
            let name = required_arg(args, "name")?;
            let instructions = required_arg(args, "instructions")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            let record = ctx.store.put(
                "skill",
                &json!({
                    "name": name,
                    "instructions": instructions,
                    "description": instructions.chars().take(160).collect::<String>(),
                    "enabled": false,
                    "status": "draft",
                    "source": "agent",
                    "scope": "agent",
                    "sessionId": ctx.session_id,
                    "agentId": ctx.agent_id,
                    "partitionKey": scope.partition_key,
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            Ok(json!({ "proposed": true, "skill": record }))
        }
        "rsi_method_propose" => crate::engine::rsi_evolve::propose_method(ctx.state, ctx.session_id, args),
        "rsi_system_plan" => crate::engine::rsi_system::propose(ctx.state, ctx.session_id, args),
        "algorithm_propose" => {
            let session = ctx
                .store
                .get("session", ctx.session_id)
                .ok_or("자기개선 세션을 찾을 수 없습니다.")?;
            if session["rsiRun"] != true {
                return Err("algorithm_propose는 자기개선 세대에서만 사용할 수 있습니다.".into());
            }
            let campaign_id = session["rsiCampaignId"]
                .as_str()
                .ok_or("진행 중인 자기개선 캠페인이 없습니다.")?;
            let running = ctx.store.get("job", campaign_id).is_some_and(|job| {
                job["kind"] == "rsi_evolve"
                    && job["status"] == "running"
                    && job["cancelRequested"] != true
            });
            if !running {
                return Err("진행 중인 자기개선 캠페인이 없습니다.".into());
            }
            let expression = required_arg(args, "expression")?;
            let name = string_arg(args, "name")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("rsi-search-expression");
            let mut parameters=json!({"expression":expression});
            if let Some(target_expression)=args["targetExpression"].as_str() {
                parameters["targetExpression"]=json!(target_expression);
            }
            let record = crate::engine::algorithms::register(
                ctx.state,
                &json!({
                    "kind": "selfimprove.search",
                    "name": name,
                    "variant": name,
                    "implementation": "expression",
                    "parameters": parameters,
                    "description": args["description"].as_str().unwrap_or(""),
                    "source": "rsi-generated",
                }),
            )?;
            Ok(json!({
                "proposed": true,
                "algorithmId": record["id"],
                "algorithm": record,
                "next": "다음 예정된 세대에서 제한적으로 시험 사용됩니다. 과거 일치율은 참고 기록이며 채택 근거가 아닙니다.",
            }))
        }
        "unsafe_audit" => {
            let session = ctx
                .store
                .get("session", ctx.session_id)
                .ok_or("자기개선 세션을 찾을 수 없습니다.")?;
            if session["rsiRun"] != true {
                return Err("unsafe_audit는 자기개선 세대에서만 사용할 수 있습니다.".into());
            }
            let scope = string_arg(args, "path").unwrap_or(".");
            let path = resolve_workspace(ctx.workspace, scope)?;
            Ok(crate::engine::unsafe_audit::audit_path(&path))
        }
        "delegate_task" => delegate(ctx, args).await,
        "delegate_tasks" => batch::delegate_many(ctx,args).await,
        "task_status" => task_payload(ctx, args, false),
        "task_result" => task_payload(ctx, args, true),
        "account_status" => Ok(json!({
            "providers": crate::engine::oauth::status(ctx.state),
        })),
        "github_api" => github_api(ctx, args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}

