//! Native tool dispatcher: approval-aware tool execution.
//! Active Rust execution paths imported by tools.rs.
use super::*;

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &Value,
    run_id: &str,
) -> Result<Value, String> {
    let isolated_rsi = ctx.store.get("session",ctx.session_id)
        .is_some_and(|s|s["rsiRun"]==true);
    if isolated_rsi && !rsi_allowed(name) {
        return Err("분리 자기개선 작업에서는 작업공간 내부 도구만 허용됩니다.".into());
    }
    if isolated_rsi && matches!(name,
        "workspace_read_range" | "workspace_write" | "workspace_replace" | "workspace_replace_range" | "workspace_replace_function") {
        let requested=args["path"].as_str().ok_or("경로가 필요합니다.")?;
        let canonical=resolve_workspace(ctx.workspace,requested)?;
        if crate::engine::rsi::hidden_rsi_oracle_path(
            &canonical.to_string_lossy()) {
            return Err("자기개선 모델은 동결 평가 오라클 원문에 접근할 수 없습니다.".into());
        }
    }
    if isolated_rsi && matches!(name, "workspace_write" | "workspace_replace" | "workspace_replace_range" | "workspace_replace_function") {
        let path=args["path"].as_str().unwrap_or("");
        if !crate::engine::rsi::safe_path(path) {
            return Err("자기개선은 허용된 소스코드 경로만 수정할 수 있습니다.".into());
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
            // Return *raw source* for native model edits. Earlier responses
            // embedded line numbers in content; small models copied those
            // numbers into newText and produced uncompilable Rust. The exact
            // start/end line and source SHA are already separate fields.
            let source_excerpt = lines[begin.saturating_sub(1)..end].join("\n");
            let source_excerpt = if !source_excerpt.is_empty()
                && text.ends_with('\n') && end==lines.len() {
                format!("{source_excerpt}\n")
            } else {source_excerpt};
            // The absolute detached worktree path is an internal engine
            // detail. If exposed in the read result, a small model reuses it
            // as the write argument and loses the RSI scoped-write approval.
            let display_path=if isolated_rsi {
                let root=std::fs::canonicalize(ctx.workspace)
                    .map_err(|error|error.to_string())?;
                let file=std::fs::canonicalize(&path)
                    .map_err(|error|error.to_string())?;
                let relative=file.strip_prefix(&root).map_err(|_|
                    "RSI read escaped its isolated workspace".to_string())?;
                let relative=relative.to_str()
                    .ok_or("RSI source path is not valid UTF-8")?;
                if !crate::engine::rsi::safe_path(relative) {
                    return Err("RSI source must be an allowed relative source file".into());
                }
                json!(relative)
            } else {result["path"].clone()};
            let mut output = json!({
                "path": display_path,
                "startLine": begin.max(1),
                "endLine": end,
                "totalLines": lines.len(),
                "sourceSha256":format!("{:x}",Sha256::digest(text.as_bytes())),
                "content": source_excerpt,
                "contentFormat": "raw_source_no_line_prefix",
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
        "workspace_replace_range" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            replace_source_lines(
                ctx,
                &path,
                args["startLine"].as_u64().ok_or("startLine은 1 이상의 정수여야 합니다.")?,
                args["endLine"].as_u64().ok_or("endLine은 1 이상의 정수여야 합니다.")?,
                &required_arg(args, "newText")?,
                &required_arg(args, "expectedSha256")?,
                run_id,
            )
            .await
        }
        "workspace_replace_function" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            replace_source_function(
                ctx, &path,
                &required_arg(args, "functionName")?,
                &required_arg(args, "newText")?,
                &required_arg(args, "expectedSha256")?,
                run_id,
            ).await
        }
        "workspace_replace" => {
            let path=resolve_workspace(ctx.workspace,&required_arg(args,"path")?)?;
            let new_text=required_arg(args,"newText")?;
            if let Some(old)=args["oldText"].as_str().filter(|old|!old.is_empty()){
                replace_text(ctx,&path,old,&new_text,
                    args["expectedSha256"].as_str(),run_id).await
            }else if isolated_rsi {
                // The model must select the modified bytes. Recover malformed
                // edit coordinates only when its authored Rust unambiguously
                // identifies one existing function or one explicit source line.
                let indexed=rsi_pinned_source_sha(ctx,&path,args);
                let expected=args["expectedSha256"].as_str()
                    .or(indexed.as_deref())
                    .ok_or("sourceSha256가 없습니다. 해당 소스를 읽고 다시 수정하세요.")?;
                let intent=rsi_unambiguous_edit_intent(args,&new_text);
                match intent {
                    Some(RsiEditIntent::CompleteFunction(name))=>{
                        let mut receipt=replace_source_function(
                            ctx,&path,&name,&new_text,expected,run_id).await?;
                        receipt["editContractRecovered"]=json!("model_authored_complete_function");
                        receipt["sourceHashRecoveredFromIndex"]=json!(indexed.is_some());
                        Ok(receipt)
                    }
                    Some(RsiEditIntent::SingleLine(line))=>{
                        let mut receipt=replace_source_lines(
                            ctx,&path,line,line,&new_text,expected,run_id).await?;
                        receipt["editContractRecovered"]=json!("explicit_single_line");
                        Ok(receipt)
                    }
                    None=>{
                        let start=rsi_line_number(args,"startLine")
                            .filter(|n|*n>0)
                            .ok_or("oldText 또는 유효한 startLine이 필요합니다.")?;
                        let end=rsi_line_number(args,"endLine")
                            .filter(|n|*n>0)
                            .ok_or("endLine이 없거나 유효하지 않습니다. 정확한 줄 번호 또는 완성된 Rust 함수를 제공하세요.")?;
                        let (start,end)=(start.min(end),start.max(end));
                        replace_source_lines(ctx,&path,start,end,
                            &new_text,expected,run_id).await
                    }
                }
            }else{
                Err("일반 편집은 정확히 일치하는 oldText가 필요합니다.".into())
            }
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
        "task_status" => task_payload(ctx, args, false),
        "task_result" => task_payload(ctx, args, true),
        "account_status" => Ok(json!({
            "providers": crate::engine::oauth::status(ctx.state),
        })),
        "github_api" => github_api(ctx, args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}

