//! Context and policy grounded tool selection.
//! Active Rust subtree of chat::runtime.
use super::*;

pub(crate) fn restore_program_state(
    state: &AppState,
    task_id: &str,
    execution_id: &str,
    program: &Value,
) -> Result<(), String> {
    let source = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| {
            execution["id"].as_str() != Some(execution_id)
                && execution["taskId"].as_str() == Some(task_id)
                && execution["status"].as_str() == Some("interrupted")
                && execution["programResumeApproved"].as_bool() == Some(true)
                && (execution["programCheckpoint"]["programRevision"].as_str()
                    == program["revision"].as_str()
                    || execution["revisedProgram"]["revision"].as_str()
                        == execution["programCheckpoint"]["programRevision"].as_str())
        })
        .max_by_key(|execution| {
            execution["startedAt"].as_str().unwrap_or("").to_string()
        });
    let Some(source) = source else {
        return Ok(());
    };
    let checkpoint = &source["programCheckpoint"];
    if checkpoint["pendingOperationIds"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        return Err("중단된 program 연산의 결과를 확인해야 합니다.".into());
    }
    let source_id = source["id"].as_str().unwrap_or("");
    if let Some(mut target) = state.store.get("execution", execution_id) {
        target["programCheckpoint"] = checkpoint.clone();
        if let Some(adaptations) = source.get("adaptations") {
            target["adaptations"] = adaptations.clone();
        }
        if let Some(adaptation_checkpoint) = source.get("adaptationCheckpoint") {
            target["adaptationCheckpoint"] = adaptation_checkpoint.clone();
        }
        if let Some(last_adaptation) = source.get("lastAdaptation") {
            target["lastAdaptation"] = last_adaptation.clone();
        }
        if let Some(revised) = source.get("revisedProgram") {
            target["revisedProgram"] = revised.clone();
        }
        target["resumedFromExecutionId"] = json!(source_id);
        state.store.put("execution", &target);
    }
    let mut old = source.clone();
    old["programResumeApproved"] = json!(false);
    old["resumedByExecutionId"] = json!(execution_id);
    state.store.put("execution", &old);
    state.store.audit(
        "program.resumed",
        json!({"from": source_id, "to": execution_id, "revision": program["revision"]}),
    );
    Ok(())
}

pub(crate) fn tool_is_read_only(name: &str) -> bool {
    matches!(
        name,
        "workspace_list" | "workspace_read" | "workspace_read_range" | "file_list" | "file_read"
            | "web_search" | "http_fetch" | "memory_search"
            | "task_status" | "task_result" | "account_status"
    )
}

pub(crate) fn mark_tool_argument_signal(
    state: &AppState, request: &TurnRequest, execution_id: &str, ok: bool,
) {
    let _ = crate::engine::algorithms::choose_for_agent(
        state,
        "tool.arguments",
        request.agent_id.as_deref(),
        Some(execution_id),
    );
    crate::engine::algorithms::mark_signal(state, execution_id, "tool.arguments", ok);
}

pub(crate) fn normalize_tool_arguments(mut arguments: Value, strategy: &str) -> Value {
    if strategy != "normalize-paths" {
        return arguments;
    }
    let Some(map) = arguments.as_object_mut() else {
        return arguments;
    };
    for key in ["path", "cwd", "file", "filename"] {
        let Some(raw) = map.get(key).and_then(Value::as_str) else {
            continue;
        };
        let mut normalized = raw.trim().replace('\\', "/");
        while normalized.contains("//") {
            normalized = normalized.replace("//", "/");
        }
        while normalized.starts_with("./") {
            normalized = normalized[2..].to_string();
        }
        if !normalized.is_empty() {
            map.insert(key.to_string(), Value::String(normalized));
        }
    }
    arguments
}

pub(crate) fn tool_name(definition: &Value) -> &str {
    definition["function"]["name"]
        .as_str()
        .or_else(|| definition["name"].as_str())
        .unwrap_or("")
}

