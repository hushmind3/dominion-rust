//! 2026 progressive disclosure for *existing* authorized external tools.
//! Discovery never owns a second executor, approval path, or remote permission.
//! A successful tool_search receipt unlocks native model schemas on the next
//! turn; the original tool is then dispatched by the existing journal.
use crate::{domain::Run,policy};
use serde_json::{json,Value};
use std::collections::BTreeSet;

pub const THRESHOLD:usize=8;
const MAX_RETAINED:usize=16;
const MAX_RESULT:usize=8;

fn name(tool:&Value)->Option<&str>{
    tool.pointer("/function/name").and_then(Value::as_str)
}
fn allowed<'a>(run:&Run,tools:impl IntoIterator<Item=&'a Value>,
    denied:&[String])->Vec<&'a Value>{
    tools.into_iter().filter(|tool|name(tool)
        .is_some_and(|n|policy::admitted_tool(run,n,denied))).collect()
}
/// Small external catalogs remain eager: cold discovery has a real model-turn
/// cost. If search was disabled, NEVER hide the tools needed to complete work.
pub fn for_turn(run:&Run,tools:Vec<Value>,denied:&[String])->(Vec<Value>,usize,bool){
    let eligible=allowed(run,tools.iter(),denied);
    if eligible.len()<=THRESHOLD || !policy::admitted_tool(run,"tool_search",denied){
        return (tools,0,false);
    }
    let original=eligible.iter().map(|t|t.to_string().len()).sum::<usize>();
    let visible=tools.into_iter().filter(|tool|name(tool)
        .is_some_and(|n|run.discovered_tools.iter().any(|saved|saved==n)))
        .collect::<Vec<_>>();
    let retained=visible.iter().map(|t|t.to_string().len()).sum::<usize>();
    (visible,original.saturating_sub(retained),true)
}
fn words(s:&str)->BTreeSet<String>{
    s.to_lowercase().split(|c:char|!c.is_alphanumeric())
        .filter(|w|w.chars().count()>=2 && !matches!(*w,
            "the"|"for"|"and"|"with"|"from"|"this"|"that"|"what"|
            "need"|"tool"|"tools"|"please"|"using"|"use"|"can"))
        .map(str::to_string).collect()
}
fn query_terms(query:&str)->BTreeSet<String>{
    let mut enriched=query.to_lowercase();
    for (key,aliases) in [
        ("검색"," search find query"),("조사"," search fetch research"),
        ("수정"," edit update patch"),("코드"," code file github"),
        ("깃허브"," github"),("이슈"," issue"),("저장"," write create"),
        ("문서"," document docs drive"),("시트"," sheets spreadsheet"),
        ("搜索"," search find"),("代码"," code github file"),
        ("问题"," issue"),("修改"," edit update")
    ]{
        if enriched.contains(key){enriched.push_str(aliases);}
    }
    words(&enriched)
}
struct Doc<'a> {
    tool:&'a Value,
    name:String,
    name_terms:BTreeSet<String>,
    description:String,
    fields:BTreeSet<String>,
}
fn documents<'a>(eligible:Vec<&'a Value>)->Vec<Doc<'a>>{
    eligible.into_iter().filter_map(|tool|{
        let n=name(tool)?.to_lowercase();
        let desc=tool.pointer("/function/description").and_then(Value::as_str)
            .unwrap_or("").to_lowercase();
        let fields=tool.pointer("/function/parameters/properties")
            .and_then(Value::as_object).map(|m|m.keys().cloned().collect())
            .unwrap_or_default();
        Some(Doc{tool,name_terms:words(&n),name:n,description:desc,fields})
    }).collect()
}
fn score(doc:&Doc<'_>,query:&str,terms:&BTreeSet<String>,docs:&[Doc<'_>])->f64{
    let mut score=if doc.name.contains(query){24.0}else{0.0};
    for term in terms {
        let weight=|item:&Doc<'_>|{
            if item.name_terms.contains(term){16.0}
            else if item.name.contains(term){9.0}
            else if item.fields.iter().any(|field|field.to_lowercase().contains(term)){5.0}
            else if item.description.contains(term){2.0}
            else{0.0}
        };
        let tf=weight(doc);
        if tf==0.0{continue}
        let frequency=docs.iter().filter(|d|weight(d)>0.0).count();
        let n=docs.len() as f64;
        // Smoothed IDF floor: common service names (all tools are GitHub)
        // must not become unsearchable through zero inverse frequency.
        let idf=(1.0+(n-frequency as f64+0.5)/(frequency as f64+0.5)).ln().max(0.3);
        score+=idf*tf;
    }
    score
}
/// Read-only discovery over the current Run's actual operator-authorized
/// catalog. Returns names/required fields, never an execution grant.
pub fn search(run:&Run,tools:Vec<Value>,denied:&[String],
    query:&str,limit:usize)->Result<String,String>{
    let query=query.trim();
    if query.is_empty()||query.chars().count()>256 {return Err("tool_search query must contain 1..256 characters".into());}
    if !(1..=MAX_RESULT).contains(&limit){return Err("tool_search limit must be 1..8".into());}
    let docs=documents(allowed(run,tools.iter(),denied));
    let terms=query_terms(query);
    let lowercase=query.to_lowercase();
    let mut ranks=docs.iter().map(|doc|(score(doc,&lowercase,&terms,&docs),doc))
        .filter(|(score,_)|*score>0.0).collect::<Vec<_>>();
    ranks.sort_by(|a,b|b.0.total_cmp(&a.0).then_with(||a.1.name.cmp(&b.1.name)));
    let matches=ranks.into_iter().take(limit).map(|(_,doc)|{
        let required=doc.tool.pointer("/function/parameters/required")
            .and_then(Value::as_array).map(|v|v.iter().take(12).cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        json!({"name":name(doc.tool),"description":
            doc.description.chars().take(180).collect::<String>(),"required":required})
    }).collect::<Vec<_>>();
    Ok(json!({"query":query,"available":docs.len(),"matches":matches,
        "next":"Matched tools become directly available with validated JSON schemas on the next model turn. Use their exact names; existing approval and runtime policies still apply."}).to_string())
}
/// Only successful, hook-approved, journaled search receipts can update this
/// bounded discovery view. Existing sessions deserialize with an empty list.
pub fn names_from_receipt(output:&str)->Vec<String>{
    serde_json::from_str::<Value>(output).ok()
        .and_then(|v|v.get("matches").and_then(Value::as_array).cloned())
        .unwrap_or_default().into_iter().filter_map(|item|{
            item.get("name").and_then(Value::as_str)
                .filter(|name|!name.is_empty()&&name.len()<=128)
                .map(str::to_owned)
        }).take(MAX_RESULT).collect()
}
pub fn remember(run:&mut Run,names:&[String]){
    let mut changed=Vec::new();
    for name in names {
        if let Some(idx)=run.discovered_tools.iter().position(|n|n==name){
            run.discovered_tools.remove(idx);
        }else{changed.push(name.clone());}
        run.discovered_tools.push(name.clone());
        if run.discovered_tools.len()>MAX_RETAINED{run.discovered_tools.remove(0);}
    }
    if !changed.is_empty(){
        run.event("tools_discovered",
            format!("Disclosed {} external tool schemas for the next model turn",changed.len()),
            Some(json!({"names":changed})));
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{domain::{CreateRun,ToolCall},tool_schema};
    fn stub(name:&str,desc:&str)->Value{
        json!({"type":"function","function":{"name":name,"description":desc,
            "parameters":{"type":"object","properties":{"title":{"type":"string"}},
                "required":["title"]}}})
    }
    fn run()->Run{
        Run::new(CreateRun{instruction:"Fix issue".into(),budget_krw:None,max_steps:Some(10)},10).unwrap()
    }
    fn catalog()->Vec<Value>{
        let mut c=(0..20).map(|i|stub(&format!("mcp__repo__generic_{i:02}"),
            "Generic repository tool")).collect::<Vec<_>>();
        c.push(stub("mcp__repo__create_issue","Create GitHub issue or ticket"));
        c.push(stub("mcp__repo__get_issue","Read GitHub issue"));
        c
    }
    #[test]
    fn progressive_discovery_requires_an_observed_receipt_and_original_schema(){
        let mut r=run();
        let external=catalog();
        let (hidden,bytes,active)=for_turn(&r,external.clone(),&[]);
        assert!(active&&hidden.is_empty()&&bytes>300);
        let found=search(&r,external.clone(),&[],"create github issue",4).unwrap();
        let names=names_from_receipt(&found);
        assert_eq!(names.first().map(String::as_str),Some("mcp__repo__create_issue"));
        let queued=ToolCall{id:"x".into(),name:names[0].clone(),arguments:json!({"title":"Bug"})};
        assert!(tool_schema::admit(&queued,&hidden).is_err());
        remember(&mut r,&names);
        let restored:Run=serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        let (visible,_,_)=for_turn(&restored,external.clone(),&[]);
        assert!(tool_schema::admit(&queued,&visible).is_ok());
        assert!(tool_schema::admit(&ToolCall{arguments:json!({}),..queued},&visible).is_err());
        let forbidden=vec!["mcp__repo__create_issue".into()];
        let (revoked,_,_)=for_turn(&restored,external,&forbidden);
        assert!(!revoked.iter().any(|x|name(x)==Some("mcp__repo__create_issue")));
    }
    #[test]
    fn no_discovery_bridge_overhead_for_small_catalog_or_disabled_search(){
        let r=run();
        assert_eq!(for_turn(&r,catalog()[..4].to_vec(),&[]).0.len(),4);
        let denied=vec!["tool_search".into()];
        assert_eq!(for_turn(&r,catalog(),&denied).0.len(),22);
    }
    #[test]
    fn read_only_scope_and_removed_endpoint_do_not_leak_old_disclosures(){
        let mut r=run();let external=catalog();
        let _=search(&r,external.clone(),&[],"repo",5).unwrap();
        remember(&mut r,&["mcp__repo__create_issue".to_string()]);
        r.read_only=true;
        assert!(names_from_receipt(&search(&r,external.clone(),&[],"create issue",5).unwrap()).is_empty());
        assert!(for_turn(&r,external,&[]).0.is_empty());
        r.read_only=false;
        assert!(for_turn(&r,vec![stub("mcp__other__ping","Ping")],&[]).0.len()==1);
    }
    #[test]
    fn old_run_json_restores_without_discovered_tools(){
        let r=run();
        let mut v=serde_json::to_value(r).unwrap();
        v.as_object_mut().unwrap().remove("discovered_tools");
        let restored:Run=serde_json::from_value(v).unwrap();
        assert!(restored.discovered_tools.is_empty());
    }
    #[test]
    fn repeated_search_receipts_are_bounded_and_do_not_duplicate_names(){
        let mut r=run();
        for i in 0..30 {remember(&mut r,&[format!("mcp__x__{i}")]);}
        assert_eq!(r.discovered_tools.len(),MAX_RETAINED);
        remember(&mut r,&["mcp__x__29".into()]);
        assert_eq!(r.discovered_tools.len(),MAX_RETAINED);
        assert_eq!(r.discovered_tools.last().unwrap(),"mcp__x__29");
    }
}
