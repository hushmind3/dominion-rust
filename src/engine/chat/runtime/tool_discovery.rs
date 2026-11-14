//! DOMINION native, provider-neutral on-demand tool discovery.
 //! The full catalog is already filtered by model/agent permission and RSI
 //! constraints BEFORE reaching this module. Discovery cannot authorize tools.
use super::*;
use std::collections::{BTreeSet, HashSet};

pub(super) const TOOL_SEARCH_NAME: &str = "dominion_tool_search";
const MAX_REVEALS: usize = 20;
const MAX_RESULTS: usize = 6;

/// Small, stable schema stays visible when the full catalog exceeds context.
/// This is DOMINION-owned Rust, not an external provider/SDK search tool.
fn tool_search_definition() -> Value {
    json!({
        "type":"function",
        "function":{
            "name":TOOL_SEARCH_NAME,
            "description":"Search DOMINION's available native and installed MCP tools. Use when the currently visible tools cannot perform a requested step. Matched tool schemas become callable on your NEXT turn. Search does not execute them or grant permission. Do not repeat a query with no matches.",
            "parameters":{
                "type":"object",
                "properties":{
                    "query":{"type":"string","description":"Tool name, API/service, or action needed (e.g. github pull request, browser click, file edit, 기억 검색)."},
                    "limit":{"type":"integer","minimum":1,"maximum":6}
                },
                "required":["query"],"additionalProperties":false
            }
        }
    })
}

fn schema_size(definition: &Value) -> usize {
    serde_json::to_string(definition).map(|s|s.len()).unwrap_or(0)
}

/// Keep explicit revelations visible across model turns; remove unrelated,
/// large schemas from the initial request, never the permission-filtered catalog.
pub(super) fn search_tool_definitions_for_round(
    catalog: &[Value], task: &str, budget: usize, initially_visible: &BTreeSet<String>,
    revealed: &BTreeSet<String>, dynamic: bool,
) -> Vec<Value> {
    let initial=catalog.iter().filter(|d|initially_visible.contains(tool_name(d))).cloned()
        .collect::<Vec<_>>();
    if !dynamic || catalog.is_empty() {
        return super::tool_selection::trim_tool_definitions(initial, budget, task);
    }
    let hidden=catalog.iter().any(|d|!initially_visible.contains(tool_name(d)));
    if !hidden && catalog.iter().map(schema_size).sum::<usize>()<=budget {
        return catalog.to_vec();
    }
    let search=tool_search_definition();
    let search_size=schema_size(&search);
    if search_size>=budget {
        return super::tool_selection::trim_tool_definitions(catalog.to_vec(),budget,task);
    }
    let available=budget-search_size;
    let mut ranked=catalog.iter().enumerate()
        .filter(|(_,def)| !tool_name(def).is_empty() && tool_name(def)!=TOOL_SEARCH_NAME)
        .map(|(i,def)| (i,
            revealed.contains(tool_name(def)),
            initially_visible.contains(tool_name(def)),
            super::tool_selection::tool_schema_relevance(def,task),
            schema_size(def)))
        .collect::<Vec<_>>();
    // A model search must actually reveal the tool on the next turn. Already
    // revealed names outrank initial task relevance; smaller schemas win ties.
    ranked.sort_by(|a,b|b.1.cmp(&a.1).then_with(||b.2.cmp(&a.2))
        .then_with(||b.3.cmp(&a.3)).then_with(||a.4.cmp(&b.4))
        .then_with(||a.0.cmp(&b.0)));
    let mut chosen=HashSet::new();
    let mut used=0usize;
    for (idx,_,_,_,size) in &ranked {
        if *size<=available.saturating_sub(used) {
            chosen.insert(*idx);
            used+=size;
        }
    }
    // The search schema remains available even if all tool schemas are huge.
    let mut result=catalog.iter().enumerate()
        .filter(|(i,_)|chosen.contains(i)).map(|(_,d)|d.clone())
        .collect::<Vec<_>>();
    result.push(search);
    result
}

