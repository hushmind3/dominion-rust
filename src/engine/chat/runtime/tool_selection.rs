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

// Shared with autonomous goal verification; a single lexical boundary policy
// prevents incidental English substrings from changing agent execution.
use crate::engine::text_intent::{has_any as intent_has_any, has_term as intent_has_term};

/// Explicit owner directives outrank token/menu ranking heuristics. A match
/// requires the exact authorized tool name with word boundaries, not a fuzzy
/// description match that a tool response or repository file could impersonate.
pub(crate) fn owner_named_tool(query: &str, name: &str) -> bool {
    !name.is_empty() && intent_has_term(query, &name.to_lowercase())
}

/// Keep the *existing* catalog contract. This changes presentation priority,
/// never grants a missing or upstream-rejected capability.
pub(crate) fn prioritize_owner_named_tools(
    definitions: Vec<Value>, owner_text: &str,
) -> Vec<Value> {
    let lower = owner_text.to_lowercase();
    let (mut requested, ordinary): (Vec<_>, Vec<_>) = definitions
        .into_iter()
        .partition(|definition| owner_named_tool(&lower, tool_name(definition)));
    requested.extend(ordinary);
    requested
}

/// Casual room turns carry zero tool schemas. Action requests select the
/// exact native schemas the harness judges relevant; no model call is used.
pub(crate) fn room_tool_request(input: &str) -> bool {
    let lower = input.to_lowercase();
    let terms: &[&str] = &[
        "파일", "폴더", "읽고", "읽어", "읽어줘", "읽어봐", "읽기",
        "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
        "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
        "열어줘", "열어봐", "보여줘", "보여 줘", "실행", "명령", "터미널",
        "빌드", "테스트", "설치", "검색", "웹",
        "http", "url", "api", "코드", "버그", "도구", "계속", "이어", "마저",
        "결과", "가져와", "확인해", "진행해",
        "스킬", "메모리", "브라우저", "깃허브", "github", "shell", "file", "read",
        "write", "run", "search", "fetch", "install", "build", "test", "tool",
        "browser", "memory", "delegate", "task", "continue", "resume",
        "files", "tests", "testing", "searching", "running", "commit", "git",
        "fix", "bug", "code", "source", "edit", "delete", "create", "save",
        "refactor", "patch", "debug", "push", "pull",
    ];
    intent_has_any(&lower, terms)
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
                "열어줘", "열어봐", "보여줘", "보여 줘", "read", "file", "files",
            ],
            &["workspace_list", "workspace_read", "workspace_read_range", "file_list", "file_read"],
        ),
        (
            &[
                "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
                "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
                "기록", "남겨", "write", "save", "edit", "delete", "create", "fix", "patch", "refactor",
            ],
            &["workspace_read", "workspace_read_range", "workspace_replace", "workspace_replace_range",
              "workspace_replace_function", "workspace_write", "file_write"],
        ),
        (
            &["실행", "명령", "터미널", "빌드", "테스트", "설치", "shell", "run", "running", "build", "test", "tests", "testing", "install"],
            &["shell_exec", "terminal_exec"],
        ),
        (
            &["검색", "웹", "http", "url", "fetch", "search", "searching"],
            &["web_search", "http_fetch"],
        ),
        (
            &["메모리", "memory", "기억해둬", "기억해 둬", "저장해"],
            &["memory_store", "memory_search"],
        ),
        (&["코드", "버그", "소스", "code", "source", "bug", "debug", "fix", "refactor", "patch"],
            &["code_search", "workspace_list", "workspace_read", "workspace_read_range"]),
        (&["스킬", "skill"], &["skill_propose"]),
        (
            &["위임", "delegate", "task", "에이전트"],
            &["delegate_task", "delegate_tasks", "task_status", "task_result"],
        ),
        (
            &["브라우저", "browser", "클릭", "click"],
            &["browser_control"],
        ),
        (&["github", "깃허브", "git", "커밋", "commit", "push", "pull"],
            &["github_api", "shell_exec"]),
        (&["계정", "oauth", "account"], &["account_status"]),
    ];
    // Last-used tools are relevant to explicit continuation, not every new
    // topic in a shared room. History alone must not inflate a fresh menu.
    let continuation = intent_has_any(&lower, &[
        "계속", "이어", "마저", "continue", "resume", "again", "next",
    ]);
    let mut scored: Vec<(i32, Value)> = definitions
        .iter()
        .filter_map(|definition| {
            let name = tool_name(definition);
            let name_lower = name.to_lowercase();
            let haystack = serde_json::to_string(definition)
                .unwrap_or_default()
                .to_lowercase();
            let mut score = 0i32;
            if intent_has_term(&lower, &name_lower) {
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
                if intent_has_any(&lower, terms)
                    && tools.contains(&name_lower.as_str())
                {
                    score += 12;
                }
            }
            if continuation && recent.contains(&name_lower) {
                score += 8;
            }
            (score > 0).then(|| (score, definition.clone()))
        })
        .collect();
    // A semantic relevance score must not outvote an explicit owner tool name.
    scored.sort_by(|a, b| {
        owner_named_tool(&lower, tool_name(&b.1))
            .cmp(&owner_named_tool(&lower, tool_name(&a.1)))
            .then_with(|| b.0.cmp(&a.0))
    });
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
    // In a large repository the minimal schema set must still let the model
    // find symbols and apply bounded edits. A cheap "core" policy previously
    // hid code_search and both surgical editors, causing blind whole-file
    // rewrites and failed localization on otherwise solvable coding goals.
    let coding_request = intent_has_any(query, &[
        "코드","소스","버그","함수","리팩터","컴파일","개발",
        "code","source","bug","fix","patch","refactor","implement",
        "function","rust","python","typescript","javascript",
    ]) || [".rs",".py",".ts",".tsx",".js",".jsx",".go"].iter()
        .any(|suffix|query.to_lowercase().contains(suffix));
    let coding_tools=["code_search","workspace_replace_range","workspace_replace_function"];
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
        let lower = query.to_lowercase();
        return definitions
            .into_iter()
            .filter(|definition| core.contains(tool_name(definition))
                || (coding_request && coding_tools.contains(&tool_name(definition)))
                || owner_named_tool(&lower, tool_name(definition)))
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
    let lower = query.to_lowercase();
    definitions
        .into_iter()
        .filter(|definition| {
            core.contains(tool_name(definition))
                || (coding_request && coding_tools.contains(&tool_name(definition)))
                || owner_named_tool(&lower, tool_name(definition))
                || keep_extra
                    .iter()
                    .any(|extra| tool_name(extra) == tool_name(definition))
        })
        .collect()
}

