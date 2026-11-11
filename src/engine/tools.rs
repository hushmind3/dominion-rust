use crate::db::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Everything a tool needs in order to run. Tools are pure Rust: no Node,
/// no sidecar process.
pub struct ToolContext<'a> {
    pub workspace: &'a Path,
    pub store: &'a Store,
    pub state: &'a crate::state::AppState,
    pub session_id: &'a str,
    pub agent_id: Option<&'a str>,
    /// Parent turn's model tool permission. Delegation is attenuated, never
    /// widened.
    pub tools_enabled: bool,
    /// Parent turn's overall execution permission.
    pub tools_execution_enabled: bool,
}

/// OpenAI-style tool definitions handed to the model.
pub fn definitions() -> Vec<Value> {
    let object = |properties: Value, required: Vec<&str>| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    };
    let text = |description: &str| json!({ "type": "string", "description": description });

    vec![
        tool("workspace_list", "List files and folders in the workspace", object(json!({"path": text("Relative folder path")}), vec![])),
        tool("workspace_read", "Read a UTF-8 file from the workspace", object(json!({"path": text("Relative file path")}), vec!["path"])),
        tool("workspace_read_range", "Read only a line range from a workspace file. Use the line numbers from the repository index instead of reading a large file end to end.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1, "description":"Inclusive end line; at most 400 lines are returned"})}), vec!["path","startLine"])),
        tool("code_search", "Search the persistent repository index for file paths, symbols and line numbers. Use this before reading a large file.", object(json!({"query": text("Function, type, path or keyword fragment"), "limit": json!({"type":"integer","minimum":1,"maximum":50})}), vec!["query"])),
        tool("workspace_replace_range", "Replace 1-120 previously read lines using sourceSha256 from workspace_read_range. Send only newText, not the old block. Abort if the file changed.", object(json!({"path": text("Relative source path"), "startLine":json!({"type":"integer","minimum":1}), "endLine":json!({"type":"integer","minimum":1}), "newText":text("Replacement source lines"), "expectedSha256":text("Required exact sourceSha256 from workspace_read_range")}), vec!["path","startLine","endLine","newText","expectedSha256"])),
        tool("workspace_replace", "Replace one exact block in a workspace file without resending the whole file (approval required). oldText must occur exactly once.", object(json!({"path": text("Relative file path"), "oldText": text("Exact existing text to replace; must occur exactly once"), "newText": text("Replacement text"), "expectedSha256": text("Optional SHA-256 observed before editing")}), vec!["path","oldText","newText"])),
        tool("workspace_write", "Write a UTF-8 file inside the workspace (approval required)", object(json!({"path": text("Relative file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("file_list", "List any folder on this computer using an absolute path", object(json!({"path": text("Absolute folder path")}), vec!["path"])),
        tool("file_read", "Read any UTF-8 text file using an absolute path", object(json!({"path": text("Absolute file path")}), vec!["path"])),
        tool("file_write", "Create or overwrite any file using an absolute path (approval required)", object(json!({"path": text("Absolute file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("shell_exec", "Run a shell command on this computer and return stdout/stderr (approval required)", object(json!({"command": text("Shell command line"), "cwd": text("Optional absolute working directory")}), vec!["command"])),
        tool("terminal_exec", "Run an executable with an argument array without a shell (approval required)", object(json!({"command": text("Executable"), "args": json!({"type": "array", "items": {"type": "string"}})}), vec!["command"])),
        tool("web_search", "Search the public web for current information", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("http_fetch", "Fetch a URL and return its readable text content", object(json!({"url": text("HTTP or HTTPS URL")}), vec!["url"])),
        tool("memory_store", "Save a grounded memory for this agent. Quote the user's exact words; invented facts are rejected. Shared memories are managed by the user.", object(json!({"text": text("Fact to remember, maximum 1600 characters"), "evidenceQuote": text("Exact quote from the current user message supporting the fact"), "sourceMessageId": text("Optional earlier source message ID"), "type": json!({"type":"string","enum":["fact","preference","decision","procedure","lesson"]}), "key": text("Optional stable fact key; a new value replaces the previous value for this key"), "scope": json!({"type":"string","enum":["agent"]})}), vec!["text", "evidenceQuote"])),
        tool("memory_search", "Find saved knowledge relevant to a topic", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("skill_propose", "Draft a reusable skill for user review", object(json!({"name": text("Skill name"), "instructions": text("Reusable instructions")}), vec!["name", "instructions"])),
        tool("algorithm_propose", "Propose domain selection and optional source-targeting formulas. Trial is independent and does not activate a proposal.", object(json!({
            "name": text("Algorithm name"),
            "expression": text("Domain score over attempts, accepted, regressions, inconclusive, novelty, successRate, usefulGenerations, barrenRate, tokenCost, objectiveMatch, unresolved, exploration"),
            "targetExpression": text("Optional file score over pathEvidence, symbolEvidence, summaryEvidence, bodyEvidence, productiveEvidence, domainEvidence, repeatPenalty, novelty, sourceCost"),
            "description": text("Why this formula should choose better improvement targets"),
            "evidence": json!({"type":"array","items":{"type":"string"},"description":"Observed failures or regressions that justify the formula"})
        }), vec!["expression"])),
        tool("unsafe_audit", "Scan workspace Rust files for unsafe blocks, functions, impls, traits or extern items without a preceding SAFETY justification. Read-only; available only during RSI generations.", object(json!({
            "path": text("Optional relative file or folder; defaults to the workspace root")
        }), vec![])),
        tool("rsi_method_propose", "Save a reusable executable procedure learned during RSI research. Existing procedure nodes run through the normal tool runtime in later scheduled work; registration is not improvement evidence.", object(json!({
            "name":text("Reusable method name"),
            "description":text("Principle, source and limits of the method"),
            "domain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "verification":json!({"type":"object","description":"Existing procedure format: type=procedure, appliesTo.requestContains, nodes [{id,tool,arguments}], edges [{from,to}], after [{kind:file_equals/file_contains/file_exists/tool_called,path,text,tool}]. Arguments can bind a prior node result via $node.field. Include concrete postconditions."})
        }), vec!["name","domain","verification"])),
        tool("delegate_task", "Delegate a self-contained task to another agent and optionally wait for its result", object(json!({
            "agentId": text("Target agent id"),
            "prompt": text("Task instructions"),
            "wait": json!({"type":"boolean","description":"Wait for completion (default true)"}),
            "timeoutSeconds": json!({"type":"number","description":"5-300 seconds"})
        }), vec!["agentId", "prompt"])),
        tool("task_status", "Check a delegated task's status", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("task_result", "Read a delegated task's result, artifacts and final answer", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("account_status", "List OAuth accounts connected to DOMINION", object(json!({}), vec![])),
        tool("browser_control", "Control a local Chrome/Chromium window: open pages, observe text and interactive elements, click, type, press keys and capture screenshots (approval required)", object(json!({
            "action": json!({"type": "string", "enum": ["status", "open", "observe", "click", "type", "press", "screenshot", "close"]}),
            "url": text("URL to open"),
            "index": json!({"type": "number", "description": "Element index returned by observe"}),
            "text": text("Text to type into the indexed element"),
            "key": text("Key to press: Enter, Escape, Tab, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Backspace")
        }), vec!["action"])),
        tool("github_api", "Call the GitHub REST API as the connected account. Write methods require approval (approval required)", object(json!({"method": json!({"type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"]}), "path": text("GitHub API path beginning with /"), "body": json!({"type": "object"})}), vec!["method", "path"])),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "workspace_write"
            | "workspace_replace"
            | "workspace_replace_range"
            | "file_write"
            | "shell_exec"
            | "terminal_exec"
            | "github_api"
            | "browser_control"
    )
}

pub fn rsi_allowed(name:&str)->bool{
    matches!(name,"code_search"|"workspace_read_range"|"workspace_replace"|"workspace_replace_range"|"workspace_write"|"web_search"|"http_fetch"|"algorithm_propose"|"unsafe_audit"|"rsi_method_propose")
}

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
        "workspace_read_range" | "workspace_write" | "workspace_replace" | "workspace_replace_range") {
        let requested=args["path"].as_str().ok_or("경로가 필요합니다.")?;
        let canonical=resolve_workspace(ctx.workspace,requested)?;
        if crate::engine::rsi::hidden_rsi_oracle_path(
            &canonical.to_string_lossy()) {
            return Err("자기개선 모델은 동결 평가 오라클 원문에 접근할 수 없습니다.".into());
        }
    }
    if isolated_rsi && matches!(name, "workspace_write" | "workspace_replace" | "workspace_replace_range") {
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
        "workspace_replace" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            replace_text(
                ctx,
                &path,
                &required_arg(args, "oldText")?,
                &required_arg(args, "newText")?,
                args["expectedSha256"].as_str(),
                run_id,
            )
            .await
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

async fn github_api(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let token = crate::engine::oauth::access_token(ctx.state, "github")
        .ok_or("GitHub 계정이 연결되지 않았습니다. 설정 > 계정 연동에서 연결하세요.")?;
    let method = string_arg(args, "method").unwrap_or("GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("지원하지 않는 HTTP 메서드입니다.".into());
    }
    let path = required_arg(args, "path")?;
    if !path.starts_with('/') || path.contains("..") {
        return Err("GitHub API 경로는 / 로 시작해야 합니다.".into());
    }
    let method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| error.to_string())?;
    let mut request = reqwest::Client::new()
        .request(method, format!("https://api.github.com{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(30));
    if let Some(body) = args.get("body").filter(|value| !value.is_null()) {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
    Ok(json!({
        "status": status,
        "body": parsed.to_string().chars().take(20_000).collect::<String>(),
    }))
}

const MAX_DELEGATION_DEPTH: usize = 4;

/// Number of delegated sessions between the owner-facing session and this one.
/// A chain cannot loop forever, so a cycle is cut defensively.
fn delegation_depth(store: &Store, session_id: &str) -> usize {
    let mut depth = 0;
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(session_id.to_string());
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(session) = store.get("session", &id) else {
            break;
        };
        if session["delegated"].as_bool() == Some(true) {
            depth += 1;
        }
        current = session["parentSessionId"].as_str().map(str::to_string);
    }
    depth
}

async fn delegate(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let agent_id = required_arg(args, "agentId")?;
    // A delegated worker cannot delegate beyond a bounded depth. This keeps a
    // runaway chain from consuming unbounded workers.
    if delegation_depth(ctx.store, ctx.session_id) >= MAX_DELEGATION_DEPTH {
        return Err(format!(
            "위임 깊이 제한({MAX_DELEGATION_DEPTH})에 도달했습니다. 상위 작업으로 결과를 모아 처리하세요."
        ));
    }
    let prompt = required_arg(args, "prompt")?;
    let agent = ctx
        .state
        .store
        .get("agent", &agent_id)
        .ok_or_else(|| format!("위임 대상 에이전트를 찾을 수 없습니다: {agent_id}"))?;
    let wait = args["wait"].as_bool().unwrap_or(true);
    let timeout = args["timeoutSeconds"]
        .as_u64()
        .unwrap_or(90)
        .clamp(5, 300);

    let session = ctx
        .state
        .store
        .put(
            "session",
            &json!({
                "title": format!("Delegate · {}", prompt.chars().take(36).collect::<String>()),
                "agentId": agent_id,
                "providerId": agent["providerId"],
                "model": agent["model"],
                "parentSessionId": ctx.session_id,
                "delegated": true,
            }),
        )
        .ok_or("위임 세션을 만들 수 없습니다.")?;
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let job = crate::engine::jobs::enqueue(
        ctx.state,
        "prompt",
        json!({
            "prompt": prompt,
            "agentId": agent_id,
            "providerId": agent["providerId"],
            "model": agent["model"],
            "sessionId": session_id,
            "toolsEnabled": ctx.tools_enabled && ctx.tools_execution_enabled,
            "toolsExecutionEnabled": ctx.tools_execution_enabled,
            "parentSessionId": ctx.session_id,
            "parentAgentId": ctx.agent_id,
            "delegation": true,
        }),
    )
    .ok_or("위임 작업을 만들 수 없습니다.")?;
    let job_id = job["id"].as_str().unwrap_or("").to_string();
    crate::engine::jobs::start_now(ctx.state.clone(), job_id.clone());

    if !wait {
        return Ok(json!({
            "jobId": job_id, "status": "queued", "sessionId": session_id,
            "hint": "task_status 또는 task_result로 확인하세요."
        }));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        if ctx.state.is_cancelled(ctx.session_id) {
            return Err("실행이 중단되었습니다. 위임 작업은 백그라운드에서 계속됩니다.".into());
        }
        let Some(current) = ctx.state.store.get("job", &job_id) else {
            return Err("위임 작업 기록이 사라졌습니다.".into());
        };
        let status = current["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled" | "needs_review") {
            return task_payload(ctx, &json!({"jobId": job_id}), true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(json!({
                "jobId": job_id, "status": status, "sessionId": session_id,
                "hint": "아직 실행 중입니다. task_result로 나중에 확인하세요."
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

fn task_payload(ctx: &ToolContext<'_>, args: &Value, with_result: bool) -> Result<Value, String> {
    let job_id = required_arg(args, "jobId")?;
    let job = ctx
        .state
        .store
        .get("job", &job_id)
        .ok_or_else(|| format!("작업을 찾을 수 없습니다: {job_id}"))?;
    // A job result is visible only to the session that created it or to the
    // parent session that delegated it.
    let owned = job["input"]["sessionId"].as_str() == Some(ctx.session_id)
        || job["input"]["parentSessionId"].as_str() == Some(ctx.session_id);
    if !owned {
        return Err("이 작업에 접근할 권한이 없습니다.".into());
    }
    let status = job["status"].as_str().unwrap_or("unknown");
    let mut payload = json!({
        "jobId": job_id,
        "kind": job["kind"],
        "status": status,
        "stepIndex": job["stepIndex"],
        "stepTotal": job["stepTotal"],
        "error": job["error"],
        "result": job["result"],
    });
    if !with_result || !matches!(status, "completed" | "failed" | "needs_review") {
        return Ok(payload);
    }
    let Some(session_id) = job
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .or_else(|| job["sessionId"].as_str())
    else {
        return Ok(payload);
    };
    let messages = ctx.state.store.messages(session_id);
    let answer = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("")
        .chars()
        .take(8000)
        .collect::<String>();
    let operations = ctx
        .state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|operation| {
            operation["runId"].as_str().is_some()
                && operation["sessionId"].as_str() == Some(session_id)
                && operation["status"] == "completed"
        })
        .map(|operation| {
            json!({"tool":operation["tool"],"arguments":operation["arguments"],"result":operation["result"]})
        })
        .take(20)
        .collect::<Vec<_>>();
    payload["sessionId"] = json!(session_id);
    payload["content"] = json!(answer);
    payload["operations"] = json!(operations);
    Ok(payload)
}

fn required_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing argument: {key}"))
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|value| value.as_str())
}

pub const WORKSPACE_ESCAPE: &str = "Path escapes the workspace";

/// Reject `..` traversal and symlink escapes for workspace-scoped tools.
pub(super) fn resolve_workspace(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
    let base = canonical_target(workspace).map_err(|error| error.to_string())?;
    let candidate = Path::new(relative);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WORKSPACE_ESCAPE.into());
    }
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };
    let resolved = canonical_target(&joined).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&base) {
        return Err(WORKSPACE_ESCAPE.into());
    }
    Ok(resolved)
}

/// Resolve existing ancestors even when the final file has not been created.
fn canonical_target(path: &Path) -> std::io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(error);
            }
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(error);
            };
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            Ok(canonical_target(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

async fn list_dir(path: &Path) -> Result<Value, String> {
    let mut entries = Vec::new();
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    while let Ok(Some(entry)) = reader.next_entry().await {
        let metadata = entry.metadata().await.ok();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "type": if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) { "dir" } else { "file" },
            "size": metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        }));
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Ok(json!({ "path": path.to_string_lossy(), "entries": entries }))
}

async fn read_text(path: &Path) -> Result<Value, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err("Target is not a regular file".into());
    }
    if metadata.len() > 1_500_000 {
        return Err("File is larger than 1.5 MB".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy(), "content": text }))
}

fn preflight_policy(ctx: &ToolContext<'_>, path: &Path, run_id: &str) -> Option<Value> {
    let run = ctx.store.get("execution", run_id)?;
    let policy = &run["preflightPolicy"];
    (crate::engine::policy::is_preflight_kernel(&policy["parameters"]["kernel"])
        && policy["targetPath"].as_str().is_none_or(|target| {
            let target = Path::new(target);
            path == target || canonical_target(path).is_ok_and(|path| path == target)
        }))
        .then(|| policy.clone())
}

async fn observed_read(ctx: &ToolContext<'_>, path: &Path, run_id: &str) -> Result<Value, String> {
    if preflight_policy(ctx, path, run_id).is_none() {
        return read_text(path).await;
    }
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut result = read_text(&canonical).await?;
    result["path"] = json!(path.to_string_lossy());
    let text = result["content"].as_str().unwrap_or_default();
    result["fileObservation"] = json!({
        "path": canonical.to_string_lossy(),
        "sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
    });
    Ok(result)
}

fn search_code_index(
    ctx: &ToolContext<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, String> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Err("code_search query는 비어 있을 수 없습니다.".into());
    }
    let session=ctx.store.get("session",ctx.session_id).unwrap_or(Value::Null);
    let bound=session["rsiCodeIndexId"].as_str()
        .filter(|_|session["rsiRun"]==true)
        .and_then(|id|ctx.store.get("code_index",id))
        .filter(|meta|meta["sourceDir"].is_string() && meta["sourceDir"]==session["rsiSourceDir"]);
    // Never search whichever unrelated repository happened to be indexed last.
    let meta=match bound.or_else(||crate::engine::code_index::get(ctx.state,ctx.workspace)) {
        Some(meta)=>meta,
        None=>crate::engine::code_index::refresh(ctx.state,ctx.workspace)?,
    };
    let index_id = meta["id"].as_str().unwrap_or("");
    let terms=needle.split_whitespace().map(|term|term.trim_matches(['`','\"','\'','(',')',',']))
        .filter(|term|!term.is_empty() && !matches!(*term,"fn"|"pub"|"async"|"struct"|"enum"))
        .collect::<Vec<_>>();
    let score=|value:&str|{
        let value=value.to_lowercase();
        if value==needle {1000} else {
            u64::from(value.contains(&needle))*200 + terms.iter().map(|term|
                if value==*term {100} else {u64::from(value.contains(term))*20}).sum::<u64>()
        }
    };
    let mut matches=Vec::new();
    for file in ctx.store.all("code_index_file") {
        if file["indexId"].as_str()!=Some(index_id) {continue;}
        let path=file["path"].as_str().unwrap_or("");
        if session["rsiRun"]==true
            && crate::engine::rsi::hidden_rsi_oracle_path(path) {
            continue;
        }
        let rank=score(path);
        if rank>0 {matches.push((rank,json!({"path":path,"kind":"file","name":path,"line":1})));}
        for symbol in file["symbols"].as_array().into_iter().flatten() {
            let name=symbol["name"].as_str().unwrap_or("");let rank=score(name);
            if rank==0 {continue;}
            matches.push((rank,json!({"path":path,"kind":symbol["kind"],"name":name,"line":symbol["line"]})));
        }
    }
    matches.sort_by(|(a,left),(b,right)|b.cmp(a)
        .then_with(||left["path"].as_str().cmp(&right["path"].as_str()))
        .then_with(||left["line"].as_u64().cmp(&right["line"].as_u64())));
    matches.dedup_by(|(_,a),(_,b)|a==b);
    matches.truncate(limit);
    let matches=matches.into_iter().map(|(_,row)|row).collect::<Vec<_>>();
    Ok(json!({"query":query,"indexId":index_id,"indexedFiles":meta["files"],"matchCount":matches.len(),"matches":matches}))
}

/// Native small-model edit: the verified full-file hash binds a concise
/// line replacement to exactly the source observed by workspace_read_range.
/// checked_write repeats the hash comparison under its file mutation lock.
async fn replace_source_lines(
    ctx:&ToolContext<'_>, path:&Path, start:u64, end:u64,
    new_text:&str, expected_sha256:&str, run_id:&str,
) -> Result<Value,String> {
    if start==0 || end<start || end.saturating_sub(start)>=120 {
        return Err("workspace_replace_range: 1~120개의 유효한 연속 줄 범위를 지정하세요.".into());
    }
    if expected_sha256.len()!=64 || !expected_sha256.bytes().all(|b|b.is_ascii_hexdigit()) {
        return Err("workspace_replace_range: workspace_read_range가 반환한 sourceSha256이 필요합니다.".into());
    }
    let before=tokio::fs::read_to_string(path).await
        .map_err(|e|format!("{}: {e}",path.display()))?;
    let observed=format!("{:x}",Sha256::digest(before.as_bytes()));
    let expected=expected_sha256.to_ascii_lowercase();
    if observed!=expected {
        return Err(format!(
            "workspace_replace_range: 소스가 읽은 뒤 변경되었습니다 (expected={expected}, observed={observed}). 해당 줄을 다시 읽어 재시도하세요."
        ));
    }
    let lines=before.split_inclusive('\n').collect::<Vec<_>>();
    let first=usize::try_from(start-1).map_err(|e|e.to_string())?;
    let last=usize::try_from(end).map_err(|e|e.to_string())?;
    if last>lines.len() {
        return Err(format!(
            "workspace_replace_range: {start}-{end}는 파일의 실제 {}줄을 벗어납니다. 다시 읽으세요.",
            lines.len()
        ));
    }
    let prior=lines[first..last].concat();
    let mut replacement=new_text.to_string();
    if prior.ends_with('\n') && !replacement.is_empty() && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    if prior==replacement {
        return Err("workspace_replace_range: 원문과 수정 결과가 동일하여 변경 0건입니다.".into());
    }
    let updated=format!("{}{}{}",
        lines[..first].concat(),replacement,lines[last..].concat());
    let mut result=checked_write(ctx,path,&updated,run_id,true,Some(&expected)).await?;
    if result["preflightSyntaxRejected"]==true || result["preflightScopeRejected"]==true {return Ok(result);}
    if result["written"]!=true {
        return Err(format!(
            "workspace_replace_range 쓰기 거부: {}",
            result.to_string().chars().take(1400).collect::<String>()
        ));
    }
    result["replaced"]=json!(true);
    result["replacedLines"]=json!(end-start+1);
    result["writtenSha256"]=json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    if let Some(parse)=rsi_rust_parse_receipt(ctx,path).await {
        result["syntaxValid"]=parse["valid"].clone();
        result["syntaxCheck"]=parse;
    }
    Ok(result)
}

/// Reject malformed model-authored Rust *before* mutating the source file.
///
/// The old post-write rustfmt receipt diagnosed syntax errors only after a
/// malformed tool call had already replaced real source. During RSI this
/// poisoned later source reads, consumed entire generation quotas and left
/// unrecoverable candidate diffs. Preflight runs while checked_write holds the
/// file mutation lock; the original bytes survive every parse failure.
/// Rustfmt is a syntax-only guard. Compiling and frozen paired evaluation
/// remain independent requirements for adopting a candidate.
async fn rsi_rust_parse_preview(
    ctx: &ToolContext<'_>, path: &Path, content: &str,
) -> Result<(), String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs")
        || ctx.store.get("session", ctx.session_id)
            .is_none_or(|session| session["rsiRun"] != true)
    {
        return Ok(());
    }
    use tokio::io::AsyncWriteExt;
    let mut command = tokio::process::Command::new("rustfmt");
    command.args(["--edition", "2021", "--emit", "stdout",
            "--config", "skip_children=true"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let check = async {
        let mut child = command.spawn()
            .map_err(|error| format!("rsi_rust_parser_unavailable: {error}"))?;
        let mut stdin = child.stdin.take()
            .ok_or("rsi_rust_parser_unavailable: parser stdin missing")?;
        stdin.write_all(content.as_bytes()).await
            .map_err(|error| format!("rsi_rust_parser_io: {error}"))?;
        drop(stdin);
        child.wait_with_output().await
            .map_err(|error| format!("rsi_rust_parser_io: {error}"))
    };
    let output = tokio::time::timeout(Duration::from_secs(15), check).await
        .map_err(|_| "rsi_rust_parser_timeout: source not modified".to_string())??;
    if output.status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "rsi_rust_syntax_rejected: Rust syntax invalid; source NOT modified. \
         Change the replacement boundaries or newText using the exact existing \
         source, then call an edit tool again. rustfmt: {}",
        diagnostic.chars().take(1600).collect::<String>()
    ))
}

/// Preserve existing sibling functions during narrowly scoped RSI source edits.
///
/// In live model campaigns, broad replacement ranges removed unrelated
/// functions even when Rust syntax stayed valid. When the owner explicitly
/// targets an existing top-level function, keep its sibling definitions
/// unchanged. This is scoped to named-function RSI objectives only; the
/// normal exploratory and multi-file improvement paths are unaffected.
fn rsi_top_level_functions(source: &str) -> Vec<(String, &str)> {
    let mut starts = Vec::<(String, usize)>::new();
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let declaration = line.trim_end_matches(['\r', '\n']);
        if let Some((prefix, tail)) = declaration.split_once("fn ") {
            if matches!(prefix, "" | "pub " | "pub(crate) " | "async " |
                "pub async " | "pub(crate) async " | "unsafe " |
                "pub unsafe " | "const " | "pub const ") {
                let name = tail.chars()
                    .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                    .collect::<String>();
                if !name.is_empty() && tail[name.len()..].trim_start().starts_with('(') {
                    starts.push((name, offset));
                }
            }
        }
        offset += line.len();
    }
    starts.iter().enumerate().map(|(index, (name, start))| {
        let end = starts.get(index + 1).map_or(source.len(), |(_, next)| *next);
        (name.clone(), &source[*start..end])
    }).collect()
}

async fn rsi_rust_scope_preview(
    ctx: &ToolContext<'_>, path: &Path, updated: &str,
) -> Result<(), String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Ok(());
    }
    let Some(session) = ctx.store.get("session", ctx.session_id) else {
        return Ok(());
    };
    if session["rsiRun"] != true || session["rsiExplicitFocus"] != true {
        return Ok(());
    }
    let Some(objective) = session["rsiObjective"].as_str() else {
        return Ok(());
    };
    let Ok(previous) = tokio::fs::read_to_string(path).await else {
        return Ok(()); // New source files have no existing siblings.
    };
    let before = rsi_top_level_functions(&previous);
    let mut targets = before.iter()
        .filter_map(|(name, _)| objective.find(&format!("{name}("))
            .map(|position| (position, name.as_str())))
        .collect::<Vec<_>>();
    targets.sort_unstable();
    let Some((_, target)) = targets.first() else {
        return Ok(()); // No concrete function was named.
    };
    let after = rsi_top_level_functions(updated);
    if !after.iter().any(|(name, _)| name == target) {
        return Err(format!(
            "rsi_scope_rejected: required function {target}() disappeared; source NOT modified. Edit the exact target range, not its siblings."
        ));
    }
    for (name, original_body) in &before {
        if name.as_str() == *target {
            continue;
        }
        if after.iter().find(|(after_name, _)| after_name == name)
            .is_none_or(|(_, new_body)| *new_body != *original_body) {
            return Err(format!(
                "rsi_scope_rejected: objective targets {target}(), but edit changes or removes sibling {name}(); source NOT modified. Re-read and replace only the targeted function."
            ));
        }
    }
    Ok(())
}

/// Read-only Rust parser immediately after a model-authored source
/// mutation. Same-turn feedback is not an independent quality verdict.
async fn rsi_rust_parse_receipt(ctx:&ToolContext<'_>,path:&Path)->Option<Value> {
    if path.extension().and_then(|ext|ext.to_str())!=Some("rs")
        || ctx.store.get("session",ctx.session_id)?["rsiRun"]!=true {
        return None;
    }
    let mut command=tokio::process::Command::new("rustfmt");
    command.args(["--edition","2021","--emit","stdout",
        "--config","skip_children=true"])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let output=tokio::time::timeout(Duration::from_secs(12),command.output())
        .await.ok()?.ok()?;
    if output.status.success() {
        return Some(json!({"valid":true,"tool":"rustfmt_syntax_read_only"}));
    }
    let diagnostic=String::from_utf8_lossy(&output.stderr).to_string();
    if !diagnostic.contains("error") {return None;}
    let line=diagnostic.lines().find_map(|row|{
        let row=row.trim();
        if !row.starts_with("-->"){return None;}
        let (prefix,col)=row.rsplit_once(':')?;
        col.trim().parse::<u64>().ok()?;
        let (_,line)=prefix.rsplit_once(':')?;
        line.trim().parse::<u64>().ok()
    });
    Some(json!({"valid":false,"tool":"rustfmt_syntax_read_only",
        "line":line,
        "diagnostic":diagnostic.chars().take(1800).collect::<String>()}))
}

async fn replace_text(
    ctx: &ToolContext<'_>,
    path: &Path,
    old_text: &str,
    new_text: &str,
    expected_sha256: Option<&str>,
    run_id: &str,
) -> Result<Value, String> {
    if old_text.is_empty() {
        return Err("oldText는 비어 있을 수 없습니다.".into());
    }
    if old_text == new_text {
        return Err("workspace_replace: oldText와 newText가 동일하여 소스 변경이 없습니다.".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let occurrences = text.matches(old_text).count();
    if occurrences != 1 {
        return Err(format!(
            "workspace_replace 실행 실패: oldText 출현 {occurrences}회 (정확히 1회 필요), 파일 {}. workspace_read_range로 실제 소스와 들여쓰기·줄바꿈을 다시 확인한 뒤 교정해 재호출하세요.",
            path.display()
        ));
    }
    let updated = text.replacen(old_text, new_text, 1);
    let mut result = checked_write(ctx, path, &updated, run_id, true, expected_sha256).await?;
    if result["preflightSyntaxRejected"]==true || result["preflightScopeRejected"]==true {return Ok(result);}
    // checked_write can return an error *inside* an Ok(Value) for a stale
    // revision or a preflight refusal. Do not label that as a real edit.
    if result["written"] != true {
        return Err(format!(
            "workspace_replace 실행 거부 (수정 0회): {}. 최신 파일을 읽고 인자를 고쳐 다시 시도하세요.",
            result.to_string().chars().take(1400).collect::<String>()
        ));
    }
    result["replaced"] = json!(true);
    result["replacedChars"] = json!(old_text.chars().count());
    result["writtenSha256"] = json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    if let Some(parse)=rsi_rust_parse_receipt(ctx,path).await {
        result["syntaxValid"]=parse["valid"].clone();
        result["syntaxCheck"]=parse;
    }
    Ok(result)
}

async fn checked_write(
    ctx: &ToolContext<'_>,
    path: &Path,
    content: &str,
    run_id: &str,
    workspace_write: bool,
    expected_sha256: Option<&str>,
) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let policy = preflight_policy(ctx, path, run_id);
    let canonical = canonical_target(path);
    let lock_path = match &canonical {
        Ok(path) => path.as_path(),
        Err(error) if policy.is_some() => return Err(error.to_string()),
        Err(_) => path,
    };
    let lock = {
        let mut locks = ctx.state.file_mutations.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(lock_path).and_then(std::sync::Weak::upgrade) {
            lock
        } else {
            let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
            locks.insert(lock_path.to_path_buf(), std::sync::Arc::downgrade(&lock));
            lock
        }
    };
    let _guard = lock.lock().await;
    if let Err(error)=rsi_rust_scope_preview(ctx,path,content).await {
        if error.starts_with("rsi_scope_rejected:") {
            // Keep the same-turn native editor alive and preserve its read SHA.
            return Ok(json!({"path":path.to_string_lossy(),
                "written":false,"sourceUnchanged":true,
                "preflightScopeRejected":true,"error":error}));
        }
        return Err(error);
    }
    if let Err(error)=rsi_rust_parse_preview(ctx,path,content).await {
        if error.starts_with("rsi_rust_syntax_rejected:") {
            // A recoverable model proposal failure, not a changed file.
            // Return a typed receipt so same-turn editing can continue without
            // launching expensive generic model adaptation.
            return Ok(json!({
                "path":path.to_string_lossy(),"written":false,
                "preflightSyntaxRejected":true,"sourceUnchanged":true,
                "syntaxValid":false,
                "syntaxCheck":{"valid":false,"tool":"rustfmt_syntax_preflight",
                    "diagnostic":error.chars().take(1800).collect::<String>()},
                "error":error,
            }));
        }
        return Err(error);
    }
    let Some(policy) = policy else {
        // Even without the opt-in guard, a caller that supplies a frozen
        // version gets a compare-before-write under the same per-file lock.
        let Some(expected) = expected_sha256 else {
            return write_text(path, content).await;
        };
        let canonical = canonical.map_err(|error| error.to_string())?;
        let bytes = match tokio::fs::read(&canonical).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(json!({
                    "error": format!("파일을 다시 읽지 못했습니다: {error}"),
                    "staleVersion": true,
                    "expectedSha256": expected,
                }))
            }
        };
        let observed = format!("{:x}", Sha256::digest(&bytes));
        if observed != expected {
            return Ok(json!({
                "error": "파일이 읽은 뒤 변경되었습니다. 다시 읽고 판단하세요.",
                "staleVersion": true,
                "expectedSha256": expected,
                "observedSha256": observed,
                "path": path.to_string_lossy(),
            }));
        }
        tokio::fs::write(&canonical, content)
            .await
            .map_err(|error| format!("{}: {error}", path.display()))?;
        return Ok(json!({
            "path": path.to_string_lossy(),
            "bytes": content.len(),
            "written": true,
            "conditionalSha256": observed,
        }));
    };
    let canonical = canonical.map_err(|error| error.to_string())?;
    let blocked = |reason: &str| {
        json!({"error": format!("Preflight: {reason}. Read this exact file with workspace_read or file_read, then retry with its current content."),
            "preflightBlocked": true, "preflightRevision": policy["revision"]})
    };
    let target = canonical.to_string_lossy();
    let operations = ctx.store.execution_operations(run_id)?;
    let previous = operations.iter().rev().find(|operation| {
        (operation["status"] == "completed" || operation["result"]["preflightBlocked"] == true)
            && (operation["fileObservation"]["path"].as_str() == Some(target.as_ref())
                || operation["result"]["preflightPath"].as_str() == Some(target.as_ref()))
    });
    let metadata = match tokio::fs::metadata(&canonical).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if previous.is_some() {
                return Ok(blocked("the previously observed target disappeared"));
            }
            if let Some(parent) = canonical
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            // An absent target can be created, but must not overwrite a file
            // that appeared between the probe and the open.
            let mut file = match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&canonical)
                .await
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(blocked("the target now exists"));
                }
                Err(error) => return Err(error.to_string()),
            };
            file.write_all(content.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            file.flush().await.map_err(|error| error.to_string())?;
            return Ok(
                json!({"path": path.to_string_lossy(), "bytes": content.len(), "written": true,
                "preflightPath": target}),
            );
        }
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Err("Preflight target is not a regular file".into());
    }
    let revision = previous.and_then(|operation| operation["fileObservation"]["sha256"].as_str());
    let inline_observation = workspace_write
        && policy["parameters"]["kernel"] == crate::engine::preflight_fixture::GUARD_KERNEL
        && metadata.len() <= 16 * 1024;
    if revision.is_none() && !inline_observation {
        return Ok(blocked("there is no successful read of the existing target in this run"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&canonical)
        .await
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let read_limit = if revision.is_none() { 16 * 1024 + 1 } else { 1_500_001 };
    (&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    let observed = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() > 1_500_000 || revision != Some(observed.as_str()) {
        let mut result = blocked(if revision.is_some() {
            "the target changed after the read"
        } else {
            "there is no successful read of the existing target in this run"
        });
        if inline_observation && bytes.len() <= 16 * 1024 {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                result["error"] = json!("Preflight: write skipped. The current file is below. Apply only the requested change and retry this write using the same path.");
                result["currentContent"] = json!(text);
                result["fileObservation"] = json!({"path":target,"sha256":observed});
            }
        }
        return Ok(result);
    }
    if let Some(expected) = expected_sha256 {
        if expected != observed {
            let mut result = blocked("the target changed after the frozen version");
            result["staleVersion"] = json!(true);
            result["expectedSha256"] = json!(expected);
            result["observedSha256"] = json!(observed);
            return Ok(result);
        }
    }
    use tokio::io::AsyncSeekExt;
    file.rewind().await.map_err(|error| error.to_string())?;
    file.write_all(content.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    file.flush().await.map_err(|error| error.to_string())?;
    file.set_len(content.len() as u64)
        .await
        .map_err(|error| error.to_string())?;
    Ok(
        json!({"path": path.to_string_lossy(), "bytes": content.len(), "written": true,
        "preflightRevision": policy["revision"], "preflightPath": target, "observedSha256": observed}),
    )
}

async fn write_text(path: &Path, content: &str) -> Result<Value, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes": content.len(),
        "written": true
    }))
}

