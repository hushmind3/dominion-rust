use crate::db::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[path = "tools/batch.rs"]
mod batch;
/// Model schemas and native/MCP dispatch share this capability boundary.
pub(crate) fn delegation_allows(store:&Store,session_id:&str,name:&str)->bool{
    batch::allows(store,session_id,name)
}

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
        tool("workspace_replace", "Edit a small source range without rewriting the whole file (approval required). Either supply a unique oldText, or the startLine/endLine and expectedSha256 from workspace_read_range. For line mode, newText replaces those inclusive lines.", object(json!({"path": text("Relative file path"), "oldText": text("Option A: exact existing source text, occurring once"), "startLine":json!({"type":"integer","minimum":1,"description":"Option B: first line from workspace_read_range"}), "endLine":json!({"type":"integer","minimum":1,"description":"Option B: inclusive last line; max 120 lines"}), "newText": text("Replacement code text (do not include numbered line prefixes)"), "expectedSha256": text("Required in line mode: sourceSha256 from workspace_read_range; optional in oldText mode")}), vec!["path","newText"])),
        tool("workspace_replace_function", "Replace one existing, uniquely named top-level Rust function from a version-checked source hash. Send a complete fn declaration and body; all unrelated source remains byte-for-byte unchanged.", object(json!({
            "path": text("Relative Rust .rs source path"),
            "functionName": text("Unique existing top-level Rust function name"),
            "newText": text("Complete replacement Rust function with body"),
            "expectedSha256": text("Full-file SHA-256 returned by workspace_read_range")
        }), vec!["path","functionName","newText","expectedSha256"])),
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
        tool("rsi_system_plan", "Design executable improvements across one or several intelligence subsystems. For a coupled design set component=system, specify components and componentFiles assigning EACH target file to exactly one role, then really edit EVERY target. successorStages is a program handed to the REBUILT descendant for its next design decisions; a plan alone never earns evidence.", object(json!({
            "component":text("Existing subsystem (algorithms, harness, memory, learning, tools, models, improvement_methods, frontend), new intelligence role, or 'system' for coupled redesign (Rust and Python)"),
            "components":json!({"type":"array","items":{"type":"string"},"minItems":2,"maxItems":16,
                "description":"For component=system, all redesign roles including any new modules"}),
            "componentFiles":json!({"type":"object","description":"For component=system map each role to its own nonempty list of files from targetFiles; files cannot be credited twice",
                "additionalProperties":{"type":"array","items":{"type":"string"},"minItems":1}}),
            "moduleIntroductions":json!({"type":"array","maxItems":8,
                "description":"Invent a NEW intelligence role with new implementation source AND an existing integrating consumer source; both files must change and pass independent evaluation.",
                "items":{"type":"object","properties":{
                    "role":{"type":"string"},"newFiles":{"type":"array",
                        "minItems":1,"maxItems":8,"items":{"type":"string"}},
                    "integrationFiles":{"type":"array",
                        "minItems":1,"maxItems":8,"items":{"type":"string"}},
                    "interfaceContract":{"type":"string"},
                    "integrationSymbol":{"type":"string",
                        "description":"Exact identifier appearing in BOTH new implementation source and existing consumer source after edit"},
                    "improvementMechanism":{"type":"string"}
                },"required":["role","newFiles","integrationFiles",
                    "interfaceContract","integrationSymbol","improvementMechanism"],
                "additionalProperties":false}}),
            "interactionEdges":json!({"type":"array","maxItems":16,
                "description":"Source-bound A->B improvement mechanism: both assigned sources must actually change; acceptance and descendant transfer are independently evaluated",
                "items":{"type":"object","properties":{
                    "fromRole":{"type":"string"},"toRole":{"type":"string"},
                    "fromPath":{"type":"string"},"toPath":{"type":"string"},
                    "mechanism":{"type":"string"},"expectedEffect":{"type":"string"}
                },"required":["fromRole","toRole","fromPath","toPath","mechanism","expectedEffect"],
                "additionalProperties":false}}),
            "nextDomain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "hypothesis":text("Falsifiable causal limitation; at least 20 characters"),
            "design":text("Executable architecture, interface contracts, memory/learning/tool changes and failure handling; at least 30 characters"),
            "targetFiles":json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":24}),
            "improvementPrinciple": json!({"type":"object",
                "description":"Model-authored source-bound learning principle: change both one task-intelligence component AND the editable method that discovers its improvements, with independent next-generation predictions",
                "properties": {
                    "name":{"type":"string"},
                    "taskRole":{"type":"string","description":"Any actual source-backed intelligence role discovered or invented by this descendant; not the improvement_methods role"},
                    "taskFile":{"type":"string"},
                    "methodFile":{"type":"string"},
                    "failureMode":{"type":"string"},
                    "mechanism":{"type":"string"},
                    "counterfactual":{"type":"string"},
                    "transferPrediction":{"type":"string"},
                    "learningSignal":{"type":"string"}
                },
                "required":["name","taskRole","taskFile","methodFile",
                    "failureMode","mechanism","counterfactual",
                    "transferPrediction","learningSignal"],
                "additionalProperties":false}),
            "expectedOutcomes":json!({"type":"object",
                "description":"Probability estimates before code modification; independently calibrated after evaluation.",
                "properties":{
                    "candidateCaptured":{"type":"number","minimum":0,"maximum":1},
                    "adoption":{"type":"number","minimum":0,"maximum":1},
                    "metaImprovement":{"type":"number","minimum":0,"maximum":1},
                    "regression":{"type":"number","minimum":0,"maximum":1}
                },"additionalProperties":false}),
            "openProgram":json!({"type":"object",
                "description":"Model-authored open recursive experiment graph. Unlike fixed successorStages, nodes, roles, mechanics, branching, loops and replacement plans are authored by the model; an adopted source change is required before the compiled descendant interprets it.",
                "properties":{
                    "entry":{"type":"string"},
                    "nodes":{"type":"array","minItems":1,"maxItems":96,
                        "items":{"type":"object","properties":{
                            "id":{"type":"string"},"role":{"type":"string"},
                            "domain":{"type":"string","enum":crate::engine::rsi_evolve::DOMAINS},
                            "path":{"type":"string"},"objective":{"type":"string"},
                            "mechanism":{"type":"string"}},
                            "required":["id","role","domain","path","objective","mechanism"],
                            "additionalProperties":false}},
                    "edges":{"type":"array","maxItems":384,
                        "items":{"type":"object","properties":{
                            "from":{"type":"string"},"to":{"type":"string"},
                            "when":{"description":"String outcome or recursive aggregate-metric condition: {all:[...]}, {any:[...]}, {not:...}, or {metric:'improvedHoldouts',op:'>=',value:2}. Model may combine predicates without prewritten experiment stages.",
                                "anyOf":[{"type":"string","enum":["always","adopted",
                                    "not_adopted","improved","regressed",
                                    "rejected","no_candidate","neutral"]},
                                    {"type":"object"}]}},
                            "required":["from","to","when"],
                            "additionalProperties":false}}
                },"required":["entry","nodes","edges"],"additionalProperties":false}),
            "successorStages":json!({"type":"array","maxItems":8,
                "description":"Executable planning intentions for the NEXT compiled descendant. Each stage is rebound to fresh indexed source; no stage receives improvement credit before actual editing.",
                "items":{"type":"object","properties":{
                    "role":{"type":"string"},"domain":{"type":"string","enum":crate::engine::rsi_evolve::DOMAINS},
                    "path":{"type":"string"},"objective":{"type":"string"},
                    "learningPolicy":{"type":"string","enum":["avoid_failed","retry_failed"],
                        "description":"Rank successor stages by fewer failures, or revisit difficult observed failures"},
                    "toolPolicy":{"type":"string","enum":["patch_only","allow_new_modules"],
                        "description":"Restrict inherited descendant tool gateway to existing source patches, or permit new modules"},
                    "executionPolicy":{"type":"string","enum":["grounded_edit","explore_then_edit"],
                        "description":"Bind the inherited source to the native edit checkpoint, or retain exploration"}
                    },
                    "required":["role","domain","path","objective"],"additionalProperties":false}})
        }), vec!["component","nextDomain","hypothesis","design","targetFiles"])),
        tool("rsi_method_propose", "Save a reusable executable procedure learned during RSI research. Existing procedure nodes run through the normal tool runtime in later scheduled work; registration is not improvement evidence.", object(json!({
            "name":text("Reusable method name"),
            "description":text("Principle, source and limits of the method"),
            "domain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "verification":json!({"type":"object","description":"Existing procedure format: type=procedure, appliesTo.requestContains, nodes [{id,tool,arguments}], edges [{from,to}], after [{kind:file_equals/file_contains/file_exists/tool_called,path,text,tool}]. Arguments can bind a prior node result via $node.field. Include concrete postconditions."})
        }), vec!["name","domain","verification"])),
        tool("delegate_tasks", "Launch up to 4 independent read-only research agents in parallel; only the parent may edit files.", object(json!({
            "tasks":json!({"type":"array","minItems":1,"maxItems":4,"items":{"type":"object",
                "properties":{"agentId":text("Research agent id"),"prompt":text("Independent read-only investigation")},
                "required":["agentId","prompt"],"additionalProperties":false}}),
            "wait":json!({"type":"boolean","description":"Wait for all replies (default false)"}),
            "timeoutSeconds":json!({"type":"integer","minimum":5,"maximum":300})
        }),vec!["tasks"])),
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
            | "workspace_replace_function"
            | "file_write"
            | "shell_exec"
            | "terminal_exec"
            | "github_api"
            | "browser_control"
    )
}