/// Read persisted discoveries, but validate against CURRENT permission-scoped
/// catalog so no restarted turn can widen an agent's tool permissions.
pub(super) fn restore_revealed(state:&AppState,execution_id:&str,catalog:&[Value]) -> BTreeSet<String> {
    let allowed=catalog.iter().map(tool_name).collect::<HashSet<_>>();
    let Some(current)=state.store.get("execution",execution_id) else{
        return BTreeSet::new();
    };
    // Each resumed attempt creates a NEW execution ID. Recover only from a
    // prior attempt with the identical durable task and session; never import
    // tool disclosures from another owner or a different job.
    let values=current["revealedToolNames"].as_array().cloned().or_else(||{
        let task=current["taskId"].as_str()?;
        let session=current["sessionId"].as_str()?;
        state.store.all("execution").into_iter()
            .filter(|previous|
                previous["id"].as_str()!=Some(execution_id)
                && previous["taskId"].as_str()==Some(task)
                && previous["sessionId"].as_str()==Some(session)
                && previous["revealedToolNames"].as_array()
                    .is_some_and(|items|!items.is_empty())
            )
            .max_by_key(|previous|previous["startedAt"].as_str().unwrap_or("").to_owned())
            .and_then(|previous|previous["revealedToolNames"].as_array().cloned())
    }).unwrap_or_default();
    values.iter().filter_map(Value::as_str)
        .filter(|name|allowed.contains(name))
        .take(MAX_REVEALS).map(str::to_string).collect()
}

fn lexemes(text:&str) -> BTreeSet<String> {
    text.to_lowercase().split(|ch:char|!ch.is_alphanumeric())
        .filter(|s|s.chars().count()>=2)
        .take(72).map(str::to_string).collect()
}

fn aliases(query:&str)->Vec<&'static str> {
    let q=query.to_lowercase();
    let mut out=Vec::new();
    let names:&[(&[&str],&[&str])]=&[
       (&["깃허브","커밋","풀리퀘","리포지토리"],&["github","git","pull","repo"]),
       (&["파일","문서","폴더","소스"],&["workspace","file","read","write"]),
       (&["고쳐","수정","편집","패치"],&["replace","write","edit"]),
       (&["검색","찾아","탐색","조회"],&["search","fetch","list"]),
       (&["웹","인터넷","링크"],&["web","http","browser"]),
       (&["브라우저","클릭","화면"],&["browser","control"]),
       (&["빌드","테스트","실행","터미널"],&["shell","terminal","exec"]),
       (&["기억","회상","메모리"],&["memory","search"]),
       (&["위임","협업","에이전트"],&["delegate","task","agent"]),
       (&["영상","장면","렌더"],&["video","render","scene"])
    ];
    for (terms,words) in names {
        if terms.iter().any(|t|q.contains(t)) {out.extend_from_slice(words);}
    }
    out
}

fn keyword_score(def:&Value,query:&str,terms:&BTreeSet<String>)->i32 {
    let name=tool_name(def).to_lowercase();
    if name.is_empty() {return 0}
    let name_words=lexemes(&name.replace('_'," ").replace('-'," "));
    let description=def["function"]["description"].as_str().or_else(||def["description"].as_str()).unwrap_or("");
    let all_words=lexemes(&format!("{name} {description}"));
    let mut score=0;
    if query.eq_ignore_ascii_case(&name) {score+=200;}
    if name.contains(&query.to_lowercase()) {score+=70;}
    for term in terms{
        if name_words.contains(term) {score+=14;}
        else if all_words.contains(term) {score+=3;}
    }
    for alias in aliases(query){
        if name_words.contains(alias) {score+=11;}
        else if all_words.contains(alias) {score+=2;}
    }
    score
}