/// A tool command gets a private Unix process group. Killing only the shell
/// leaves its children running after Stop; the guard covers cancellation,
/// timeout and I/O errors and is disarmed only after a completed command.
#[cfg(unix)]
struct ToolProcessGroup {
    id: Option<i32>,
}
#[cfg(unix)]
impl ToolProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self {
            id: pid
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1),
        }
    }
    fn disarm(&mut self) {
        self.id = None;
    }
}
#[cfg(unix)]
impl Drop for ToolProcessGroup {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // This is the exact child PID created with process_group(0).
            // A negative PID targets that private group, never our own group;
            // zero and special global process IDs are excluded by new().
            // SIGKILL is 9 on the supported macOS/Linux Unix hosts.
            // SAFETY: the target is the private process group created above;
            // the group id is positive and our own group is never targeted.
            unsafe {
                kill(-id, 9);
            }
        }
    }
}

async fn read_command_output(
    mut stream: impl tokio::io::AsyncRead + Unpin,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    // Keep enough UTF-8 bytes for 20,001 characters so output_payload can
    // preserve its existing 20,000-character truncation indicator. Continue
    // draining after this cap so a chatty child cannot fill a pipe or memory.
    const MAX_RETAINED_BYTES: usize = 4 * 20_001;
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(kept);
        }
        let retain = count.min(MAX_RETAINED_BYTES.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..retain]);
    }
}