pub fn rsi_allowed(name:&str)->bool{
    matches!(name,"code_search"|"workspace_read_range"|"workspace_replace"|"workspace_replace_function"|"workspace_write"|"web_search"|"http_fetch"|"algorithm_propose"|"unsafe_audit"|"rsi_method_propose"|"rsi_system_plan")
}

#[path = "tools/gateway.rs"]
mod gateway;
pub use gateway::execute;

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
    let inherited_goal=crate::engine::goal_workspace::goal_for_session(ctx.state,ctx.session_id)?;
    let readonly=args["readOnly"]==true;
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
                "goalId": inherited_goal,
                "delegatedReadOnly": readonly,
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
            "delegatedReadOnly": readonly,
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

#[path = "tools/editor.rs"]
mod editor;
use editor::*;

// Independent native tool implementation boundaries. The parent remains the
// compatibility façade used by SDK, the policy gateway and existing clients.
#[path = "tools/process.rs"]
mod process;
pub(crate) use process::run_command;
use process::{shell,executable,output_payload};
#[path = "tools/network.rs"]
mod network;
use network::{http_fetch,web_search,public_http_ip,validate_http_target};
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_fetch_rejects_private_addresses_and_local_hosts() {
        for url in [
            "http://127.0.0.1:4317/", "http://10.1.2.3/", "http://172.16.0.1/",
            "http://192.168.1.8/", "http://169.254.169.254/latest/meta-data/",
            "http://100.100.100.100/", "http://[::1]/", "http://[fc00::1]/",
            "http://[64:ff9b::a9fe:a9fe]/",
            "http://localhost/", "http://service.internal/", "http://printer.local/"
        ] {
            let parsed=reqwest::Url::parse(url).unwrap();
            assert!(validate_http_target(&parsed).is_err(),"{url}");
        }
        assert!(validate_http_target(&reqwest::Url::parse("https://1.1.1.1/").unwrap()).is_ok());
        assert!(validate_http_target(&reqwest::Url::parse("https://example.com/").unwrap()).is_ok());
        assert!(validate_http_target(&reqwest::Url::parse("ftp://example.com/").unwrap()).is_err());
        assert!(validate_http_target(&reqwest::Url::parse("https://user:pass@example.com/").unwrap()).is_err());
    }

    #[tokio::test]
    async fn http_fetch_rejects_loopback_before_connecting() {
        assert!(http_fetch("http://127.0.0.1:4317/admin").await.unwrap_err().contains("non-public"));
        assert!(http_fetch("http://[::1]/").await.is_err());
    }

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
        // Repository indexing intentionally covers source paths, not ad-hoc
        // root files. Use a genuine indexed Rust source for the hash-recovery
        // stage of this otherwise standalone tool test.
        let path = dir.join("src/engine/large.rs");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
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
            .unwrap();
        assert_eq!(ambiguous["occurrences"], 2, "{ambiguous}");
        assert!(result["bytes"].as_u64().unwrap() > 0);
        // A smaller model can update a numbered span without echoing the
        // entire existing file. Every range write is version-pinned.
        let frozen=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let changed=replace_line_span(&ctx,&path,
            &json!({"startLine":2,"endLine":2,"expectedSha256":frozen}),
            "let untouched = 7;","run").await.unwrap();
        assert_eq!(changed["written"],true,"{changed}");
        assert_eq!(changed["editMode"],"versioned_line_span");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 7;\n");
        let stale=replace_line_span(&ctx,&path,
            &json!({"startLine":2,"endLine":2,"expectedSha256":frozen}),
            "let untouched = 8;","run").await.unwrap();
        assert_eq!(stale["staleVersion"],true,"{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 7;\n");
        // In a real RSI generation the coordinator already indexed the exact
        // file revision. A small model may omit SHA and serialize line numbers
        // as strings; that is repairable only while the indexed bytes match.
        let index=crate::engine::code_index::refresh(&state,&dir).unwrap();
        state.store.put("session",&json!({
            "id":"s","rsiRun":true,"rsiCodeIndexId":index["id"]
        }));
        let grounded=replace_line_span(&ctx,&path,
            &json!({"startLine":"2","endLine":"2"}),
            "let untouched = 9;","run").await.unwrap();
        assert_eq!(grounded["written"],true,"{grounded}");
        assert_eq!(grounded["sourceHashRecoveredFromIndex"],true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 9;\n");
        // An actual Qwen local-model campaign failed twice after swapping
        // absolute start/end coordinates. Reordering a bounded, version-pinned
        // span is safe; guessing a relative line count is not.
        let again=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let corrected=replace_line_span(&ctx,&path,
            &json!({"startLine":2,"endLine":1,"expectedSha256":again}),
            "let new_value = 6;\nlet untouched = 9;","run").await.unwrap();
        assert_eq!(corrected["written"],true,"{corrected}");
        assert_eq!(corrected["reorderedLineEndpoints"],true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 6;\nlet untouched = 9;\n");
        let after=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let rejected=replace_line_span(&ctx,&path,
            &json!({"startLine":1,"endLine":0,"expectedSha256":after}),
            "bad","run").await;
        assert!(rejected.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 6;\nlet untouched = 9;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn real_model_edit_without_end_line_recovers_only_unambiguous_intent() {
        let function="pub fn evaluate_policy(value: u64) -> u64 {\n    value + 1\n}\n";
        assert_eq!(
            rsi_unambiguous_edit_intent(&json!({"endLine":0}),function),
            Some(RsiEditIntent::CompleteFunction("evaluate_policy".into()))
        );
        assert_eq!(
            rsi_unambiguous_edit_intent(&json!({"startLine":"7"}),"let x=3;"),
            Some(RsiEditIntent::SingleLine(7))
        );
        // A multi-line snippet without an explicit final coordinate has no
        // unique range. The model must provide one, not let the harness guess.
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":7,"endLine":0}),
            "let x=3;\nlet y=4;\n"),None);
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":7,"endLine":0}),
            "pub fn a() {\n}\npub fn b() {\n}\n"),None);
        // A valid explicit span is never rewritten into the fallback path.
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":7,"endLine":8}),function),None);
    }

    #[tokio::test]
    async fn descendant_function_edit_without_end_line_writes_real_sha_pinned_rust() {
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "rsi-compiled-descendant-tool-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        let path=root.join("src/engine/decision.rs");
        std::fs::write(&path,
            "pub fn choose(value:u32)->u32 {\n    value + 1\n}\n\npub fn keep()->bool {\n    true\n}\n").unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        state.store.put("session",&json!({"id":"descendant","rsiRun":true,
            "rsiCodeIndexId":index["id"]})).unwrap();
        let ctx=ToolContext{
            workspace:&root,store:&state.store,state:&state,
            session_id:"descendant",agent_id:None,
            tools_enabled:true,tools_execution_enabled:true
        };
        let arguments=json!({
            "path":"src/engine/decision.rs",
            "startLine":0,"endLine":0,
            "newText":"pub fn choose(value:u32)->u32 {\n    value + 2\n}\n"
        });
        let applied=execute(&ctx,"workspace_replace",&arguments,"run-1")
            .await.unwrap();
        assert_eq!(applied["written"],true,"{applied}");
        assert_eq!(applied["editContractRecovered"],
            "model_authored_complete_function");
        assert_eq!(applied["sourceHashRecoveredFromIndex"],true);
        let changed=std::fs::read_to_string(&path).unwrap();
        assert!(changed.contains("value + 2"));
        assert!(changed.contains("pub fn keep()->bool"));
        // The same index cannot approve a second write against new bytes.
        let replay=execute(&ctx,"workspace_replace",&arguments,"run-2")
            .await.unwrap();
        assert_eq!(replay["staleVersion"],true,"{replay}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),changed);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn rsi_can_read_only_a_range_or_replace_a_block() {
        assert!(rsi_allowed("code_search"));
        assert!(!rsi_allowed("workspace_list"));
        assert!(rsi_allowed("workspace_read_range"));
        assert!(rsi_allowed("workspace_replace"));
        assert!(rsi_allowed("workspace_write"));
        assert!(rsi_allowed("algorithm_propose"));
        assert!(rsi_allowed("unsafe_audit"));
        assert!(!rsi_allowed("workspace_read"));
        assert!(!rsi_allowed("shell_exec"));
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
