//! Permission-scoped progressive tool discovery for native and MCP tools.
//! Search reveals schemas on a later round, NEVER grants an execution right.
use super::*;
use std::collections::{BTreeSet,HashSet};

pub(super) const SEARCH_NAME:&str="dominion_tool_search";
const MAX_REVEALS:usize=20;

fn bytes(v:&Value)->usize{serde_json::to_string(v).map(|s|s.len()).unwrap_or(usize::MAX)}
pub(super) fn definition()->Value{
    json!({"type":"function","function":{
        "name":SEARCH_NAME,
        "description":"Find an authorized DOMINION tool by name, action, or service when the visible tools cannot complete a task. Matching schemas become callable NEXT round. This never executes or authorizes a tool.",
        "parameters":{"type":"object","properties":{
            "query":{"type":"string","description":"Service or action, e.g. GitHub create PR, browser click, file edit"},
            "limit":{"type":"integer","minimum":1,"maximum":6}
        },"required":["query"],"additionalProperties":false}
    }})
}

/// Keep the model's usual cached order. Reserve full schemas for explicit
/// discoveries, then fill with the normal context-aware routing candidates.
/// The search schema itself always fits the same byte budget.
pub(super) fn offer(
    catalog:&[Value], initial:&[Value], budget:usize, revealed:&BTreeSet<String>,
)->Vec<Value>{
    let initial_names=initial.iter().map(tool_name).collect::<HashSet<_>>();
    let eligible=catalog.iter().filter(|d|{
        let name=tool_name(d);
        !name.is_empty() && name!=SEARCH_NAME
    }).collect::<Vec<_>>();
    let all_fit=eligible.iter().map(|d|bytes(d)).sum::<usize>()<=budget;
    let hidden=eligible.iter().any(|d|!initial_names.contains(tool_name(d)));
    if !hidden && all_fit {return initial.to_vec();}
    let search=definition();
    let search_bytes=bytes(&search);
    if search_bytes.saturating_add(128)>=budget {
        return trim_tool_definitions(initial.to_vec(),budget);
    }
    let mut remaining=budget-search_bytes;
    let mut picked=HashSet::new();
    // A discovery must have priority next round or it becomes an empty promise.
    for def in &eligible {
        if !revealed.contains(tool_name(def)) {continue;}
        let size=bytes(def);
        if size<=remaining {picked.insert(tool_name(def).to_string());remaining-=size;}
    }
    let others=initial.iter().filter(|d|!picked.contains(tool_name(d))).cloned()
        .collect::<Vec<_>>();
    for def in trim_tool_definitions(others,remaining){
        picked.insert(tool_name(&def).to_string());
    }
    let mut result=eligible.into_iter().filter(|d|picked.contains(tool_name(d)))
        .map(|d|d.clone()).collect::<Vec<_>>();
    result.push(search);
    result
}