pub(crate) async fn run_command(
    command: &mut tokio::process::Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let mut group = ToolProcessGroup::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or("Command stdout is not available")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Command stderr is not available")?;
    let outcome = tokio::time::timeout(limit, async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_command_output(stdout),
            read_command_output(stderr)
        )?;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match outcome {
        Ok(Ok(output)) => {
            #[cfg(unix)]
            group.disarm();
            Ok(output)
        }
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(if limit.subsec_nanos() == 0 {
            format!("Command timed out after {}s", limit.as_secs())
        } else {
            format!("Command timed out after {}ms", limit.as_millis())
        }),
    }
}

async fn shell(command: &str, cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    #[cfg(windows)]
    let mut child = {
        let mut child = Command::new("cmd");
        child.args(["/C", command]);
        child
    };
    #[cfg(not(windows))]
    let mut child = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut child = Command::new(shell);
        child.args(["-lc", command]);
        child
    };
    child.current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

async fn executable(command: &str, args: &[String], cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    if !command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:".contains(c))
    {
        return Err("Invalid executable".into());
    }
    let mut child = Command::new(command);
    child.args(args).current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

fn output_payload(output: &std::process::Output) -> Value {
    let clip = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes).to_string();
        if text.chars().count() > 20_000 {
            text.chars().take(20_000).collect::<String>() + "\n… (truncated)"
        } else {
            text
        }
    };
    json!({
        "exitCode": output.status.code(),
        "stdout": clip(&output.stdout),
        "stderr": clip(&output.stderr),
    })
}