/// Owner-specified autonomous source edits are not generic browsing.
/// Constrain only the MODEL'S tool menu to a few executable workspace actions.
/// The tool executor still checks the same owner policy, file scope, approval
/// and exact SHA. This is a native Rust counterpart to progressive tool
/// selection in modern agent frameworks, not a foreign SDK runner.
pub(crate) fn autonomous_code_task(input:&str)->bool{
    let lower=input.to_lowercase();
    intent_has_any(&lower,&[
        "코드","소스","파일","개발","구현","편집",
        "code","source","file","fix","edit","modify","patch","implement",
        "module","function","translation","locale","config","script",
    ]) || [".rs",".py",".js",".mjs",".ts",".tsx",".json",".md"].iter()
        .any(|extension|lower.contains(extension))
}
pub(crate) fn autonomous_code_tool_window(definitions:Vec<Value>,input:&str)->Vec<Value>{
    if !autonomous_code_task(input) {return definitions}
    let lower=input.to_lowercase();
    let preferred=[
        "workspace_list","workspace_read","workspace_read_range",
        "code_search","workspace_replace","workspace_write",
        "workspace_replace_range",
    ];
    let mut selected=Vec::new();
    for name in preferred {
        if let Some(tool)=definitions.iter().find(|definition|tool_name(definition)==name){
            selected.push(tool.clone());
        }
    }
    // Rust source tasks can additionally replace one existing function while
    // keeping unrelated functions unchanged. Other tasks should not pay for
    // its large function-specific schema on every model round.
    if lower.contains(".rs") || intent_has_any(&lower,&["rust","러스트"]) {
        if let Some(tool)=definitions.iter().find(|definition|
            tool_name(definition)=="workspace_replace_function"){
            selected.push(tool.clone());
        }
    }
    // When a bounded source workflow explicitly asks to execute tests, keep
    // shell_exec ONLY if already advertised by the owner's tool policy.
    if intent_has_any(&lower,&["테스트","빌드","컴파일","test","build","cargo","npm"]) {
        if let Some(tool)=definitions.iter().find(|definition|tool_name(definition)=="shell_exec"){
            selected.push(tool.clone());
        }
    }
    // A compact code menu must never silently remove a capability that the
    // owner explicitly requested. This is selection, not an authorization
    // layer: preserve only definitions already admitted by the upstream gate.
    // In particular, mixed coding + research/browser/shell goals must be able
    // to execute every part of the owner's request.
    let web_requested=intent_has_any(&lower,&[
        "웹 검색","검색해","인터넷","온라인","브라우저","웹사이트",
        "web search","browse","browser","online","http://","https://",
    ]);
    let shell_requested=intent_has_any(&lower,&[
        "명령 실행","명령어","터미널","셸","실행해","실행하",
        "run ","execute ","terminal","shell","bash",
    ]);
    for definition in &definitions {
        let name=tool_name(definition);
        if name.is_empty() {continue;}
        let exact=lower.contains(&name.to_lowercase());
        let web=web_requested && matches!(name,
            "web_search"|"http_fetch"|"browser_control"|"browser_open");
        let shell=shell_requested && matches!(name,"shell_exec"|"terminal_exec");
        if (exact||web||shell)
            && !selected.iter().any(|item|tool_name(item)==name) {
            selected.push(definition.clone());
        }
    }
    if selected.is_empty(){definitions}else{selected}
}

