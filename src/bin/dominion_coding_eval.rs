//! Opt-in live-model coding benchmark. Never assigns a success grade from CI mocks.
//! Run identical manifests/model budgets against each branch for an honest comparison.
use dominion::{config::Settings,domain::{CreateRun,RunStatus},sdk::Dominion};
use serde::{Deserialize,Serialize};
use std::{collections::HashSet,env,path::{Component,Path,PathBuf},process::Stdio,time::Instant};
use tokio::{process::Command,time::{timeout,Duration}};

#[derive(Deserialize)]
struct Manifest{cases:Vec<Case>}
#[derive(Deserialize)]
struct Case{
    id:String,repository:String,instruction:String,expected_files:Vec<String>,
    verify:Vec<Vec<String>>,
    budget_krw:Option<f64>,max_steps:Option<usize>,timeout_seconds:Option<u64>,
}
#[derive(Serialize)]
struct Check{command:Vec<String>,passed:bool,output:String}
#[derive(Serialize)]
struct ResultRow{
    id:String,run_id:Option<String>,status:String,passed:bool,reason:String,
    expected_files:Vec<String>,changed_files:Vec<String>,checks:Vec<Check>,
    tool_approvals:usize,input_tokens:u64,output_tokens:u64,cached_input_tokens:u64,
    reported_cost_krw:f64,cost_priced:bool,elapsed_ms:u128,
}
#[derive(Serialize)]
struct Report{version:u32,model:String,total:usize,passed:usize,results:Vec<ResultRow>}
fn validate_relative(path:&str)->Result<(),String>{
    if path.is_empty()||path.len()>512||Path::new(path).components()
        .any(|part|!matches!(part,Component::Normal(_))){
        return Err(format!("unsafe relative path: {path}"));
    }
    Ok(())
}
fn validate(case:&Case)->Result<(),String>{
    if case.id.trim().is_empty()||case.id.len()>80{return Err("invalid case ID".into());}
    validate_relative(&case.repository)?;
    for path in &case.expected_files{validate_relative(path)?;}
    if case.expected_files.is_empty(){return Err("expected_files must name actual artifacts".into());}
    if case.instruction.trim().is_empty()||case.instruction.len()>20_000{
        return Err("invalid coding task instruction".into());
    }
    if case.verify.is_empty()||case.verify.len()>8||case.verify.iter().any(|v|
        v.is_empty()||v.len()>20||v.iter().any(|s|s.is_empty()||s.len()>512)
    ){return Err("at least one explicit executable verification argv is required".into());}
    if case.timeout_seconds.is_some_and(|n|!(30..=3600).contains(&n)){
        return Err("timeout_seconds must be between 30 and 3600".into());
    }
    if case.max_steps.is_some_and(|n|!(1..=100).contains(&n)){
        return Err("max_steps must be between 1 and 100".into());
    }
    if case.budget_krw.is_some_and(|n|!n.is_finite()||!(0.0..=1_000_000.0).contains(&n)){
        return Err("invalid budget_krw".into());
    }
    Ok(())
}
async fn execute(dir:&Path,argv:&[String],seconds:u64)->Result<Check,String>{
    let result=timeout(Duration::from_secs(seconds),Command::new(&argv[0])
        .args(&argv[1..]).current_dir(dir)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).output())
        .await.map_err(|_|format!("verification timed out: {}",argv[0]))?
        .map_err(|e|format!("verification cannot start {}: {e}",argv[0]))?;
    let output=String::from_utf8_lossy(&result.stdout).chars().take(1200).collect::<String>()
        + &String::from_utf8_lossy(&result.stderr).chars().take(1200).collect::<String>();
    Ok(Check{command:argv.to_vec(),passed:result.status.success(),output})
}
async fn git_changes(worktree:&Path)->Result<Vec<String>,String>{
    let mut result=HashSet::new();
    for command in [["git","diff","--name-only","HEAD","--"].as_slice(),
                    ["git","ls-files","--others","--exclude-standard"].as_slice()]{
        let args=command.iter().map(|s|(*s).to_owned()).collect::<Vec<_>>();
        let receipt=execute(worktree,&args,20).await?;
        if !receipt.passed{return Err(format!("git inspection failed: {}",receipt.output));}
        for path in receipt.output.lines().filter(|s|!s.is_empty()){result.insert(path.to_owned());}
    }
    let mut paths=result.into_iter().collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}