const MAX_HTTP_FETCH_BYTES: usize = 256 * 1024;

/// Exclude local, reserved, routing infrastructure and non-public addresses.
/// A request is pinned to the checked DNS answers to close rebinding gaps.
fn public_fetch_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0 || a == 10 || a == 127 || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || b == 0
                    || (b == 88 && c == 99) || (b == 0 && c == 2)))
                || (a == 198 && ((18..=19).contains(&b) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        std::net::IpAddr::V6(ip) => {
            // Reject IPv4-mapped and compatible addresses, including private
            // addresses embedded in an IPv6 URL.
            if ip.to_ipv4().is_some() { return false; }
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && segments[0] != 0x2002  // 6to4 tunnels may encode private IPv4
                && !(segments[0] == 0x2001
                    && (segments[1] == 0 || segments[1] == 0x0db8))
        }
    }
}

fn validated_http_target(url: &str) -> Result<(reqwest::Url, String, u16), String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("HTTP fetch accepts only public HTTP(S) URLs without credentials".into());
    }
    let host = parsed.host_str().ok_or("HTTP fetch URL requires a hostname")?
        .trim_matches(|ch| ch == '[' || ch == ']')
        .trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host == "localhost"
        || [".localhost",".local",".internal",".test",".invalid"].iter().any(|suffix|host.ends_with(suffix)) {
        return Err("HTTP fetch cannot access local or internal hosts".into());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !public_fetch_ip(ip) { return Err("HTTP fetch cannot access non-public IP addresses".into()); }
    }
    let port=parsed.port_or_known_default().ok_or("HTTP fetch URL port is missing")?;
    Ok((parsed,host,port))
}