/// Reserve space for explicitly requested, already-authorized tools FIRST.
/// Return an error if even those schemas exceed capacity, instead of quietly
/// pretending the agent can obey a request whose tool never reached the model.
pub(crate) fn trim_tool_definitions(
    definitions: Vec<Value>, budget: usize, owner_text: &str,
) -> Result<Vec<Value>, String> {
    let lower = owner_text.to_lowercase();
    let ordered = prioritize_owner_named_tools(definitions, owner_text);
    let required_bytes = ordered.iter()
        .filter(|definition| owner_named_tool(&lower, tool_name(definition)))
        .map(|definition| serde_json::to_string(definition).unwrap_or_default().len())
        .sum::<usize>();
    if required_bytes > budget {
        return Err(format!(
            "사용자가 지정한 도구의 스키마({required_bytes}바이트)가 현재 모델 예산({budget}바이트)을 초과합니다. 더 큰 컨텍스트 모델을 선택하거나 도구를 나눠 실행해야 합니다."
        ));
    }
    let mut kept = Vec::new();
    let mut used = 0usize;
    for definition in ordered {
        let size = serde_json::to_string(&definition)
            .map(|text| text.len())
            .unwrap_or(0);
        if used.saturating_add(size) > budget && !kept.is_empty() {
            continue;
        }
        used = used.saturating_add(size);
        kept.push(definition);
    }
    Ok(kept)
}