/// Casual room turns carry zero tool schemas. Action requests select the
/// exact native schemas the harness judges relevant; no model call is used.
pub(crate) fn room_tool_request(input: &str) -> bool {
    let lower = input.to_lowercase();
    [
        "파일", "폴더", "읽고", "읽어", "읽어줘", "읽어봐", "읽기",
        "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
        "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
        "열어줘", "열어봐", "보여줘", "보여 줘", "실행", "명령", "터미널",
        "빌드", "테스트", "설치", "검색", "웹",
        "http", "url", "api", "코드", "버그", "도구", "계속", "이어", "마저",
        "결과", "가져와", "확인해", "진행해",
        "스킬", "메모리", "브라우저", "깃허브", "github", "shell", "file", "read",
        "write", "run", "search", "fetch", "install", "build", "test", "tool",
        "browser", "memory", "delegate", "task",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

pub(crate) fn recent_room_tools(state: &AppState, session_id: &str) -> std::collections::HashSet<String> {
    state
        .store
        .messages(session_id)
        .into_iter()
        .rev()
        .take(30)
        .flat_map(|message| {
            message["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|call| {
            call["function"]["name"]
                .as_str()
                .map(|name| name.to_lowercase())
        })
        .collect()
}

pub(crate) fn select_room_tools(
    definitions: Vec<Value>,
    query: &str,
    recent: &std::collections::HashSet<String>,
    acceptance: bool,
) -> Vec<Value> {
    if definitions.is_empty() {
        return Vec::new();
    }
    if !acceptance && !room_tool_request(query) {
        return Vec::new();
    }
    let lower = query.to_lowercase();
    let groups: &[(&[&str], &[&str])] = &[
        (
            &[
                "파일", "폴더", "읽고", "읽어", "읽어줘", "읽어봐", "읽기",
                "열어줘", "열어봐", "보여줘", "보여 줘", "read", "file",
            ],
            &["workspace_list", "workspace_read", "workspace_read_range", "file_list", "file_read"],
        ),
        (
            &[
                "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
                "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
                "기록", "남겨", "write", "save", "edit", "delete", "create",
            ],
            &["workspace_read", "workspace_read_range", "workspace_replace", "workspace_write", "file_write"],
        ),
        (
            &["실행", "명령", "터미널", "빌드", "테스트", "설치", "shell", "run", "build", "test", "install"],
            &["shell_exec", "terminal_exec"],
        ),
        (
            &["검색", "웹", "http", "url", "fetch", "search"],
            &["web_search", "http_fetch"],
        ),
        (
            &["메모리", "memory", "기억해둬", "기억해 둬", "저장해"],
            &["memory_store", "memory_search"],
        ),
        (&["스킬", "skill"], &["skill_propose"]),
        (
            &["위임", "delegate", "task", "에이전트"],
            &["delegate_task", "task_status", "task_result"],
        ),
        (
            &["브라우저", "browser", "클릭", "click"],
            &["browser_control"],
        ),
        (&["github", "깃허브"], &["github_api"]),
        (&["계정", "oauth", "account"], &["account_status"]),
    ];
    let mut scored: Vec<(i32, Value)> = definitions
        .iter()
        .filter_map(|definition| {
            let name = tool_name(definition);
            let name_lower = name.to_lowercase();
            let haystack = serde_json::to_string(definition)
                .unwrap_or_default()
                .to_lowercase();
            let mut score = 0i32;
            if lower.contains(&name_lower) {
                score += 30;
            }
            for token in lower
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|token| token.chars().count() > 1)
            {
                if haystack.contains(token) {
                    score += 2;
                }
            }
            for (terms, tools) in groups {
                if terms.iter().any(|term| lower.contains(term))
                    && tools.contains(&name_lower.as_str())
                {
                    score += 12;
                }
            }
            if recent.contains(&name_lower) {
                score += 8;
            }
            (score > 0).then(|| (score, definition.clone()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    if scored.is_empty() {
        if acceptance {
            return definitions;
        }
        return definitions
            .into_iter()
            .filter(|definition| {
                matches!(
                    tool_name(definition),
                    "workspace_list" | "workspace_read" | "workspace_read_range" | "workspace_replace" | "workspace_write" | "shell_exec" | "web_search"
                )
            })
            .collect();
    }
    scored.truncate(8);
    scored.into_iter().map(|(_, definition)| definition).collect()
}

pub(crate) fn select_tool_definitions(
    definitions: Vec<Value>,
    query: &str,
    algorithm: &Value,
) -> Vec<Value> {
    let strategy = algorithm["implementation"]
        .as_str()
        .or_else(|| algorithm["variant"].as_str())
        .unwrap_or("all");
    if strategy == "all" {
        return definitions;
    }
    let default_core = [
        "workspace_list", "workspace_read", "workspace_read_range", "workspace_replace", "workspace_write", "shell_exec",
        "web_search", "memory_store", "memory_search", "delegate_task",
        "delegate_tasks", "task_status", "task_result",
    ];
    let core = algorithm["parameters"]["tools"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_else(|| {
            default_core
                .iter()
                .map(|name| name.to_string())
                .collect::<std::collections::BTreeSet<_>>()
        });
    if strategy == "core" {
        return definitions
            .into_iter()
            .filter(|definition| core.contains(tool_name(definition)))
            .collect();
    }
    // query-relevant: always keep the core set, add the best matches.
    let tokens = query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let extra = algorithm["parameters"]["extra"].as_u64().unwrap_or(8) as usize;
    let mut matched = definitions
        .iter()
        .filter(|definition| !core.contains(tool_name(definition)))
        .map(|definition| {
            let haystack = serde_json::to_string(definition)
                .unwrap_or_default()
                .to_lowercase();
            let score = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            (score, definition.clone())
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| b.0.cmp(&a.0));
    let keep_extra = matched
        .into_iter()
        .take(extra)
        .map(|(_, definition)| definition)
        .collect::<Vec<_>>();
    definitions
        .into_iter()
        .filter(|definition| {
            core.contains(tool_name(definition))
                || keep_extra
                    .iter()
                    .any(|extra| tool_name(extra) == tool_name(definition))
        })
        .collect()
}

pub(crate) fn trim_tool_definitions(definitions: Vec<Value>, budget: usize) -> Vec<Value> {
    let mut kept = Vec::new();
    let mut used = 0usize;
    for definition in definitions {
        let size = serde_json::to_string(&definition)
            .map(|text| text.len())
            .unwrap_or(0);
        if used + size > budget && !kept.is_empty() {
            continue;
        }
        used += size;
        kept.push(definition);
    }
    kept
}

