//! tool selection: independent chat runtime responsibility.
use super::*;

pub(super) fn tool_is_read_only(name: &str) -> bool {
    matches!(
        name,
        "workspace_list" | "workspace_read" | "workspace_read_range" | "workspace_diff" | "file_list" | "file_read"
            | "web_search" | "http_fetch" | "memory_search"
            | "task_status" | "task_result" | "account_status"
    )
}

pub(super) fn mark_tool_argument_signal(
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

pub(super) fn normalize_tool_arguments(mut arguments: Value, strategy: &str) -> Value {
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

/// Authoritative schema boundary for this exact model round. A tool the
/// model has not been shown must never be dispatched merely because the
/// underlying process has a global implementation for it (including MCP).
/// The allowlist is the actual post-selection, post-budget, post-phase menu.
pub(super) fn tool_advertised(name:&str,round_tools:&[Value])->bool{
    !name.is_empty()&&round_tools.iter().any(|definition|tool_name(definition)==name)
}

pub(super) fn tool_name(definition: &Value) -> &str {
    definition["function"]["name"]
        .as_str()
        .or_else(|| definition["name"].as_str())
        .unwrap_or("")
}

/// Casual room turns carry zero tool schemas. Action requests select the
/// exact native schemas the harness judges relevant; no model call is used.
pub(super) fn room_tool_request(input: &str) -> bool {
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

pub(super) fn recent_room_tools(state: &AppState, session_id: &str) -> std::collections::HashSet<String> {
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

pub(super) fn select_room_tools(
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
            &["workspace_list", "workspace_read", "workspace_read_range", "workspace_diff", "file_list", "file_read"],
        ),
        (
            &[
                "쓰기", "써줘", "써 줘", "저장해", "저장하", "수정해", "수정하",
                "삭제해", "삭제하", "만들어", "만들어줘", "작성해", "작성하",
                "기록", "남겨", "write", "save", "edit", "delete", "create",
            ],
            &["workspace_read", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "file_write"],
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
            &["delegate_task", "delegate_parallel", "task_status", "task_result"],
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
                    "workspace_list" | "workspace_read" | "workspace_read_range" | "workspace_replace_range" | "workspace_replace" | "workspace_write" | "shell_exec" | "web_search"
                )
            })
            .collect();
    }
    scored.truncate(8);
    scored.into_iter().map(|(_, definition)| definition).collect()
}

