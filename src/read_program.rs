//! Evidence-directed Rust execution program, independent of any model provider.
//! One admitted model call declares a read-only partial-order program. The
//! kernel schedules resource-bounded ready nodes, derives decisions from real
//! observations, and emits content-addressed evidence. No synthetic score,
//! automatic source adoption, nested model call, or duplicated tool journal.
//!
//! 2026 reference architecture: programmatic tool execution and telemetry-aware
//! harness control. This is a typed, native partial-order design, not a port of
//! an external TypeScript executor.
use crate::{config::Settings,domain::{Run,ToolCall},policy,tool_schema,tools};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{HashMap,HashSet},time::Duration};
use tokio::{task::JoinSet,time::timeout};
use uuid::Uuid;

const MAX_STEPS:usize=12;
const MAX_ITERATION:usize=8;
const MAX_NESTED_READS:usize=24;
const MAX_RECEIPT_CHARS:usize=1500;
const MAX_PARALLEL:usize=4;

#[derive(Clone,Debug)]
struct Guard{
    source:String,
    status:String,
    contains:Option<String>,
    negate:bool,
}
#[derive(Clone,Debug)]
struct Step{
    id:String,
    tool:String,
    arguments:Value,
    guard:Option<Guard>,
    deps:Vec<String>,
    from:Option<String>,
    suffix:String,
    needle:String,
    max_files:usize,
    lines_containing:Option<String>,
}
#[derive(Clone,Debug)]
struct Assertion{
    source:String,
    contains:Option<String>,
    status:String,
}
#[derive(Clone,Debug)]
struct Objective{
    mode:String,
    assertions:Vec<Assertion>,
}
#[derive(Clone,Debug)]
struct Program{
    steps:Vec<Step>,
    objective:Option<Objective>,
}
#[derive(Clone,Debug)]
struct Observation{
    status:String,
    raw:String,
    receipt:Value,
    calls:usize,
    failures:usize,
}
fn field<'a>(v:&'a Value,name:&str)->Result<&'a str,String>{
    v.get(name).and_then(Value::as_str).ok_or_else(||format!("read_program requires {name}"))
}
fn digest(text:&str)->String{
    format!("{:x}",Sha256::digest(text.as_bytes()))
}
fn guard_condition(guard:&Guard,found:&Observation)->bool{
    // Tool failure is never evidence of success. A caller must explicitly
    // request status=error before inspecting its diagnostic text.
    // A negative substring predicate must not turn a failed read into
    // positive evidence. The status match is mandatory even under negation.
    if found.status!=guard.status{return false;}
    let contained=guard.contains.as_ref()
        .map(|needle|found.raw.contains(needle)).unwrap_or(true);
    if guard.negate{!contained}else{contained}
}
fn objective_satisfied(objective:&Objective,observed:&HashMap<String,Observation>)->bool{
    let matches=|assertion:&Assertion|{
        observed.get(&assertion.source).is_some_and(|item|
            item.status==assertion.status && assertion.contains.as_ref()
                .is_none_or(|needle|item.raw.contains(needle)))
    };
    if objective.mode=="all"{objective.assertions.iter().all(matches)}
    else{objective.assertions.iter().any(matches)}
}
fn parse(request:&Value,run:&Run,settings:&Settings)->Result<Program,String>{
    if request.to_string().len()>24_000{return Err("read_program request exceeds 24 KB".into());}
    let steps=request.get("steps").and_then(Value::as_array)
        .ok_or("read_program requires steps array")?;
    if steps.is_empty()||steps.len()>MAX_STEPS{
        return Err(format!("read_program needs 1..{MAX_STEPS} steps"));
    }
    let mut previous=HashMap::<String,String>::new();
    let mut parsed=Vec::with_capacity(steps.len());
    let catalog=tools::definitions(false);
    for raw in steps {
        let id=field(raw,"id")?.to_string();
        if id.is_empty()||id.len()>40||!id.bytes().all(|b|
            b.is_ascii_alphanumeric()||b==b'_'||b==b'-'){
            return Err("step id must be 1..40 ASCII alphanumeric, _ or -".into());
        }
        if previous.contains_key(&id){return Err(format!("duplicate step id {id}"));}
        let tool=field(raw,"tool")?.to_string();
        if !matches!(tool.as_str(),
            "list_files"|"read_file"|"search_files"|"git_status"|"git_diff"|"read_listed_files"|"read_matched_files"){
            return Err(format!("read_program forbids nested {tool}"));
        }
        let mut deps=Vec::new();
        if let Some(after)=raw.get("after"){
            let list=after.as_array().ok_or("after must be array of earlier step ids")?;
            if list.len()>MAX_STEPS{return Err("too many step dependencies".into());}
            for dep in list{
                let name=dep.as_str().ok_or("dependency must be string")?;
                if !previous.contains_key(name){return Err("dependency must precede its consumer".into());}
                if !deps.iter().any(|item|item==name){deps.push(name.to_string());}
            }
        }
        let guard=if let Some(when)=raw.get("when"){
            let source=field(when,"step")?;
            if !previous.contains_key(source){return Err("when.step must refer to an earlier step".into());}
            let status=when.get("status").and_then(Value::as_str).unwrap_or("ok");
            if !matches!(status,"ok"|"error"|"skipped"){
                return Err("when.status must be ok, error or skipped".into());
            }
            let contains=when.get("contains").map(|x|
                x.as_str().ok_or("when.contains must be text")
            ).transpose()?;
            if contains.is_some_and(|s|s.is_empty()||s.len()>256){
                return Err("when.contains must contain 1..256 chars".into());
            }
            if contains.is_none()&&when.get("status").is_none(){
                return Err("when must specify status or contains".into());
            }
            if !deps.iter().any(|d|d==source){deps.push(source.to_owned());}
            Some(Guard{source:source.to_owned(),status:status.into(),
                contains:contains.map(str::to_owned),
                negate:when.get("not").and_then(Value::as_bool).unwrap_or(false)})
        }else{None};
        let lines_containing=raw.get("lines_containing").map(|v|
            v.as_str().ok_or("lines_containing must be text")
        ).transpose()?;
        if lines_containing.is_some_and(|v|v.is_empty()||v.len()>256){
            return Err("lines_containing must contain 1..256 chars".into());
        }
        let arguments=raw.get("arguments").cloned().unwrap_or_else(||json!({}));
        let mut from=None;
        let mut suffix=String::new();
        let mut needle=String::new();
        let mut max_files=1;
        if matches!(tool.as_str(),"read_listed_files"|"read_matched_files"){
            if !policy::admitted_tool(run,"read_file",&settings.denied_tools){
                return Err("read_listed_files requires allowed read_file".into());
            }
            let reference=field(raw,"from")?;
            let required_source=if tool=="read_listed_files"{"list_files"}else{"search_files"};
            if previous.get(reference).map(String::as_str)!=Some(required_source){
                return Err(format!("{tool}.from must reference a prior {required_source}"));
            }
            if !deps.iter().any(|d|d==reference){deps.push(reference.to_owned());}
            from=Some(reference.to_string());
            suffix=raw.get("suffix").and_then(Value::as_str).unwrap_or("").to_owned();
            needle=raw.get("contains").and_then(Value::as_str).unwrap_or("").to_owned();
            if suffix.len()>80||needle.len()>256{return Err("iteration filters too long".into());}
            max_files=raw.get("max_files").and_then(Value::as_u64)
                .unwrap_or(4).try_into().map_err(|_|"invalid iteration count")?;
            if !(1..=MAX_ITERATION).contains(&max_files){return Err("max_files must be 1..8".into());}
            if raw.get("arguments").is_some(){
                return Err(format!("{tool} has no arguments"));
            }
        }else{
            if !policy::admitted_tool(run,&tool,&settings.denied_tools){
                return Err(format!("nested tool denied: {tool}"));
            }
            if matches!(tool.as_str(),"git_status"|"git_diff") && run.repository.is_none(){
                return Err("Git inspection requires repository".into());
            }
            let call=ToolCall{id:format!("read-program-admission-{id}"),
                name:tool.clone(),arguments:arguments.clone()};
            tool_schema::admit(&call,&catalog)
                .map_err(|error|format!("step {id}: {}",error.reason))?;
        }
        previous.insert(id.clone(),tool.clone());
        parsed.push(Step{id,tool,arguments,guard,deps,from,
            suffix,needle,max_files,lines_containing:lines_containing.map(str::to_owned)});
    }
    let objective=if let Some(raw)=request.get("objective"){
        let mode=field(raw,"mode")?;
        if !matches!(mode,"all"|"any"){return Err("objective.mode must be all or any".into());}
        let assertions=raw.get("assertions").and_then(Value::as_array)
            .ok_or("objective.assertions must be an array")?;
        if assertions.is_empty()||assertions.len()>8{
            return Err("objective needs 1..8 assertions".into());
        }
        let mut clauses=Vec::new();
        for assertion in assertions {
            let source=field(assertion,"step")?;
            if !previous.contains_key(source){
                return Err("objective references unknown step".into());
            }
            let status=assertion.get("status").and_then(Value::as_str).unwrap_or("ok");
            if !matches!(status,"ok"|"error"){
                return Err("objective status must be ok or error".into());
            }
            let contains=assertion.get("contains").map(|v|
                v.as_str().ok_or("objective.contains must be text")
            ).transpose()?;
            if contains.is_some_and(|s|s.is_empty()||s.len()>256){
                return Err("objective.contains must be 1..256 chars".into());
            }
            clauses.push(Assertion{source:source.into(),
                contains:contains.map(str::to_owned),status:status.into()});
        }
        Some(Objective{mode:mode.into(),assertions:clauses})
    }else{None};
    Ok(Program{steps:parsed,objective})
}
fn clipped(content:&str,contains:Option<&str>)->String{
    let shown=match contains{
        Some(needle)=>content.lines().filter(|line|line.contains(needle))
            .take(24).collect::<Vec<_>>().join("\n"),
        None=>content.to_owned(),
    };
    let count=shown.chars().count();
    if count<=MAX_RECEIPT_CHARS{return shown;}
    let part=shown.chars().take(MAX_RECEIPT_CHARS).collect::<String>();
    format!("{part}\n[{} chars omitted; exact source hash preserved]",count-MAX_RECEIPT_CHARS)
}
async fn read_many(settings:&Settings,run_id:Uuid,list:&str,
    step:&Step)->(Result<String,String>,usize){
    let listed:Vec<String>=if step.tool=="read_matched_files"{
        let hits:Vec<Value>=match serde_json::from_str(list){
            Ok(hits)=>hits,Err(_)=>return (Err("search_files result was not a JSON hit array".into()),0)
        };
        let mut seen=HashSet::new();
        hits.into_iter().filter_map(|hit|hit.get("file").and_then(Value::as_str).map(str::to_owned))
            .filter(|path|seen.insert(path.clone())).collect()
    }else{
        match serde_json::from_str(list){
            Ok(paths)=>paths,Err(_)=>return (Err("list_files result was not a JSON string array".into()),0)
        }
    };
    let mut inspected=0usize;
    let mut results=Vec::new();
    for path in listed.into_iter().filter(|p|p.ends_with(&step.suffix)).take(step.max_files){
        inspected+=1;
        let call=ToolCall{id:format!("read-program:{}:{}",step.id,inspected),
            name:"read_file".into(),arguments:json!({"path":path})};
        let outcome=timeout(Duration::from_secs(30),tools::execute(settings,run_id,&call)).await;
        match outcome{
            Ok(Ok(body))=>{
                if step.needle.is_empty()||body.contains(&step.needle){
                    results.push(json!({"path":path,"text":clipped(&body,step.lines_containing.as_deref()),
                        "sha256":digest(&body),"full_chars":body.chars().count()}));
                }
            }
            Ok(Err(error))=>results.push(json!({"path":path,
                "error":error.chars().take(450).collect::<String>()})),
            Err(_)=>results.push(json!({"path":path,"error":"read timed out"})),
        }
    }
    (Ok(json!({"inspected":inspected,"matches":results}).to_string()),inspected)
}
async fn run_step(settings:Settings,run_id:Uuid,step:Step,from:Option<String>)
    ->(Result<String,String>,usize){
    if matches!(step.tool.as_str(),"read_listed_files"|"read_matched_files"){
        let Some(content)=from else{return (Err("list dependency has no successful result".into()),0)};
        return read_many(&settings,run_id,&content,&step).await;
    }
    let call=ToolCall{id:format!("read-program:{}",step.id),
        name:step.tool,arguments:step.arguments};
    let result=timeout(Duration::from_secs(30),tools::execute(&settings,run_id,&call)).await;
    (match result{
        Ok(output)=>output,
        Err(_)=>Err(format!("nested {} timed out",call.name))
    },1)
}
fn failed(id:&str,kind:&str,reason:&str)->Observation{
    let raw=format!("TOOL ERROR: {reason}");
    Observation{status:kind.into(),raw:raw.clone(),calls:0,
        failures:if kind=="error"{1}else{0},
        receipt:json!({"id":id,"status":kind,"reason":reason})}
}
fn receive(step:&Step,result:Result<String,String>,count:usize,wave:usize)->Observation{
    match result{
        Ok(raw)=>Observation{status:"ok".into(),calls:count,failures:0,
            receipt:json!({"id":step.id,"tool":step.tool,"status":"ok",
                "wave":wave,"sha256":digest(&raw),"full_chars":raw.chars().count(),
                "result":clipped(&raw,step.lines_containing.as_deref())}),raw},
        Err(error)=>{
            let text=error.chars().take(450).collect::<String>();
            Observation{status:"error".into(),calls:count,failures:1,
                raw:format!("TOOL ERROR: {text}"),
                receipt:json!({"id":step.id,"tool":step.tool,"status":"error",
                    "wave":wave,"error":text})}
        }
    }
}
/// Execute independent reads in actual parallel waves; commit their receipts
/// in source-program order regardless of completion timing. Conditions and
/// objective witnesses evaluate only **settled, typed** evidence.
pub async fn execute(settings:&Settings,run:&Run,request:&Value)->Result<String,String>{
    let program=parse(request,run,settings)?;
    let mut observed=HashMap::<String,Observation>::new();
    let mut calls=0usize;
    let mut failures=0usize;
    let mut wave=0usize;
    let max_parallel=settings.max_parallel_reads.clamp(1,MAX_PARALLEL);
    let mut objective_met=false;
    loop{
        if observed.len()==program.steps.len(){break;}
        if let Some(objective)=&program.objective{
            if objective_satisfied(objective,&observed){
                objective_met=true;
                for step in &program.steps{
                    if !observed.contains_key(&step.id){
                        observed.insert(step.id.clone(),
                            failed(&step.id,"skipped","evidence objective already satisfied"));
                    }
                }
                break;
            }
        }
        let mut ready=Vec::new();
        let mut reserved=0usize;
        // A syntactically earlier node does not block unrelated nodes. A
        // dependent node waits for all provenance dependencies to settle.
        for (i,step) in program.steps.iter().enumerate(){
            if observed.contains_key(&step.id) || !step.deps.iter().all(|d|observed.contains_key(d)){
                continue;
            }
            if let Some(source)=&step.from{
                if observed.get(source).is_some_and(|o|o.status!="ok"){
                    observed.insert(step.id.clone(),failed(&step.id,"skipped",
                        "list predecessor did not yield an eligible file set"));
                    continue;
                }
            }
            if let Some(guard)=&step.guard{
                let Some(source)=observed.get(&guard.source) else{
                    return Err("invalid dependency scheduler state".into());
                };
                if !guard_condition(guard,source){
                    observed.insert(step.id.clone(),failed(&step.id,"skipped",
                        "typed observation condition was not satisfied"));
                    continue;
                }
            }
            let worst=if matches!(step.tool.as_str(),"read_listed_files"|"read_matched_files"){
                step.max_files
            }else{1};
            if reserved.saturating_add(worst)>MAX_NESTED_READS.saturating_sub(calls){
                // The interpreter is finite even when a model supplies many
                // list iterations. Do not launch a partial loop silently.
                observed.insert(step.id.clone(),failed(&step.id,"error",
                    "read-program call limit reached before dispatch"));
                failures+=1;
                continue;
            }
            reserved+=worst;
            ready.push(i);
            if ready.len()>=max_parallel{break;}
        }
        if ready.is_empty(){
            if observed.len()==program.steps.len(){break;}
            // Next iteration consumes branches skipped in this wave. With
            // strictly earlier dependencies it cannot stall indefinitely.
            if program.steps.iter().any(|s|!observed.contains_key(&s.id)
                &&s.deps.iter().all(|d|observed.contains_key(d))){
                continue;
            }
            return Err("read_program dependency graph could not settle".into());
        }
        wave+=1;
        let mut tasks=JoinSet::new();
        for index in &ready{
            let step=program.steps[*index].clone();
            let from=step.from.as_ref()
                .and_then(|ref_id|observed.get(ref_id)).map(|o|o.raw.clone());
            let settings=settings.clone();
            let index=*index;
            let id=run.id;
            tasks.spawn(async move{(index,run_step(settings,id,step,from).await)});
        }
        let mut results=HashMap::new();
        while let Some(joined)=tasks.join_next().await{
            match joined{
                Ok((index,result))=>{results.insert(index,result);},
                Err(error)=>return Err(format!("nested pure-read worker failed: {error}")),
            }
        }
        // A batch is a deterministic observation boundary. The objective is
        // checked only after all launched concurrent reads have settled.
        for index in ready {
            let step=&program.steps[index];
            let (result,count)=results.remove(&index)
                .ok_or("missing read-program execution receipt")?;
            let receipt=receive(step,result,count,wave);
            calls+=receipt.calls;
            failures+=receipt.failures;
            observed.insert(step.id.clone(),receipt);
        }
    }
    let mut receipts=Vec::new();
    for step in &program.steps{
        receipts.push(observed.get(&step.id).ok_or("missing settled observation")?.receipt.clone());
    }
    let witnesses=program.objective.as_ref().map(|obj|{
        obj.assertions.iter().filter_map(|assertion|{
            let observation=observed.get(&assertion.source)?;
            if observation.status!=assertion.status || !assertion.contains.as_ref()
                .is_none_or(|needle|observation.raw.contains(needle)){
                return None;
            }
            Some(json!({"step":assertion.source,"status":assertion.status,
                "sha256":digest(&observation.raw),"contains":assertion.contains}))
        }).collect::<Vec<_>>()
    }).unwrap_or_default();
    let output=json!({"kind":"evidence_program","model_calls_inside":0,
        "execution_waves":wave,"nested_read_calls":calls,"failed_steps":failures,
        "objective_status":if program.objective.is_none(){"not_requested"}
            else if objective_met{"satisfied"}else{"not_satisfied"},
        "objective_witnesses":witnesses,"steps":receipts});
    if output.to_string().chars().count()>30_000{
        return Err("read_program result too large; narrow file inspection".into());
    }
    Ok(output.to_string())
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::CreateRun;
    fn example()->Run{
        Run::new(CreateRun{instruction:"inspect".into(),
            budget_krw:Some(50.0),max_steps:Some(4)},4).unwrap()
    }
    #[tokio::test]
    async fn typed_branch_dataflow_and_parallel_waves_produce_evidence(){
        let mut settings=Settings::from_env().unwrap();
        let root=std::env::temp_dir().join(format!("dominion-evidence-program-{}",Uuid::new_v4()));
        settings.workspace_dir=root.clone();
        let run=example();
        let dir=root.join(run.id.to_string());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.rs"),"first signal=present\n").await.unwrap();
        tokio::fs::write(dir.join("b.rs"),"second signal=present\n").await.unwrap();
        let output=execute(&settings,&run,&json!({"steps":[
            {"id":"files","tool":"list_files","arguments":{"path":""}},
            {"id":"peer","tool":"read_file","arguments":{"path":"a.rs"}},
            {"id":"found","tool":"read_listed_files","from":"files","suffix":".rs",
                "contains":"signal","max_files":2},
            {"id":"skipped","tool":"read_file","arguments":{"path":"b.rs"},
                "when":{"step":"peer","contains":"not-the-signal"}},
            {"id":"target","tool":"read_file","arguments":{"path":"b.rs"},
                "when":{"step":"files","contains":"b.rs"},"lines_containing":"signal"}
        ]})).await.unwrap();
        let report:Value=serde_json::from_str(&output).unwrap();
        assert_eq!(report["model_calls_inside"],0);
        assert_eq!(report["nested_read_calls"],5);
        assert_eq!(report["steps"][0]["wave"],report["steps"][1]["wave"]);
        assert_eq!(report["steps"][3]["status"],"skipped");
        assert_eq!(report["steps"][4]["status"],"ok");
        assert_eq!(report["steps"][4]["sha256"],digest("second signal=present\n"));
        assert_eq!(report["steps"][2]["status"],"ok");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn search_results_drive_unique_file_reads_without_a_second_model_turn(){
        let mut settings=Settings::from_env().unwrap();
        let root=std::env::temp_dir().join(format!("dominion-search-dataflow-{}",Uuid::new_v4()));
        settings.workspace_dir=root.clone();
        let run=example();
        let dir=root.join(run.id.to_string());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("first.rs"),"TODO alpha\nTODO beta\n").await.unwrap();
        tokio::fs::write(dir.join("second.rs"),"TODO gamma\n").await.unwrap();
        tokio::fs::write(dir.join("ignored.rs"),"not relevant\n").await.unwrap();
        let result=execute(&settings,&run,&json!({"steps":[
            {"id":"hits","tool":"search_files","arguments":{"query":"TODO"}},
            {"id":"sources","tool":"read_matched_files","from":"hits",
                "suffix":".rs","max_files":3,"contains":"TODO"}
        ]})).await.unwrap();
        let r:Value=serde_json::from_str(&result).unwrap();
        assert_eq!(r["nested_read_calls"],3,
            "one search and two unique matched source files, not one read per hit");
        assert_eq!(r["steps"][1]["status"],"ok");
        let nested=r["steps"][1]["result"].as_str().unwrap();
        assert!(nested.contains("first.rs")&&nested.contains("second.rs"));
        assert!(!nested.contains("ignored.rs"));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn objective_short_circuits_only_after_grounded_witness(){
        let mut settings=Settings::from_env().unwrap();
        let root=std::env::temp_dir().join(format!("dominion-evidence-short-{}",Uuid::new_v4()));
        settings.workspace_dir=root.clone();
        let run=example();
        let dir=root.join(run.id.to_string());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.txt"),"needle\n").await.unwrap();
        tokio::fs::write(dir.join("b.txt"),"do not inspect\n").await.unwrap();
        let result=execute(&settings,&run,&json!({
            "objective":{"mode":"all","assertions":[
                {"step":"read","status":"ok","contains":"needle"}]},
            "steps":[
                {"id":"read","tool":"read_file","arguments":{"path":"a.txt"}},
                {"id":"next","tool":"read_file","arguments":{"path":"b.txt"},"after":["read"]}
            ]
        })).await.unwrap();
        let v:Value=serde_json::from_str(&result).unwrap();
        assert_eq!(v["objective_status"],"satisfied");
        assert_eq!(v["nested_read_calls"],1);
        assert_eq!(v["steps"][1]["status"],"skipped");
        assert_eq!(v["objective_witnesses"][0]["sha256"],digest("needle\n"));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn failure_is_typed_not_mistaken_for_positive_content(){
        let mut settings=Settings::from_env().unwrap();
        let root=std::env::temp_dir().join(format!("dominion-evidence-error-{}",Uuid::new_v4()));
        settings.workspace_dir=root.clone();
        let run=example();
        let dir=root.join(run.id.to_string());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("real.txt"),"recovered\n").await.unwrap();
        let result=execute(&settings,&run,&json!({"steps":[
            {"id":"missing","tool":"read_file","arguments":{"path":"not-found"}},
            {"id":"recover","tool":"read_file","arguments":{"path":"real.txt"},
                "when":{"step":"missing","status":"error"}},
            {"id":"must_skip","tool":"read_file","arguments":{"path":"real.txt"},
                "when":{"step":"missing","contains":"not-found"}}
        ]})).await.unwrap();
        let r:Value=serde_json::from_str(&result).unwrap();
        assert_eq!(r["steps"][0]["status"],"error");
        assert_eq!(r["steps"][1]["status"],"ok");
        assert_eq!(r["steps"][2]["status"],"skipped");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn prohibited_nested_side_effects_fail_before_any_dispatch(){
        let settings=Settings::from_env().unwrap();
        let run=example();
        for tool in ["shell","write_file","memory_store","compare_strategies","mcp__remote"]{
            assert!(execute(&settings,&run,&json!({"steps":[
                {"id":"attempt","tool":tool,"arguments":{"path":"something"}}]
            })).await.is_err());
        }
        let mut denied=settings.clone();
        denied.denied_tools.push("read_file".into());
        assert!(execute(&denied,&run,&json!({"steps":[
            {"id":"a","tool":"list_files","arguments":{"path":""}},
            {"id":"b","tool":"read_listed_files","from":"a","max_files":2}
        ]})).await.is_err());
    }
    #[test]
    fn negated_evidence_guard_never_promotes_failed_io_into_success(){
        let observation=failed("x","error","ENOENT");
        let guard=Guard{source:"x".into(),status:"ok".into(),
            contains:Some("missing-marker".into()),negate:true};
        assert!(!guard_condition(&guard,&observation),
            "a failed read cannot satisfy a negative success condition");
        let error=Guard{status:"error".into(),negate:false,..guard};
        assert!(guard_condition(&error,&observation)==false);
    }
    #[test]
    fn topological_and_budget_guards_are_checked_before_dispatch(){
        let settings=Settings::from_env().unwrap();
        let run=example();
        assert!(parse(&json!({"steps":[
            {"id":"a","tool":"read_file","arguments":{"path":"x"},"after":["b"]},
            {"id":"b","tool":"list_files","arguments":{"path":""}}
        ]}),&run,&settings).is_err());
        assert!(parse(&json!({"steps":[
            {"id":"files","tool":"list_files","arguments":{"path":""}},
            {"id":"scan","tool":"read_listed_files","from":"files","max_files":1000}
        ]}),&run,&settings).is_err());
    }
    #[test]
    fn whole_program_journal_replays_without_duplicate_write_effects(){
        let mut run=example();
        run.status=crate::domain::RunStatus::Running;
        let call=ToolCall{id:"program".into(),name:"read_program".into(),
            arguments:json!({"steps":[{"id":"files","tool":"list_files","arguments":{"path":""}}]})};
        run.queued_calls.push(call.clone());
        crate::journal::begin(&mut run,&call).unwrap();
        assert!(crate::journal::recover(&mut run));
        assert!(run.in_flight.is_none());
        assert_eq!(run.queued_calls.len(),1);
    }
}