/// Search the already-authorized full catalog only. Return names and small
/// descriptions; next round advertises the true provider tool schemas.
pub(super) fn search_catalog(
    catalog:&[Value], query:&str, limit:usize, revealed:&BTreeSet<String>
)->(Vec<Value>,Vec<String>){
    let q=query.trim();
    if q.is_empty()||q.chars().count()>240{return (vec![],vec![]);}
    let terms=lexemes(q);
    let mut ranked=catalog.iter().enumerate()
        .filter(|(_,d)|tool_name(d)!=TOOL_SEARCH_NAME)
        .map(|(i,d)|(i,keyword_score(d,q,&terms)))
        .filter(|(_,score)|*score>0)
        .collect::<Vec<_>>();
    ranked.sort_by(|a,b|
        revealed.contains(tool_name(&catalog[a.0]))
            .cmp(&revealed.contains(tool_name(&catalog[b.0])))
            .then_with(||b.1.cmp(&a.1))
            .then_with(||a.0.cmp(&b.0))
    );
    let matches=ranked.into_iter().take(limit.clamp(1,MAX_RESULTS))
        .map(|(i,_)|&catalog[i]).collect::<Vec<_>>();
    let names=matches.iter().map(|d|tool_name(d).to_string()).collect::<Vec<_>>();
    let payload=matches.iter().map(|d|{
        let description=d["function"]["description"].as_str()
            .or_else(||d["description"].as_str()).unwrap_or("");
        json!({"name":tool_name(d),"description":description.chars().take(140).collect::<String>(),
           "alreadyVisible":revealed.contains(tool_name(d))})
    }).collect::<Vec<_>>();
    (payload,names)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition(name:&str,size:usize)->Value{
        json!({"type":"function","function":{"name":name,
          "description":"x".repeat(size),"parameters":{"type":"object","properties":{}}}})
    }
    #[test]
    fn search_surfaces_hidden_mcp_and_uses_only_authorized_catalog(){
        let all=vec![definition("workspace_read",20),definition("mcp__github__create_pr",50),definition("mcp__secret__delete_all",10)];
        let granted=all[..2].to_vec();
        let (found,names)=search_catalog(&granted,"깃허브 github pull",3,&BTreeSet::new());
        assert!(!found.is_empty());assert!(names.iter().any(|x|x.contains("github")));
        assert!(!names.iter().any(|x|x.contains("secret")));
    }
    #[test]
    fn hidden_schema_is_revealed_next_round_without_breaking_budget(){
        let defs=vec![definition("unrelated_video_renderer",2700),definition("mcp__github__create_pr",250),definition("workspace_read",100)];
        let budget=1100;
        let first=search_tool_definitions_for_round(&defs,"read file",budget,
            &BTreeSet::from(["workspace_read".to_string()]),&BTreeSet::new(),true);
        assert!(first.iter().any(|d|tool_name(d)==TOOL_SEARCH_NAME));
        assert!(!first.iter().any(|d|tool_name(d)=="mcp__github__create_pr"));
        let revealed=BTreeSet::from(["mcp__github__create_pr".to_string()]);
        let second=search_tool_definitions_for_round(&defs,"read file",budget,
            &BTreeSet::from(["workspace_read".to_string()]),&revealed,true);
        assert!(second.iter().any(|d|tool_name(d)=="mcp__github__create_pr"));
        assert!(second.iter().map(schema_size).sum::<usize>()<=budget);
    }
    #[test]
    fn small_catalog_keeps_all_tools_and_no_search(){
        let defs=vec![definition("a",20),definition("b",20)];
        let kept=search_tool_definitions_for_round(&defs,"task",2048,
            &BTreeSet::from(["a".to_string(),"b".to_string()]),&BTreeSet::new(),true);
        assert_eq!(kept,defs);
    }

    #[test]
    fn core_algorithm_does_not_hide_installed_mcp_forever(){
        let defs=vec![definition("workspace_read",120),definition("mcp__github__pull_request",140)];
        let initial=BTreeSet::from(["workspace_read".to_string()]);
        let first=search_tool_definitions_for_round(&defs,"read file",1600,&initial,&BTreeSet::new(),true);
        assert!(first.iter().any(|d|tool_name(d)==TOOL_SEARCH_NAME));
        assert!(first.iter().any(|d|tool_name(d)=="workspace_read"));
        assert!(!first.iter().any(|d|tool_name(d)=="mcp__github__pull_request"));
        let (hits,names)=search_catalog(&defs,"github pull request",4,&BTreeSet::new());
        assert!(!hits.is_empty());
        assert!(names.iter().any(|n|n=="mcp__github__pull_request"));
        let second=search_tool_definitions_for_round(
            &defs,"read file",1600,&initial,
            &BTreeSet::from(["mcp__github__pull_request".to_string()]),true);
        assert!(second.iter().any(|d|tool_name(d)=="mcp__github__pull_request"));
    }
    #[test]
    fn no_keyword_no_match_does_not_invent_a_tool(){
        let defs=vec![definition("workspace_read",10)];
        let (result,names)=search_catalog(&defs,"quantumunicorn",6,&BTreeSet::new());
        assert!(result.is_empty());assert!(names.is_empty());
    }
    #[test]
    fn search_never_turns_an_unadvertised_tool_into_an_effect(){
        let defs=vec![definition("workspace_read",10)];
        let (_,names)=search_catalog(&defs,"github api",4,&BTreeSet::new());
        assert!(names.is_empty());
    }
}