async fn evaluate(sdk:&Dominion,settings:&Settings,case:&Case,permitted:&HashSet<String>)
    ->Result<ResultRow,String>{
    validate(case)?;
    let begin=Instant::now();
    let run=sdk.submit_from_repository(CreateRun{
        instruction:case.instruction.clone(),budget_krw:case.budget_krw,
        max_steps:case.max_steps
    },&case.repository).await?;
    let id=run.id;
    let worktree=settings.workspace_dir.join(id.to_string());
    let deadline=Duration::from_secs(case.timeout_seconds.unwrap_or(600));
    let mut approvals=0usize;
    let (run,reason)=loop{
        let Some(remaining)=deadline.checked_sub(begin.elapsed()) else{
            break (sdk.get(id).await?,"run exceeded deadline".to_string());
        };
        match sdk.wait(id,remaining).await{
            Ok(state) if state.status==RunStatus::AwaitingApproval=>{
                let Some(call)=state.pending_approval.as_ref()else{
                    break (state,"approval state missing a tool call".into());
                };
                if !permitted.contains(&call.name){
                    let name=call.name.clone();
                    break (state,format!("approval required for {name}; explicitly pass --approve-tool {name}"));
                }
                if approvals>=50{break (state,"approval budget of 50 calls exhausted".into());}
                sdk.approve(id,true).await?;
                approvals+=1;
            },
            Ok(state)=>break (state,String::new()),
            Err(err)=>break (sdk.get(id).await?,err),
        }
    };
    let status=serde_json::to_value(&run.status).map_err(|e|e.to_string())?
        .as_str().unwrap_or("unknown").to_string();
    let changed=git_changes(&worktree).await?;
    let missing=case.expected_files.iter().filter(|path|!changed.contains(path))
        .cloned().collect::<Vec<_>>();
    let mut checks=Vec::new();
    if run.status==RunStatus::Completed && missing.is_empty(){
        for args in &case.verify{
            // Execute manifest-owned direct argv, never a generated shell string.
            let check=execute(&worktree,args,120).await?;
            let passed=check.passed;
            checks.push(check);
            if !passed{break;}
        }
    }
    let passed=run.status==RunStatus::Completed&&missing.is_empty()
        &&checks.len()==case.verify.len()&&checks.iter().all(|c|c.passed);
    let reason=if !reason.is_empty(){reason}
        else if !missing.is_empty(){format!("missing changed files: {}",missing.join(", "))}
        else if checks.iter().any(|c|!c.passed){"artifact verifier failed".into()}
        else if run.status!=RunStatus::Completed{format!("run finished in {status} state")}
        else{String::new()};
    Ok(ResultRow{
        id:case.id.clone(),run_id:Some(id.to_string()),status,passed,reason,
        expected_files:case.expected_files.clone(),changed_files:changed,checks,
        tool_approvals:approvals,input_tokens:run.input_tokens,output_tokens:run.output_tokens,
        cached_input_tokens:run.cached_input_tokens,reported_cost_krw:run.cost_krw,
        cost_priced:settings.main.input_krw_per_million>0.0
            &&settings.main.output_krw_per_million>0.0,elapsed_ms:begin.elapsed().as_millis()
    })
}
async fn run()->Result<(),String>{
    let mut args=env::args().skip(1);
    let mut manifest=None;
    let mut output=PathBuf::from("benchmark-results/sub4-coding.json");
    let mut permitted=HashSet::new();
    while let Some(option)=args.next(){
        match option.as_str(){
            "--manifest"=>manifest=Some(PathBuf::from(args.next().ok_or("--manifest needs a path")?)),
            "--output"=>output=PathBuf::from(args.next().ok_or("--output needs a path")?),
            "--approve-tool"=>{permitted.insert(args.next().ok_or("--approve-tool needs a name")?);},
            _=>return Err(format!("unknown argument: {option}")),
        }
    }
    let manifest=manifest.ok_or("usage: dominion_coding_eval --manifest tasks.json [--output results.json] [--approve-tool write_file]")?;
    let data=tokio::fs::read_to_string(manifest).await.map_err(|e|e.to_string())?;
    let manifest:Manifest=serde_json::from_str(&data).map_err(|e|e.to_string())?;
    if manifest.cases.is_empty()||manifest.cases.len()>20{
        return Err("manifest needs 1..20 tasks".into());
    }
    let mut ids=HashSet::new();
    for case in &manifest.cases{
        validate(case)?;
        if !ids.insert(&case.id){return Err(format!("duplicate case ID: {}",case.id));}
    }
    let settings=Settings::from_env()?;
    if settings.main.api_key.is_empty()
        &&!settings.main.base_url.contains("localhost")
        &&!settings.main.base_url.contains("127.0.0.1"){
        return Err("no live provider credentials: configure DOMINION_API_KEY or local model endpoint".into());
    }
    let sdk=Dominion::open(settings.clone()).await?;
    let mut results=Vec::new();
    for case in &manifest.cases{
        eprintln!("Running: {}",case.id);
        match evaluate(&sdk,&settings,case,&permitted).await{
            Ok(result)=>results.push(result),
            Err(error)=>results.push(ResultRow{
                id:case.id.clone(),run_id:None,status:"error".into(),passed:false,
                reason:error,expected_files:case.expected_files.clone(),
                changed_files:vec![],checks:vec![],tool_approvals:0,
                input_tokens:0,output_tokens:0,cached_input_tokens:0,
                reported_cost_krw:0.0,cost_priced:false,elapsed_ms:0
            }),
        }
    }
    let successes=results.iter().filter(|r|r.passed).count();
    let report=Report{version:1,model:settings.main.name,total:results.len(),passed:successes,results};
    if let Some(parent)=output.parent(){tokio::fs::create_dir_all(parent).await.map_err(|e|e.to_string())?;}
    tokio::fs::write(&output,serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?)
        .await.map_err(|e|e.to_string())?;
    println!("coding verification: {}/{} passed; evidence: {}",report.passed,report.total,output.display());
    if report.passed!=report.total{return Err(format!("{} coding tasks did not pass",report.total-report.passed));}
    Ok(())
}
#[tokio::main]
async fn main(){
    if let Err(error)=run().await{eprintln!("coding evaluation: {error}");std::process::exit(1);}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_cannot_pass_without_external_verification_and_real_diff(){
        let task=Case{id:"case-a".into(),repository:"projects/demo".into(),
            instruction:"change two files".into(),expected_files:vec!["src/a.rs".into()],
            verify:vec![],budget_krw:Some(25.0),max_steps:Some(12),timeout_seconds:Some(60)};
        assert!(validate(&task).is_err());
        assert!(validate_relative("../escape").is_err());
        assert!(validate_relative("/tmp/repo").is_err());
        let task=Case{verify:vec![vec!["cargo".into(),"test".into()]],..task};
        assert!(validate(&task).is_ok());
    }
}