async fn http_fetch(url: &str) -> Result<Value, String> {
    const MAX_REDIRECTS: usize = 3;
    let mut next=url.to_string();
    for hop in 0..=MAX_REDIRECTS {
        let (parsed, host, port) = validated_http_target(&next)?;
        let resolved = tokio::net::lookup_host((host.as_str(),port)).await
            .map_err(|error|format!("HTTP fetch DNS lookup failed: {error}"))?
            .collect::<Vec<_>>();
        if resolved.is_empty() || resolved.iter().any(|address|!public_fetch_ip(address.ip())) {
            return Err("HTTP fetch resolved to a local or non-public address".into());
        }
        // Explicitly review every redirect target with a new DNS lookup.
        // Pin the validated answers and ignore environment proxies.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .resolve_to_addrs(&host, &resolved)
            .timeout(Duration::from_secs(30))
            .build().map_err(|error|error.to_string())?;
        let mut response = client.get(parsed.clone())
            .header("User-Agent", "DOMINION/0.2 (+rust)")
            .send().await.map_err(|error| error.to_string())?;
        if response.status().is_redirection() {
            if hop==MAX_REDIRECTS {
                return Err("HTTP fetch exceeded the redirect limit".into());
            }
            let location=response.headers().get(reqwest::header::LOCATION)
                .and_then(|value|value.to_str().ok())
                .ok_or("HTTP fetch redirect has no valid location")?;
            next=parsed.join(location).map_err(|error|
                format!("HTTP fetch redirect URL is invalid: {error}"))?.to_string();
            continue;
        }
        let status=response.status().as_u16();
        if response.content_length().is_some_and(|length|length>MAX_HTTP_FETCH_BYTES as u64) {
            return Err("HTTP fetch response exceeds the byte limit".into());
        }
        let content_type = response.headers().get("content-type")
            .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())? {
            if chunk.len()>MAX_HTTP_FETCH_BYTES.saturating_sub(bytes.len()) {
                return Err("HTTP fetch response exceeds the byte limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body=String::from_utf8_lossy(&bytes);
        let text=if content_type.contains("html") {strip_html(&body)} else {body.into_owned()};
        return Ok(json!({
            "status":status,"contentType":content_type,
            "text":text.chars().take(60_000).collect::<String>(),
            "receivedBytes":bytes.len(),"redirectsFollowed":hop,
        }));
    }
    Err("HTTP fetch exceeded the redirect limit".into())
}
async fn web_search(query: &str) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .timeout(Duration::from_secs(25))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let html = response.text().await.map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for chunk in html.split("result__a").skip(1).take(10) {
        let Some(href_start) = chunk.find("href=\"") else {
            continue;
        };
        let rest = &chunk[href_start + 6..];
        let Some(href_end) = rest.find('"') else {
            continue;
        };
        let href = decode_ddg(&rest[..href_end]);
        let title = rest[href_end..]
            .find('>')
            .map(|start| {
                let tail = &rest[href_end + start + 1..];
                let end = tail.find("</a>").unwrap_or(tail.len().min(200));
                strip_html(&tail[..end]).trim().to_string()
            })
            .unwrap_or_default();
        if !href.is_empty() && !title.is_empty() {
            results.push(json!({ "title": title, "url": href }));
        }
    }
    Ok(json!({ "query": query, "results": results }))
}

fn decode_ddg(raw: &str) -> String {
    let href = raw
        .strip_prefix("//")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or_else(|| raw.to_string());
    if let Some(index) = href.find("uddg=") {
        let value = &href[index + 5..];
        let value = value.split('&').next().unwrap_or(value);
        return percent_decode(value);
    }
    href
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("dominion-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(resolve_workspace(&dir, "sub").is_ok());
        assert!(resolve_workspace(&dir, "../etc/passwd").is_err());
        assert!(resolve_workspace(&dir, "/etc/passwd").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", dir.join("outside")).unwrap();
            assert!(resolve_workspace(&dir, "outside/nonexistent.txt").is_err());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_frozen_version_write_stops_when_the_file_changed() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-frozen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let state = crate::state::AppState::new(
            config.clone(),
            store,
            Vault::open(&config.key_path()),
        );
        let path = dir.join("frozen.txt");
        std::fs::write(&path, "one").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"one"));
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let written = checked_write(&ctx, &path, "two", "run", false, Some(&expected))
            .await
            .unwrap();
        assert_eq!(written["written"], true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");

        // The same frozen version must not overwrite a later change.
        std::fs::write(&path, "external change").unwrap();
        let stale = checked_write(&ctx, &path, "three", "run", false, Some(&expected))
            .await
            .unwrap();
        assert_eq!(stale["staleVersion"], true, "{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external change");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn workspace_replace_edits_one_exact_block_without_full_content() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let path = dir.join("large.rs");
        std::fs::write(&path, "let old_value = 1;\nlet untouched = 2;\n").unwrap();
        let expected = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let result = replace_text(
            &ctx,
            &path,
            "let old_value = 1;",
            "let new_value = 3;",
            Some(&expected),
            "run",
        )
        .await
        .unwrap();
        assert_eq!(result["replaced"], true, "{result}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 2;\n"
        );
        let ambiguous = replace_text(&ctx, &path, "let", "const", None, "run")
            .await
            .unwrap_err();
        assert!(ambiguous.contains("출현 2회"), "{ambiguous}");
        let unchanged = replace_text(&ctx, &path, "new_value", "new_value", None, "run")
            .await
            .unwrap_err();
        assert!(unchanged.contains("동일"), "{unchanged}");
        assert!(result["bytes"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn replace_range_uses_frozen_file_sha_and_does_not_forge_edits() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-range-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let path=dir.join("src/example.rs");
        std::fs::write(&path,"first();\nlet count = 0;\nlast();\n").unwrap();
        let sha=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"range",agent_id:None,tools_enabled:true,
            tools_execution_enabled:true};
        let args=json!({"path":"src/example.rs","startLine":2,"endLine":2,
            "newText":"let count = 1;","expectedSha256":sha});
        let changed=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap();
        assert_eq!(changed["written"],true);
        assert_eq!(changed["replacedLines"],1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet count = 1;\nlast();\n");
        let stale=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap_err();
        assert!(stale.contains("소스가 읽은 뒤 변경"),"{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet count = 1;\nlast();\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn isolated_rsi_syntax_failure_never_mutates_live_candidate() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir = std::env::temp_dir()
            .join(format!("sub1-syntax-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path = dir.join("src/engine/decompose.rs");
        let original = "fn marker_at() -> bool { true }\n\nfn split_markers() -> Vec<String> { vec![] }\n";
        std::fs::write(&path, original).unwrap();
        let cfg = Config { port: 0, data_dir: dir.clone(), web_dir: dir.join("web"),
            workspace: dir.clone() };
        let state = crate::state::AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(), Vault::open(&cfg.key_path()));
        state.store.put("session", &json!({"id":"rsi-syntax", "rsiRun":true,
            "rsiExplicitFocus":true, "rsiObjective":"Improve split_markers() without changing marker_at()"}));
        let ctx = ToolContext { workspace:&dir, store:&state.store, state:&state,
            session_id:"rsi-syntax", agent_id:None, tools_enabled:true,
            tools_execution_enabled:true };
        let sha = format!("{:x}", Sha256::digest(original.as_bytes()));
        let invalid = "fn marker_at() -> bool { true }\n\nfn split_markers() -> Vec<String> { vec![]\n]\n";
        let failure = replace_source_lines(&ctx, &path, 1, 3, invalid, &sha, "run")
            .await.unwrap();
        assert_eq!(failure["preflightSyntaxRejected"],true,"{failure}");
        assert_eq!(failure["syntaxValid"],false,"{failure}");
        assert_eq!(failure["sourceUnchanged"],true,"{failure}");
        assert_eq!(failure["written"],false,"{failure}");
        assert!(failure["error"].as_str().unwrap().contains("rsi_rust_syntax_rejected"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let sibling_drift = "fn marker_at() -> bool { false }\n\nfn split_markers() -> Vec<String> { vec![] }\n";
        let scope_failure = replace_source_lines(&ctx, &path, 1, 3, sibling_drift, &sha, "run")
            .await.unwrap();
        assert_eq!(scope_failure["preflightScopeRejected"],true,"{scope_failure}");
        assert_eq!(scope_failure["written"],false,"{scope_failure}");
        assert_eq!(scope_failure["sourceUnchanged"],true,"{scope_failure}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let valid = "fn marker_at() -> bool { true }\n\nfn split_markers() -> Vec<String> { vec![\"ok\".to_owned()] }\n";
        let result = replace_source_lines(&ctx, &path, 1, 3, valid, &sha, "run")
            .await.unwrap();
        assert_eq!(result["written"], true, "{result}");
        assert_eq!(result["syntaxValid"], true, "{result}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), valid);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn isolated_rsi_range_read_returns_model_usable_relative_path() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-rsi-path-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path=dir.join("src/engine/target.rs");
        std::fs::write(&path,"fn target() {\n    good();\n}\n").unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"rsi","rsiRun":true}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"rsi",agent_id:None,tools_enabled:true,
            tools_execution_enabled:true};
        let read=execute(&ctx,"workspace_read_range",
            &json!({"path":"src/engine/target.rs","startLine":1,"endLine":3}),
            "run").await.unwrap();
        assert_eq!(read["path"],"src/engine/target.rs");
        assert!(!read["path"].as_str().unwrap().starts_with('/'));
        assert_eq!(read["content"], "fn target() {\n    good();\n}\n",
            "Line numbers must never leak into editable source content");
        assert_eq!(read["contentFormat"], "raw_source_no_line_prefix");
        assert_eq!(read["sourceSha256"],
            format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap())));
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rsi_can_read_only_a_range_or_replace_a_block() {
        assert!(rsi_allowed("code_search"));
        assert!(!rsi_allowed("workspace_list"));
        assert!(rsi_allowed("workspace_read_range"));
        assert!(rsi_allowed("workspace_replace"));
        assert!(rsi_allowed("workspace_replace_range"));
        assert!(rsi_allowed("workspace_write"));
        assert!(rsi_allowed("algorithm_propose"));
        assert!(rsi_allowed("unsafe_audit"));
        assert!(!rsi_allowed("workspace_read"));
        assert!(!rsi_allowed("shell_exec"));
        assert!(crate::engine::rsi::hidden_rsi_oracle_path(
            "src/engine/rsi_oracle_fixture.rs"));
    }

    #[tokio::test]
    async fn algorithm_propose_is_limited_to_running_rsi_campaigns() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-alg-tool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let proposal = json!({"name":"objective-first","expression":"objectiveMatch * 10 + novelty"});
        assert!(execute(&ctx, "algorithm_propose", &proposal, "run").await.is_err());
        state.store.put("session", &json!({"id":"s"})).unwrap();
        assert!(execute(&ctx, "algorithm_propose", &proposal, "run").await.is_err());
        state
            .store
            .put("session", &json!({"id":"s","rsiRun":true,"rsiCampaignId":"c"}))
            .unwrap();
        assert!(execute(&ctx, "algorithm_propose", &proposal, "run").await.is_err());
        state
            .store
            .put("job", &json!({"id":"c","kind":"rsi_evolve","status":"running"}))
            .unwrap();
        let saved = execute(&ctx, "algorithm_propose", &proposal, "run")
            .await
            .unwrap();
        assert_eq!(saved["proposed"], true);
        assert!(state
            .store
            .get("algorithm", saved["algorithmId"].as_str().unwrap())
            .is_some());
        let invalid = json!({"expression":"unknownVariable * 2"});
        assert!(execute(&ctx, "algorithm_propose", &invalid, "run").await.is_err());
        let both = execute(&ctx, "algorithm_propose", &json!({
            "name":"domain-and-target-v1",
            "expression":"objectiveMatch * 6 + exploration",
            "targetExpression":"pathEvidence * 2 + symbolEvidence - repeatPenalty"
        }), "run").await.unwrap();
        assert_eq!(both["proposed"],true);
        assert_eq!(both["algorithm"]["parameters"]["targetExpression"],
            "pathEvidence * 2 + symbolEvidence - repeatPenalty");
        assert_eq!(both["algorithm"]["status"],"available");
        assert!(execute(&ctx,"algorithm_propose",&json!({
            "expression":"novelty","targetExpression":"untrusted_system_call"
        }),"run").await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn unsafe_audit_is_limited_to_rsi_sessions_and_reports_findings() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-unsafe-tool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "// SAFETY: private process group\nunsafe { kill(-id, 9); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/other.rs"),
            "unsafe fn undocumented() {}\n",
        )
        .unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(execute(&ctx, "unsafe_audit", &json!({}), "run").await.is_err());
        state
            .store
            .put("session", &json!({"id":"s","rsiRun":true}))
            .unwrap();
        let report = execute(&ctx, "unsafe_audit", &json!({}), "run").await.unwrap();
        assert_eq!(report["scanned"], 2);
        assert_eq!(report["passed"], false);
        let findings = report["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].as_str().unwrap().ends_with("src/other.rs:1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn code_search_finds_indexed_symbols_and_paths() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-code-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        state
            .store
            .put(
                "code_index",
                &json!({"id":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),"sourceDir":dir.canonicalize().unwrap(),"generatedAt":"2026-11-09T00:00:00Z","files":1}),
            )
            .unwrap();
        state
            .store
            .put(
                "code_index_file",
                &json!({
                    "id":"idx-test:src/engine/rsi_evolve.rs",
                    "indexId":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),
                    "path":"src/engine/rsi_evolve.rs",
                    "symbols":[
                        {"name":"run_loop","kind":"fn","line":120},
                        {"name":"record_rsi_lesson","kind":"fn","line":900}
                    ]
                }),
            )
            .unwrap();
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let result = search_code_index(&ctx, "run_loop", 10).unwrap();
        assert_eq!(result["matchCount"], 1, "{result}");
        assert_eq!(result["matches"][0]["path"], "src/engine/rsi_evolve.rs");
        assert_eq!(result["matches"][0]["line"], 120);
        state.store.put("code_index",&json!({"id":"foreign","sourceDir":"/another/repository","generatedAt":"2099-01-01","files":1}));
        state.store.put("code_index_file",&json!({"id":"foreign-file","indexId":"foreign","path":"private.rs","symbols":[{"name":"run_loop","kind":"fn","line":999}]}));
        let multi=search_code_index(&ctx,"fn run_loop record_rsi_lesson",10).unwrap();
        assert_eq!(multi["matchCount"],2,"{multi}");
        assert!(multi["matches"].as_array().unwrap().iter().all(|row|row["path"]!="private.rs"));
        let path_result = search_code_index(&ctx, "rsi_evolve.rs", 10).unwrap();
        assert!(path_result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "file"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delegation_depth_is_bounded_and_cycle_safe() {
        use crate::config::Config;
        use crate::db::Store;
        let dir = std::env::temp_dir().join(format!("dominion-depth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        for (id, parent) in [
            ("root", None),
            ("d1", Some("root")),
            ("d2", Some("d1")),
            ("d3", Some("d2")),
        ] {
            store.put(
                "session",
                &json!({"id":id,"parentSessionId":parent,"delegated":parent.is_some()}),
            );
        }
        assert_eq!(delegation_depth(&store, "root"), 0);
        assert_eq!(delegation_depth(&store, "d1"), 1);
        assert_eq!(delegation_depth(&store, "d3"), 3);
        store.put(
            "session",
            &json!({"id":"loop","parentSessionId":"d3","delegated":true}),
        );
        store.put(
            "session",
            &json!({"id":"d3","parentSessionId":"loop","delegated":true}),
        );
        assert!(delegation_depth(&store, "loop") <= 4);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn task_results_are_scoped_to_the_owning_or_parent_session() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-task-acl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put(
            "job",
            &json!({
                "id":"j1","kind":"prompt","status":"completed",
                "input":{"sessionId":"child","parentSessionId":"parent"},
                "result":{"answer":"ok"}
            }),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "parent",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&ctx, &json!({"jobId":"j1"}), true).is_ok());
        let child = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "child",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&child, &json!({"jobId":"j1"}), true).is_ok());
        let stranger = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "stranger",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&stranger, &json!({"jobId":"j1"}), true).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn http_fetch_rejects_internal_targets_and_credential_urls() {
        for url in [
            "http://127.0.0.1:4317/api/rsi/campaigns",
            "http://10.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://192.168.1.5/",
            "http://172.20.1.1/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://localhost/",
            "http://worker.internal/",
            "http://user:pass@example.com/",
            "file:///etc/passwd",
        ] {
            assert!(validated_http_target(url).is_err(), "{url}");
        }
        for url in ["https://example.com/resource", "http://1.1.1.1/"] {
            assert!(validated_http_target(url).is_ok(), "{url}");
        }
        let origin=validated_http_target("https://example.com/root").unwrap().0;
        let public=origin.join("/next").unwrap();
        assert!(validated_http_target(public.as_str()).is_ok());
        let private=origin.join("http://127.0.0.1:4317/").unwrap();
        assert!(validated_http_target(private.as_str()).is_err(),
            "A relative or absolute redirect is subject to the same URL gate");
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_fetch_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["127.0.0.1", "192.0.2.1", "198.51.100.7",
            "203.0.113.2", "fc00::1", "fe80::1", "2001:db8::1"] {
            assert!(!public_fetch_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn tool_definitions_cover_the_engine() {
        let names = definitions()
            .into_iter()
            .filter_map(|definition| definition["function"]["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        for expected in [
            "workspace_read",
            "workspace_write",
            "shell_exec",
            "memory_store",
            "memory_search",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} missing"
            );
        }
        assert!(requires_approval("shell_exec"));
        assert!(!requires_approval("workspace_read"));
    }
    #[cfg(unix)]
    async fn wait_for_marker(path: &Path) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "test command did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(unix)]
    fn delayed_child_command(dir: &Path) -> tokio::process::Command {
        // The parent waits while an ordinary child will mutate the temp
        // workspace later. No daemon, user process or external service is used.
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args([
                "-c",
                "(sleep 0.6; printf leaked > escaped.txt) & printf ready > ready.txt; wait",
            ])
            .current_dir(dir);
        command
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.clone();
        let task = tokio::spawn(async move {
            let mut command = delayed_child_command(&cwd);
            run_command(&mut command, Duration::from_secs(10)).await
        });
        wait_for_marker(&dir.join("ready.txt")).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after Stop"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut command = delayed_child_command(&dir);
        let result = run_command(&mut command, Duration::from_millis(150)).await;
        assert!(result.unwrap_err().contains("timed out"));
        assert!(dir.join("ready.txt").exists());
        tokio::time::sleep(Duration::from_millis(650)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after timeout"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn tool_command_preserves_output_exit_status_and_bounded_capture() {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "printf normal; printf warning >&2; exit 7"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        let payload = output_payload(&output);
        assert_eq!(payload["exitCode"], 7);
        assert_eq!(payload["stdout"], "normal");
        assert_eq!(payload["stderr"], "warning");
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "head -c 250000 /dev/zero"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.len() <= 4 * 20_001);
        assert!(output_payload(&output)["stdout"]
            .as_str()
            .unwrap()
            .contains("truncated"));
    }
}
