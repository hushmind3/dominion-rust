//! Opt-in semantic recall of a REAL 100-owner-turn, two-speaker Room.
//! No mock benchmark is ever promoted into a model-accuracy claim.
//! Tool-free participants use the existing cache-aware Room provider lane.
//! Usage comes from actual provider/Room receipts, not token estimates.
use dominion::{config::Settings,group::{FrontendRoom,NewRoom,Participant},
    sdk::Dominion};
use serde::Serialize;
use serde_json::{json,Value};
use std::{env,path::PathBuf,time::Instant};
use uuid::Uuid;

const TURNS:usize=100;
const EVALUATOR_VERSION:u32=1;
#[derive(Debug)]
struct Options{
    budget_krw:f64,
    context_budget_tokens:usize,
    output:PathBuf,
    allow_unpriced:bool,
    confirmed:bool,
}
fn options()->Result<Options,String>{
    let mut output=PathBuf::from("benchmark-results/sub4-room-recall.json");
    let mut budget=None;
    let mut context_budget_tokens=4000usize;
    let mut allow_unpriced=false;
    let mut confirmed=false;
    let mut args=env::args().skip(1);
    while let Some(opt)=args.next(){
        match opt.as_str(){
            "--confirm-live-model"=>confirmed=true,
            "--allow-unpriced"=>allow_unpriced=true,
            "--budget-krw"=>{
                budget=Some(args.next().ok_or("--budget-krw needs a value")?
                    .parse::<f64>().map_err(|_|"invalid KRW budget")?);
            }
            "--context-budget-tokens"=>{
                context_budget_tokens=args.next().ok_or("--context-budget-tokens needs a value")?
                    .parse::<usize>().map_err(|_|"invalid context budget")?;
            }
            "--output"=>output=PathBuf::from(args.next().ok_or("--output needs a path")?),
            "--help"=>return Err("usage: dominion_room_recall_eval --confirm-live-model --budget-krw 5000 [--context-budget-tokens 4000] [--output report.json] [--allow-unpriced]".into()),
            _=>return Err(format!("unknown argument: {opt}"))
        }
    }
    if !confirmed{return Err("live-model benchmark requires --confirm-live-model".into());}
    let budget_krw=budget.ok_or("--budget-krw is required (real provider calls may incur charges)")?;
    if !budget_krw.is_finite()||!(1.0..=1_000_000.0).contains(&budget_krw){
        return Err("KRW budget must be between 1 and 1,000,000".into());
    }
    if !(200..=8000).contains(&context_budget_tokens){
        return Err("context budget must be 200..8000 tokens".into());
    }
    Ok(Options{budget_krw,context_budget_tokens,output,allow_unpriced,confirmed})
}
#[derive(Debug,Clone,PartialEq,Eq)]
struct Verdict{early_fact:bool,latest_correction:bool,valid_json:bool}
fn grade(answer:&str,anchor:&str,current:&str)->Verdict{
    // Exact values only. A commentary, asserted PASS, stale old value,
    // or invalid JSON never counts as a semantic recall success.
    let parsed=serde_json::from_str::<Value>(answer.trim()).ok();
    Verdict{
        valid_json:parsed.as_ref().is_some_and(Value::is_object),
        early_fact:parsed.as_ref().and_then(|v|v["early_fact"].as_str())==Some(anchor),
        latest_correction:parsed.as_ref()
            .and_then(|v|v["current_target"].as_str())==Some(current),
    }
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct Report{
    schema_version:u32,benchmark:String,live_model_calls:bool,
    model:String,room_id:String,owner_turns_requested:usize,
    completed_owner_turns:usize,final_probe_completed:bool,
    source_messages:usize,accepted_agent_responses:usize,
    early_fact_correct:bool,latest_correction_correct:bool,
    valid_json:bool,passed:bool,
    prompt_tokens:u64,completion_tokens:u64,cached_prompt_tokens:u64,
    cache_write_tokens:u64,cost_krw:f64,pricing_configured:bool,
    provider_usage_fully_reported:bool,provider_cache_usage_fully_reported:bool,
    elapsed_ms:u128,failure:Option<String>,
    final_answer:Option<String>,source_scope:String,
}
async fn evaluate(opts:Options)->Result<Report,String>{
    let settings=Settings::from_env()?;
    if settings.main.api_key.is_empty()
        && !settings.main.base_url.starts_with("http://127.0.0.1")
        && !settings.main.base_url.starts_with("http://localhost")
        && !settings.main.base_url.starts_with("http://[::1]"){
        return Err("no live model credentials: configure DOMINION_API_KEY or a local endpoint".into());
    }
    let price_known=settings.main.input_krw_per_million>0.0
        &&settings.main.output_krw_per_million>0.0;
    if !price_known&&!opts.allow_unpriced{
        return Err("model KRW tariff is missing; configure DOMINION_INPUT_KRW_PER_MTOK and DOMINION_OUTPUT_KRW_PER_MTOK or explicitly --allow-unpriced".into());
    }
    let sdk=Dominion::open(settings.clone()).await?;
    let start=Instant::now();
    let participants=vec![
        Participant{name:"analyst".into(),role:"Review design assumptions, do not rewrite quoted owner facts".into(),economy:false},
        Participant{name:"reviewer".into(),role:"Review design verification, respect newest owner corrections".into(),economy:false}
    ];
    let room=sdk.create_group(NewRoom{
        topic:"Semantic continuity of a 100-owner-turn project discussion".into(),
        participants:participants.clone(),max_rounds:Some(TURNS+1),
        budget_krw:Some(opts.budget_krw)
    }).await?;
    let view=FrontendRoom{
        name:"100-turn semantic recall eval".into(),
        topic:room.topic.clone(),owner_name:"owner".into(),
        agent_ids:participants.iter().map(|p|p.name.clone()).collect(),
        routing_mode:"round_robin".into(),rounds:1,max_turns:1,
        lifetime_max_responses:TURNS+1,token_budget:0,
        context_budget_tokens:opts.context_budget_tokens,
        tools_enabled:false,auto_learning:false,auto_models:true,
        models:Vec::new(),reasoning_effort:String::new(),learned_context:String::new()
    };
    sdk.engine().groups.configure(room.id,view,participants).await?;
    let anchor=format!("EARLY-{}",Uuid::new_v4().simple());
    let old_target=format!("OLD-{}",Uuid::new_v4().simple());
    let current_target=format!("NEW-{}",Uuid::new_v4().simple());
    let mut completed=0usize;
    let mut failure=None::<String>;
    for i in 0..TURNS{
        let prompt=match i{
            0=>format!("Authoritative owner fact: the permanent project audit ID is {anchor}. This ID will never change. The initial deploy target is {old_target}. Keep both facts available to later speakers."),
            88=>format!("Authoritative owner correction: the deploy target has CHANGED from {old_target} to {current_target}. The OLD target is invalid. The permanent audit ID stays the same."),
            _=>format!("Owner discussion message {} of {}: compare the reliability and maintenance implications of an event-driven multi-agent coordination system in ONE brief sentence. Do not reveal project IDs unless explicitly asked.",i+1,TURNS)
        };
        let sent=sdk.say_group(room.id,dominion::group::Say{content:prompt}).await;
        if let Err(error)=sent {failure=Some(format!("owner turn {}: {error}",i+1));break;}
        match sdk.group_turn(room.id).await{
            Ok(updated) if updated.messages.last().is_some_and(|m|m.speaker!="user")=>{
                completed+=1;
            },
            Ok(_)=>{failure=Some(format!("turn {} did not produce an accepted speaker answer",i+1));break;},
            Err(error)=>{failure=Some(format!("turn {}: {error}",i+1));break;}
        }
    }
    let mut final_answer=None;
    let mut final_probe_completed=false;
    if failure.is_none()&&completed==TURNS{
        let question="Final semantic recall evaluation. Return ONLY a valid JSON object with exactly two string fields named early_fact and current_target. early_fact must contain the permanent audit ID from the earliest owner instruction. current_target must contain the latest CORRECTED deploy target, never the obsolete target. No markdown, explanation or invented values.";
        sdk.say_group(room.id,dominion::group::Say{content:question.into()}).await?;
        match sdk.group_turn(room.id).await{
            Ok(updated)=>{
                final_answer=updated.messages.last()
                    .filter(|m|m.speaker!="user").map(|m|m.content.clone());
                final_probe_completed=final_answer.is_some();
                if !final_probe_completed{
                    failure=Some("final semantic probe yielded no accepted answer".into());
                }
            },
            Err(error)=>failure=Some(format!("final probe: {error}"))
        }
    }
    let snapshot=sdk.engine().groups.get(room.id).await.ok_or("evaluation Room disappeared")?;
    let verdict=grade(final_answer.as_deref().unwrap_or(""),&anchor,&current_target);
    let prompt_tokens=snapshot.usage_receipts.iter().map(|r|r.input_tokens).sum();
    let completion_tokens=snapshot.usage_receipts.iter().map(|r|r.output_tokens).sum();
    let cached_prompt_tokens=snapshot.usage_receipts.iter().map(|r|r.cached_input_tokens).sum();
    let cache_write_tokens=snapshot.usage_receipts.iter().map(|r|r.cache_write_tokens).sum();
    let accepted_agent_responses=snapshot.usage_receipts.iter()
        .filter(|r|r.accepted).count();
    let metered=!snapshot.usage_receipts.is_empty()
        &&snapshot.usage_receipts.iter().all(|r|r.provider_usage_reported==Some(true));
    let cache_metered=!snapshot.usage_receipts.is_empty()
        &&snapshot.usage_receipts.iter().all(|r|r.cache_usage_reported==Some(true));
    let output=Report{
        schema_version:EVALUATOR_VERSION,
        benchmark:"sub4_real_room_semantic_recall".into(),
        live_model_calls:true,model:settings.main.name,
        room_id:room.id.to_string(),owner_turns_requested:TURNS,
        completed_owner_turns:completed,final_probe_completed,
        source_messages:snapshot.messages.len(),accepted_agent_responses,
        early_fact_correct:verdict.early_fact,
        latest_correction_correct:verdict.latest_correction,
        valid_json:verdict.valid_json,
        passed:completed==TURNS&&final_probe_completed&&verdict.valid_json
            &&verdict.early_fact&&verdict.latest_correction&&failure.is_none(),
        prompt_tokens,completion_tokens,cached_prompt_tokens,
        cache_write_tokens,cost_krw:snapshot.cost_krw,
        pricing_configured:price_known,
        provider_usage_fully_reported:metered,
        provider_cache_usage_fully_reported:cache_metered,
        elapsed_ms:start.elapsed().as_millis(),
        failure,final_answer,source_scope:"isolated persistent Room JSON and actual provider receipts".into()
    };
    if let Some(parent)=opts.output.parent(){
        tokio::fs::create_dir_all(parent).await.map_err(|e|e.to_string())?;
    }
    tokio::fs::write(&opts.output,serde_json::to_vec_pretty(&output)
        .map_err(|e|e.to_string())?).await.map_err(|e|e.to_string())?;
    Ok(output)
}
#[tokio::main]
async fn main(){
    match options().and_then(|options|{
        // Captured opt-in belongs to this process only, never persisted as
        // blanket authorization for agents or connected tool permissions.
        if !options.confirmed{return Err("explicit opt-in required".into());}
        Ok(options)
    }){
        Ok(options)=>match evaluate(options).await{
            Ok(report)=>{
                println!("100-turn live semantic recall: {}/2 facts correct; completed {}/100 owner turns; cost {:.4} KRW; metering {}",
                    usize::from(report.early_fact_correct)+usize::from(report.latest_correction_correct),
                    report.completed_owner_turns,report.cost_krw,
                    if report.pricing_configured{"configured"}else{"unpriced"});
                if !report.passed{std::process::exit(1);}
            },
            Err(error)=>{eprintln!("room recall evaluation error: {error}");std::process::exit(1);}
        },
        Err(error)=>{eprintln!("{error}");std::process::exit(2);}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn exact_semantic_fact_grading_rejects_stale_or_imagined_values(){
        let correct=r#"{"early_fact":"EARLY-1","current_target":"NEW-1"}"#;
        let good=grade(correct,"EARLY-1","NEW-1");
        assert!(good.valid_json&&good.early_fact&&good.latest_correction);
        let stale=grade(r#"{"early_fact":"EARLY-1","current_target":"OLD-1"}"#,
            "EARLY-1","NEW-1");
        assert!(stale.early_fact&&!stale.latest_correction);
        let claims=grade("PASS: remembered everything","EARLY-1","NEW-1");
        assert!(!claims.valid_json&&!claims.early_fact&&!claims.latest_correction);
        let hallucinated=grade(r#"{"early_fact":"EARLY-guessed","current_target":"NEW-1"}"#,
            "EARLY-1","NEW-1");
        assert!(!hallucinated.early_fact&&hallucinated.latest_correction);
    }
}