pub(super) fn select_tool_definitions(
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
        "workspace_list", "workspace_read", "workspace_read_range", "workspace_diff", "workspace_replace_range", "workspace_replace", "workspace_write", "shell_exec",
        "web_search", "memory_store", "memory_search", "delegate_task", "delegate_parallel",
        "task_status", "task_result",
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

pub(super) fn trim_tool_definitions(definitions: Vec<Value>, budget: usize) -> Vec<Value> {
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



/// Select a query-relevant *subset* only when the provider's schema byte
/// allowance cannot fit the full tool catalog. Unlike naive first-N trimming,
/// do not silently hide an editing tool just because many unrelated MCP
/// definitions were registered before it. The output retains original order
/// for stable provider prefix caching. Tool execution still checks this exact
/// menu and all native/MCP permission policies.
pub(super) fn budgeted_tool_definitions(
    definitions:Vec<Value>,budget:usize,query:&str,
    recent:&std::collections::HashSet<String>,
)->Vec<Value>{
    let sizes=definitions.iter().map(|definition|
        serde_json::to_string(definition).map(|s|s.len()).unwrap_or(usize::MAX)
    ).collect::<Vec<_>>();
    let total=sizes.iter().try_fold(0usize,|sum,size|sum.checked_add(*size));
    if total.is_some_and(|bytes|bytes<=budget){return definitions;}
    if definitions.is_empty(){return definitions;}
    let query=query.to_lowercase();
    let tokens=query.split(|c:char|!c.is_alphanumeric())
        .filter(|token|token.chars().count()>1).take(96).collect::<Vec<_>>();
    let contains_any=|terms:&[&str]|terms.iter().any(|term|query.contains(term));
    let editing=contains_any(&["파일","수정","편집","코드","리팩터","패치",
        "write","edit","patch","fix","refactor","source"]);
    let coding=editing||contains_any(&["빌드","테스트","컴파일","실행","build","test","run","compile"]);
    let searching=contains_any(&["검색","찾아","검색해","search","web","http","뉴스"]);
    let memory=contains_any(&["기억","메모리","memory","recall"]);
    let github=contains_any(&["깃허브","github","repository","pull request","이슈"]);
    let delegated=contains_any(&["위임","협업","에이전트","delegate","subagent"]);
    let mut ranked=definitions.iter().enumerate().map(|(index,definition)|{
        let name=tool_name(definition).to_lowercase();
        let haystack=serde_json::to_string(definition).unwrap_or_default().to_lowercase();
        let mut score:i64=0;
        if !name.is_empty()&&query.contains(name.as_str()){score+=100;}
        for token in &tokens {
            if name.contains(*token){score+=16;}
            else if haystack.contains(*token){score+=2;}
        }
        if recent.contains(&name){score+=36;}
        if editing&&matches!(name.as_str(),
            "workspace_read"|"workspace_read_range"|"workspace_replace"|
            "workspace_replace_range"|"workspace_write"|"workspace_list"|"workspace_diff"){
            score+=38;
        }
        if coding&&matches!(name.as_str(),"shell_exec"|"terminal_exec"){score+=34;}
        if searching&&matches!(name.as_str(),"web_search"|"http_fetch"){score+=42;}
        if memory&&matches!(name.as_str(),"memory_search"|"memory_store"){score+=42;}
        if github&&(name=="github_api"||name.contains("github")){score+=45;}
        if delegated&&matches!(name.as_str(),"delegate_task"|"delegate_parallel"|
            "task_status"|"task_result"){score+=36;}
        // Preserve a minimum observation path for non-specific queries.
        if matches!(name.as_str(),"workspace_read"|"workspace_list"|"memory_search"){score+=8;}
        (index,score,sizes[index])
    }).collect::<Vec<_>>();
    ranked.sort_by(|a,b|
        b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
    let mut used=0usize;
    let mut chosen=std::collections::HashSet::new();
    // Coding requires an executable tool *chain*, not merely the highest
    // independently-scored schemas. A cheap read/edit/test/diff bundle is
    // reserved first when all four operations fit the provider allowance.
    // Select the smallest usable schema within each role, without inventing
    // tools absent from the permission-filtered input catalog.
    if coding {
        let roles: &[&[&str]] = &[
            &["workspace_read_range","workspace_read","file_read"],
            &["workspace_replace_range","workspace_replace","workspace_write","file_write"],
            &["shell_exec","terminal_exec"],
            &["workspace_diff"],
        ];
        let anchors=roles.iter().map(|alternatives|{
            definitions.iter().enumerate()
                .filter(|(index,definition)|
                    sizes[*index]!=usize::MAX
                    && alternatives.contains(&tool_name(definition)))
                .min_by_key(|(index,_)|sizes[*index])
                .map(|(index,_)|index)
        }).collect::<Option<Vec<_>>>();
        if let Some(anchors)=anchors {
            let required=anchors.iter().try_fold(0usize,|sum,index|
                sum.checked_add(sizes[*index]));
            if required.is_some_and(|bytes|bytes<=budget) {
                for index in anchors {
                    if chosen.insert(index) {used+=sizes[index];}
                }
            }
        }
    }
    for (index,_score,size) in ranked {
        if size==usize::MAX || chosen.contains(&index) {continue;}
        if size<=budget.saturating_sub(used) {
            used+=size;
            chosen.insert(index);
        }
    }
    // A provider limit is a real byte ceiling. When no schema can fit,
    // advertise no tools instead of inventing capacity by sending one
    // oversized function signature that could reject the entire model turn.
    // The goal's normal verification gate will then prevent false completion.
    if chosen.is_empty(){return Vec::new();}
    definitions.into_iter().enumerate()
        .filter_map(|(index,definition)|chosen.contains(&index).then_some(definition))
        .collect()
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use serde_json::json;

    fn schema(name:&str)->Value {
        json!({"type":"function","function":{"name":name,"description":"execute a tool",
            "parameters":{"type":"object","properties":{}}}})
    }

    #[test]
    fn coding_budget_keeps_complete_read_edit_test_diff_chain() {
        let essentials=["workspace_read_range","workspace_replace_range",
            "shell_exec","workspace_diff"].into_iter().map(schema).collect::<Vec<_>>();
        let allowance=essentials.iter()
            .map(|tool|serde_json::to_string(tool).unwrap().len()).sum();
        let mut offered=vec![schema("mcp_code_helper"),schema("mcp_code_extra")];
        offered.extend(essentials);
        let recent=["mcp_code_helper".to_string(),"mcp_code_extra".to_string()]
            .into_iter().collect();
        let selected=budgeted_tool_definitions(offered,allowance,"fix code and run tests",&recent);
        let names=selected.iter().map(tool_name).collect::<Vec<_>>();
        for essential in ["workspace_read_range","workspace_replace_range",
            "shell_exec","workspace_diff"] {
            assert!(names.contains(&essential),"{essential} missing from coding chain");
        }
        assert_eq!(selected.iter()
            .map(|tool|serde_json::to_string(tool).unwrap().len()).sum::<usize>(),allowance);
        assert_eq!(names,vec!["workspace_read_range","workspace_replace_range",
            "shell_exec","workspace_diff"]);
    }

    #[test]
    fn schema_budget_never_introduces_unadvertised_tools() {
        let offered=vec![schema("workspace_read"),schema("shell_exec")];
        let limit=serde_json::to_string(&offered[0]).unwrap().len();
        let selected=budgeted_tool_definitions(offered.clone(),limit,
            "refactor and test", &std::collections::HashSet::new());
        assert!(!selected.is_empty());
        assert!(selected.iter().all(|row|offered.contains(row)));
        assert!(selected.iter().map(|row|serde_json::to_string(row).unwrap().len())
            .sum::<usize>()<=limit);
    }

    #[test]
    fn impossible_budget_never_leaks_an_oversized_schema() {
        let catalog=vec![schema("workspace_write"),schema("shell_exec")];
        let selected=budgeted_tool_definitions(
            catalog,1,"fix code and run tests",&std::collections::HashSet::new());
        assert!(selected.is_empty());
    }

    #[test]
    fn phase_reconstructed_menu_still_respects_hard_schema_budget() {
        let catalog=vec![
            schema("workspace_read"),schema("workspace_replace_range"),
            schema("shell_exec"),schema("workspace_diff"),
            schema("mcp_enormous_unused_tools")];
        // A staged RSI edit policy can reconstruct its menu from the entire
        // catalog after general discovery. The final provider boundary must
        // budget this phase-specific menu afresh, without inventing tools.
        let phase=catalog.iter().filter(|tool|
            matches!(tool_name(tool),"workspace_read"|
                "workspace_replace_range"|"shell_exec")).cloned().collect::<Vec<_>>();
        let budget=serde_json::to_string(&phase[1]).unwrap().len();
        let emitted=budgeted_tool_definitions(
            phase.clone(),budget,"replace the changed range",
            &std::collections::HashSet::new());
        assert!(!emitted.is_empty());
        assert!(emitted.iter().all(|tool|phase.contains(tool)));
        let encoded=emitted.iter()
            .map(|tool|serde_json::to_string(tool).unwrap().len()).sum::<usize>();
        assert!(encoded<=budget,"post-phase menu exceeded byte ceiling");
    }

    #[test]
    fn noncoding_budgets_keep_existing_relevance_ranking() {
        let offered=vec![schema("weather_search"),schema("workspace_write")];
        let allowance=serde_json::to_string(&offered[0]).unwrap().len();
        let selected=budgeted_tool_definitions(offered,allowance,"weather",
            &std::collections::HashSet::new());
        assert_eq!(selected.len(),1);
        assert_eq!(tool_name(&selected[0]),"weather_search");
    }
}