#[cfg(test)]
mod sub1_native_autonomous_tool_tests {
    use super::*;
    #[test]
    fn short_local_code_menu_preserves_read_write_and_approved_shell_only(){
        let def=|name:&str|json!({"type":"function","function":{"name":name}});
        let tools=vec!["account_status","workspace_list","workspace_read",
            "workspace_read_range","code_search","workspace_replace",
            "workspace_write","workspace_replace_range","workspace_replace_function",
            "shell_exec","browser_control","file_write","mcp__danger"]
            .iter().map(|name|def(name)).collect::<Vec<_>>();
        let selected=autonomous_code_tool_window(tools.clone(),
            "Fix trial/code/lib/price.mjs and create FIXED.txt");
        let names=selected.iter().map(tool_name).collect::<Vec<_>>();
        assert_eq!(names,vec!["workspace_list","workspace_read","workspace_read_range",
            "code_search","workspace_replace","workspace_write","workspace_replace_range"]);
        assert_eq!(autonomous_code_tool_window(tools.clone(),"How are you?").len(),tools.len());
        assert!(!autonomous_code_task("계획을 검증하세요. 결과를 조사하세요."),
            "Generic work review must not become a code-writing run");
        assert!(autonomous_code_task("Fix trial/code/lib/price.mjs and create FIXED.txt"));
        let rust=autonomous_code_tool_window(tools.clone(),"Update src/lib.rs then cargo test");
        assert!(rust.iter().any(|v|tool_name(v)=="workspace_replace_function"));
        assert!(rust.iter().any(|v|tool_name(v)=="shell_exec"));
        // The optimization cannot override explicit owner instructions for
        // web research, browser verification, shell execution or a named MCP.
        let mixed=autonomous_code_tool_window(
            [tools.clone(),vec![def("web_search"),def("http_fetch")]].concat(),
            "Fix src/main.rs, 웹 검색해 확인하고 browser_control, shell_exec, mcp__danger 도구도 사용하세요"
        );
        let names=mixed.iter().map(tool_name).collect::<Vec<_>>();
        for requested in ["web_search","http_fetch","browser_control","shell_exec","mcp__danger"] {
            assert!(names.contains(&requested),"owner-requested tool missing: {requested}");
        }
        // Unrelated capabilities remain omitted from compact single-purpose
        // tasks, and a named tool cannot be manufactured if not authorized.
        let no_shell=autonomous_code_tool_window(tools.into_iter()
            .filter(|v|tool_name(v)!="shell_exec").collect(),"fix x.rs and test; use shell_exec");
        assert!(!no_shell.iter().any(|v|tool_name(v)=="shell_exec"),
            "The window cannot invent a capability denied by upstream policy");
    }

    #[test]
    fn owner_explicit_tools_survive_discovery_even_when_not_core_or_relevant() {
        let def=|name:&str|json!({"type":"function","function":{"name":name,
            "parameters":{"type":"object","description":"unrelated"}}});
        let catalog=vec![def("workspace_read"),def("shell_exec"),def("mcp__archive")];
        let request="Inspect config and use mcp__archive and shell_exec";
        for strategy in ["core","query-relevant"] {
            let chosen=select_tool_definitions(
                catalog.clone(),request,&json!({"implementation":strategy,
                    "parameters":{"tools":["workspace_read"],"extra":0}}));
            let names=chosen.iter().map(tool_name).collect::<Vec<_>>();
            assert!(names.contains(&"mcp__archive"),"{strategy} erased owner tool");
            assert!(names.contains(&"shell_exec"),"{strategy} erased owner tool");
        }
        let allowed=select_tool_definitions(vec![def("workspace_read")],
            request,&json!({"implementation":"core","parameters":{"tools":[]}}));
        assert!(allowed.is_empty(),"Never manufacture a missing capability");
        assert!(!owner_named_tool("mcp__archive_extra","mcp__archive"));
    }
    #[test]
    fn owner_tools_take_priority_over_trim_and_fail_visibly_if_impossible() {
        let def=|name:&str,description:&str|json!({"type":"function",
            "function":{"name":name,"description":description,
            "parameters":{"type":"object"}}});
        let catalog=vec![def("background","x".repeat(350).as_str()),
            def("workspace_read","required"),def("mcp__review","required")];
        let request="Use mcp__review and workspace_read to inspect the result";
        let kept=trim_tool_definitions(catalog.clone(),270,request).unwrap();
        let names=kept.iter().map(tool_name).collect::<Vec<_>>();
        assert_eq!(names,vec!["workspace_read","mcp__review"]);
        assert!(trim_tool_definitions(catalog,20,request).is_err());
    }
    #[test]
    fn room_tool_limit_cannot_silently_drop_named_owner_tool() {
        let def=|name:&str|json!({"type":"function","function":{"name":name,
            "description":"read file review test file source"}}); 
        let mut catalog=(0..12).map(|i|def(&format!("file_tool_{i}"))).collect::<Vec<_>>();
        catalog.push(def("mcp__explicit"));
        let selected=select_room_tools(catalog,
            "Read files and invoke mcp__explicit", &Default::default(),false);
        assert!(selected.iter().any(|d|tool_name(d)=="mcp__explicit"));
        assert!(selected.len()<=8);
    }
}