fn words(s:&str)->Vec<String>{
    s.to_lowercase().split(|c:char|!c.is_alphanumeric()).filter(|t|t.chars().count()>1)
        .take(40).map(str::to_string).collect()
}
fn keyword_score(def:&Value,query:&str)->i32{
    let name=tool_name(def).to_lowercase();
    if name.is_empty(){return 0;}
    let desc=def["function"]["description"].as_str().unwrap_or("");
    let content=format!("{name} {}",desc.to_lowercase());
    let lower=query.trim().to_lowercase();
    let mut score=if lower==name {200}else if name.contains(&lower){70}else{0};
    for token in words(&lower){
        if name.contains(&token){score+=16;}
        else if content.contains(&token){score+=3;}
    }
    let aliases:&[(&[&str],&[&str])]=&[
        (&["깃허브","커밋","풀리퀘"],&["github","git","pull","repo"]),
        (&["파일","소스","문서"],&["workspace","file","read","write"]),
        (&["수정","편집","패치"],&["replace","write","edit"]),
        (&["검색","찾아","탐색"],&["search","fetch","list"]),
        (&["브라우저","클릭"],&["browser","control"]),
        (&["영상","장면","렌더"],&["video","render","scene"]),
        (&["명령","실행","빌드"],&["shell","terminal","exec"]),
    ];
    for (terms,names) in aliases{
        if terms.iter().any(|term|lower.contains(term)){
            for alias in *names {
                if name.contains(alias){score+=11;}
                else if content.contains(alias){score+=2;}
            }
        }
    }
    score
}
pub(super) fn search(
    catalog:&[Value],query:&str,limit:usize,revealed:&BTreeSet<String>,
)->(Vec<Value>,Vec<String>){
    let q=query.trim();
    if q.is_empty()||q.chars().count()>240{return (vec![],vec![]);}
    let mut ranked=catalog.iter().enumerate()
        .filter(|(_,d)|tool_name(d)!=SEARCH_NAME)
        .map(|(i,d)|(i,keyword_score(d,q)))
        .filter(|(_,score)|*score>0).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b.1.cmp(&a.1).then_with(||a.0.cmp(&b.0)));
    let mut matches=Vec::new();let mut newly=Vec::new();
    for (i,_) in ranked.into_iter().take(limit.clamp(1,6)){
        let d=&catalog[i];let name=tool_name(d);
        let known=revealed.contains(name);
        let desc=d["function"]["description"].as_str().unwrap_or("");
        matches.push(json!({"name":name,"description":desc.chars().take(150).collect::<String>(),
            "alreadyRevealed":known}));
        if !known && revealed.len()+newly.len()<MAX_REVEALS {newly.push(name.to_string());}
    }
    (matches,newly)
}

pub(super) fn restore(state:&AppState,execution_id:&str,catalog:&[Value])->BTreeSet<String>{
    let allowed=catalog.iter().map(tool_name).collect::<HashSet<_>>();
    state.store.get("execution",execution_id).and_then(|e|e["revealedToolNames"].as_array().cloned())
        .unwrap_or_default().iter().filter_map(Value::as_str)
        .filter(|name|allowed.contains(name)).take(MAX_REVEALS)
        .map(str::to_string).collect()
}

#[cfg(test)]
mod tests{
    use super::*;
    fn def(name:&str,n:usize)->Value{
        json!({"type":"function","function":{"name":name,
            "description":"x".repeat(n),"parameters":{"type":"object","properties":{}}}})
    }
    #[test]
    fn search_never_returns_tools_outside_the_authorized_catalog(){
        let authorized=vec![def("workspace_read",50),def("mcp__github__create_pull_request",50)];
        let (found,added)=search(&authorized,"깃허브 create pull",4,&BTreeSet::new());
        assert_eq!(added,vec!["mcp__github__create_pull_request"]);
        assert_eq!(found[0]["name"],"mcp__github__create_pull_request");
        assert!(!found.to_string().contains("mcp__secret"));
    }
    #[test]
    fn revelation_is_callable_next_round_within_schema_budget(){
        let allowed=vec![def("workspace_read",160),def("mcp__github__create_pull_request",210),
            def("unrelated_video_render",2600)];
        let first=offer(&allowed,&allowed[..1],1100,&BTreeSet::new());
        assert!(first.iter().any(|d|tool_name(d)==SEARCH_NAME));
        assert!(!first.iter().any(|d|tool_name(d)=="mcp__github__create_pull_request"));
        let revealed=BTreeSet::from(["mcp__github__create_pull_request".to_string()]);
        let next=offer(&allowed,&allowed[..1],1100,&revealed);
        assert!(next.iter().any(|d|tool_name(d)=="mcp__github__create_pull_request"));
        assert!(next.iter().map(bytes).sum::<usize>()<=1100);
        assert!(!next.iter().any(|d|tool_name(d)=="unrelated_video_render"));
    }
    #[test]
    fn no_overhead_when_all_tools_fit(){
        let defs=vec![def("workspace_read",20)];
        assert_eq!(offer(&defs,&defs,4096,&BTreeSet::new()),defs);
    }
}
