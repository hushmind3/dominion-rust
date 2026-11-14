//! Native Rust task runner for official SWE-bench Verified instances.
 //! Uses only the production DOMINION HTTP goal pipeline. The Python
 //! SWE-bench package is used exclusively as the external Docker oracle.
 //! No gold patches, hidden tests or reference solutions enter model context.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{env,fs,io::{BufRead,BufReader,Write},path::{Path,PathBuf},
    process::Stdio,time::{Duration,Instant}};
use tokio::{process::{Child,Command},time::{sleep,timeout}};

const DATASET:&str="princeton-nlp/SWE-bench_Verified";
const MAX_PATCH:usize=2_000_000;

#[derive(Clone,Debug)]
struct Options {
    tasks:PathBuf,
    output:PathBuf,
    engine:PathBuf,
    provider_url:String,
    provider_kind:String,
    provider_model:String,
    provider_key_env:Option<String>,
    limit:usize,
    goal_timeout:Duration,
    token_budget:u64,
    max_iterations:u64,
    evaluate:bool,
}
fn parse_args(args:&[String])->Result<Options,String>{
    let mut values=std::collections::BTreeMap::new();
    let mut flags=std::collections::BTreeSet::new();
    let mut index=0;
    while index<args.len(){
        let key=&args[index];
        if !key.starts_with("--"){return Err(format!("unexpected argument {key}"))}
        if key=="--evaluate"{flags.insert(key.clone());index+=1;continue;}
        let Some(value)=args.get(index+1) else{return Err(format!("missing {key} value"))};
        if !value.starts_with("--"){values.insert(key.clone(),value.clone());index+=2;}
        else{return Err(format!("missing {key} value"));}
    }
    let required=|key:&str|values.get(key).cloned()
        .ok_or_else(||format!("{key} is required"));
    let positive=|key:&str,default:u64,min:u64,max:u64|->Result<u64,String>{
        let n=values.get(key).map(String::as_str).unwrap_or("")
            .parse::<u64>().unwrap_or(default);
        if !(min..=max).contains(&n){return Err(format!("{key} must be {min}..{max}"))}
        Ok(n)
    };
    let kind=values.get("--provider-kind").map(String::as_str).unwrap_or("openai");
    if !matches!(kind,"openai"|"anthropic"|"ollama"|"codex"){return Err("invalid provider kind".into());}
    let url=required("--provider-url")?;
    let uri=reqwest::Url::parse(&url).map_err(|e|e.to_string())?;
    if !matches!(uri.scheme(),"http"|"https"){return Err("provider must use http(s)".into());}
    let model=required("--model")?;
    if model.trim().is_empty(){return Err("model must not be blank".into());}
    let engine=PathBuf::from(required("--engine")?);
    if !engine.is_file(){return Err(format!("DOMINION Rust engine not found: {}",engine.display()));}
    let tasks=PathBuf::from(required("--tasks")?);
    if !tasks.is_file(){return Err(format!("official task JSONL not found: {}",tasks.display()));}
    let output=PathBuf::from(required("--output")?);
    if output.exists(){return Err("output directory must be new; never mix evaluation receipts".into());}
    Ok(Options{
        tasks,output,engine,
        provider_url:url,
        provider_kind:kind.into(),
        provider_model:model,
        provider_key_env:values.get("--provider-key-env").cloned(),
        limit:positive("--limit",5,1,500)? as usize,
        goal_timeout:Duration::from_secs(positive("--timeout-seconds",900,60,7200)?),
        token_budget:positive("--token-budget",150_000,1000,2_000_000)?,
        max_iterations:positive("--iterations",16,1,64)?,
        evaluate:flags.contains("--evaluate"),
    })
}
#[derive(Clone,Debug)]
struct Task{
    id:String,
    repo:String,
    commit:String,
    problem:String,
}
fn safe_id(text:&str)->bool{
    !text.is_empty()&&text.len()<180
        &&text.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'|b'.'))
        &&!text.contains("..")
}
fn load_tasks(path:&Path,limit:usize)->Result<Vec<Task>,String>{
    let reader=BufReader::new(fs::File::open(path).map_err(|e|e.to_string())?);
    let mut tasks=Vec::new();
    let mut seen=std::collections::BTreeSet::new();
    for line in reader.lines(){
        let line=line.map_err(|e|e.to_string())?;
        if line.trim().is_empty(){continue;}
        let row:Value=serde_json::from_str(&line).map_err(|e|format!("bad dataset JSONL: {e}"))?;
        let id=row["instance_id"].as_str().ok_or("instance_id missing")?;
        let repo=row["repo"].as_str().ok_or("repo missing")?;
        let commit=row["base_commit"].as_str().ok_or("base_commit missing")?;
        let statement=row["problem_statement"].as_str().ok_or("problem_statement missing")?;
        let parts=repo.split('/').collect::<Vec<_>>();
        if !safe_id(id)||parts.len()!=2||!parts.iter().all(|s|safe_id(s))
            ||commit.len()!=40||!commit.bytes().all(|b|b.is_ascii_hexdigit())
            ||statement.is_empty()||statement.len()>200_000
        {return Err(format!("invalid SWE-bench task identity: {id}"))}
        if !seen.insert(id.to_owned()){return Err(format!("duplicate SWE-bench id {id}"))}
        tasks.push(Task{id:id.into(),repo:repo.into(),commit:commit.into(),problem:statement.into()});
        if tasks.len()==limit{break;}
    }
    if tasks.is_empty(){return Err("no SWE-bench tasks selected".into());}
    Ok(tasks)
}
async fn command(program:&str,args:&[&str],cwd:&Path,limit:Duration)->Result<String,String>{
    let mut cmd=Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).kill_on_drop(true);
    let result=timeout(limit,cmd.output()).await
        .map_err(|_|format!("{program} timed out"))?
        .map_err(|e|format!("{program}: {e}"))?;
    if !result.status.success(){
        return Err(format!("{program} {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&result.stderr).chars().take(3000).collect::<String>()));
    }
    let text=String::from_utf8_lossy(&result.stdout);
    if text.len()>MAX_PATCH*2 {return Err(format!("{program} output too large"))}
    Ok(text.into_owned())
}
fn free_port()->Result<u16,String>{
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener|listener.local_addr())
        .map(|addr|addr.port()).map_err(|e|e.to_string())
}
async fn json_get(client:&reqwest::Client,url:&str)->Result<Value,String>{
    let r=client.get(url).send().await.map_err(|e|e.to_string())?;
    let status=r.status();let v:Value=r.json().await.map_err(|e|e.to_string())?;
    if !status.is_success(){return Err(format!("GET {url}: HTTP {status} {v}"))}
    Ok(v)
}
async fn json_post(client:&reqwest::Client,url:&str,data:&Value)->Result<Value,String>{
    // Mutating calls are never retried. A timeout may have created a goal.
    let r=client.post(url).json(data).send().await.map_err(|e|e.to_string())?;
    let status=r.status();let v:Value=r.json().await.map_err(|e|e.to_string())?;
    if !status.is_success(){return Err(format!("POST {url}: HTTP {status} {v}"))}
    Ok(v)
}
async fn wait_ready(client:&reqwest::Client,base:&str,child:&mut Child)->Result<(),String>{
    for _ in 0..80{
        if child.try_wait().map_err(|e|e.to_string())?.is_some(){
            return Err("DOMINION engine exited before /api/health".into());
        }
        if json_get(client,&format!("{base}/api/health")).await.is_ok(){return Ok(());}
        sleep(Duration::from_millis(250)).await;
    }
    Err("DOMINION engine did not become ready".into())
}
async fn take_patch(work:&Path)->Result<String,String>{
    if !work.join(".git").exists(){return Err("goal worktree missing".into());}
    // Intent-to-add makes genuinely new files appear in a unified diff,
    // without committing them or changing the original checkout.
    command("git",&["add","-N","--","."],work,Duration::from_secs(30)).await?;
    let patch=command("git",&["diff","--binary","--no-ext-diff","HEAD","--"],work,Duration::from_secs(30)).await?;
    if patch.len()>MAX_PATCH{return Err("generated patch exceeds 2 MiB".into());}
    Ok(patch)
}
struct ChildGuard(Child);
impl Drop for ChildGuard{
    fn drop(&mut self){
        let _=self.0.start_kill();
    }
}
async fn solve(cfg:&Options,task:&Task,client:&reqwest::Client)->Result<Value,String>{
    let root=cfg.output.join(&task.id);
    fs::create_dir_all(&root).map_err(|e|e.to_string())?;
    let source=root.join("source");
    let repo_url=format!("https://github.com/{}.git",task.repo);
    let url=repo_url.as_str();
    command("git",&["clone","--quiet","--filter=blob:none","--no-checkout",url,
        source.to_str().ok_or("source path not UTF-8")?],
        &root,Duration::from_secs(600)).await?;
    command("git",&["checkout","--quiet","--detach",&task.commit],&source,Duration::from_secs(180)).await?;
    let base_commit=command("git",&["rev-parse","HEAD"],&source,Duration::from_secs(10)).await?;
    if base_commit.trim()!=task.commit{return Err("wrong checked-out base commit".into());}
    let port=free_port()?;
    let base=format!("http://127.0.0.1:{port}");
    let logs=fs::File::create(root.join("engine.log")).map_err(|e|e.to_string())?;
    let mut server=Command::new(&cfg.engine);
    server.current_dir(&source)
        .env("DOMINION_PORT",port.to_string())
        .env("DOMINION_DATA",root.join("data"))
        .env("DOMINION_WEB",root.join("empty-web"))
        .env("DOMINION_WORKSPACE",&source)
        .env("DOMINION_BENCHMARK","1")
        .stdin(Stdio::null()).stdout(Stdio::from(logs.try_clone().map_err(|e|e.to_string())?))
        .stderr(Stdio::from(logs)).kill_on_drop(true);
    let mut child=ChildGuard(server.spawn().map_err(|e|e.to_string())?);
    wait_ready(client,&base,&mut child.0).await?;
    let mut provider=json!({
        "name":"SWE-bench frozen provider",
        "kind":cfg.provider_kind,
        "baseUrl":cfg.provider_url,
        "model":cfg.provider_model,
        "enabled":true
    });
    if let Some(env_name)=&cfg.provider_key_env{
        let key=env::var(env_name).map_err(|_|format!("missing provider key env {env_name}"))?;
        provider["apiKey"]=json!(key);
    }
    let provider=json_post(client,&format!("{base}/api/providers"),&provider).await?;
    let provider_id=provider["id"].as_str().ok_or("provider id absent")?;
    let role=json!({"providerId":provider_id,"model":cfg.provider_model});
    // No solution, hidden tests, gold patch or evaluator report is sent.
    // Acceptance is decided only by the official SWE-bench Docker oracle.
    let goal=json_post(client,&format!("{base}/api/goals"),&json!({
        "goal":format!("Repair the issue in this checked-out repository:\n\n{}\n\nUse native source tools, inspect relevant tests and change only necessary source files. Verify locally when possible. Produce actual code changes; a textual answer alone does not solve the issue.",task.problem),
        "executor":role,"reviewer":role,"planner":role,
        "isolateWorktree":true,
        "maxIterations":cfg.max_iterations,
        "tokenBudget":cfg.token_budget,
        "successCriteria":[]
    })).await?;
    let goal_id=goal["id"].as_str().ok_or("goal id absent")?;
    let started=Instant::now();
    let mut state=goal;
    let mut timed_out=false;
    while !matches!(state["status"].as_str(),Some("completed"|"failed"|"cancelled"|"needs_review")){
        if started.elapsed()>cfg.goal_timeout {
            timed_out=true;
            let _=json_post(client,&format!("{base}/api/goals/{goal_id}/cancel"),&json!({})).await;
            sleep(Duration::from_millis(500)).await;
            break;
        }
        sleep(Duration::from_secs(2)).await;
        state=json_get(client,&format!("{base}/api/goals/{goal_id}")).await?;
    }
    let final_goal=json_get(client,&format!("{base}/api/goals/{goal_id}")).await?;
    let work=final_goal["gitWorktree"]["path"].as_str()
        .ok_or("isolated Git worktree was not created")?;
    let work=Path::new(work);
    if !work.starts_with(root.join("data")) {
        return Err("goal worktree escaped the isolated benchmark root".into());
    }
    let patch=take_patch(work).await?;
    let session=final_goal["sessionId"].as_str().unwrap_or("");
    let executions=json_get(client,&format!("{base}/api/executions")).await
        .unwrap_or(Value::Null);
    let mut tool_calls=0usize;
    let mut executions_seen=0usize;
    for execution in executions.as_array().into_iter().flatten(){
        if execution["sessionId"].as_str()!=Some(session){continue;}
        let Some(execution_id)=execution["id"].as_str()else{continue};
        executions_seen+=1;
        if let Ok(ops)=json_get(client,&format!("{base}/api/executions/{execution_id}/operations")).await{
            tool_calls+=ops.as_array().map(Vec::len).unwrap_or(0);
        }
    }
    Ok(json!({
        "instanceId":task.id,"repo":task.repo,"baseCommit":task.commit,
        "goalId":goal_id,"goalStatus":final_goal["status"],
        "timedOut":timed_out,"elapsedMs":started.elapsed().as_millis() as u64,
        "toolCalls":tool_calls,"executionCount":executions_seen,
        "tokenBudget":cfg.token_budget,
        "reportedTokens":Value::Null,
        "patchSha256":format!("{:x}",Sha256::digest(patch.as_bytes())),
        "patchBytes":patch.len(),"patch":patch,
        "notOfficiallyEvaluated":true
    }))
}
fn write_json(path:&Path,value:&Value)->Result<(),String>{
    fs::write(path,format!("{}\n",serde_json::to_string_pretty(value).map_err(|e|e.to_string())?))
        .map_err(|e|e.to_string())
}
async fn run()->Result<(),String>{
    let cfg=parse_args(&env::args().skip(1).collect::<Vec<_>>())?;
    let tasks=load_tasks(&cfg.tasks,cfg.limit)?;
    fs::create_dir_all(&cfg.output).map_err(|e|e.to_string())?;
    let client=reqwest::Client::builder()
        .timeout(Duration::from_secs(20)).build().map_err(|e|e.to_string())?;
    let predictions=cfg.output.join("predictions.jsonl");
    let mut pred=fs::File::create(&predictions).map_err(|e|e.to_string())?;
    let mut stats=Vec::new();
    let dataset_digest=fs::read(&cfg.tasks).map(|bytes|
        format!("{:x}",Sha256::digest(&bytes))).map_err(|e|e.to_string())?;
    for (index,task) in tasks.iter().enumerate(){
        eprintln!("[{}/{}] {}",index+1,tasks.len(),task.id);
        let result=solve(&cfg,task,&client).await;
        let record=match result{
            Ok(v)=>v,
            Err(message)=>json!({"instanceId":task.id,"error":message,
                "goalStatus":"runner_failure","patch":"","patchBytes":0,
                "toolCalls":Value::Null,"reportedTokens":Value::Null,"notOfficiallyEvaluated":true})
        };
        let patch=record["patch"].as_str().unwrap_or("");
        writeln!(pred,"{}",json!({
            "instance_id":task.id,
            "model_name_or_path":format!("dominion-rust__{}",cfg.provider_model.replace('/','_')),
            "model_patch":patch
        })).map_err(|e|e.to_string())?;
        pred.flush().map_err(|e|e.to_string())?;
        let mut metrics=record.clone();
        if let Some(map)=metrics.as_object_mut(){map.remove("patch");}
        stats.push(metrics);
        write_json(&cfg.output.join("progress.json"),&json!(stats))?;
    }
    let manifest=json!({
        "dataset":DATASET,"datasetExportSha256":dataset_digest,
        "model":cfg.provider_model,"providerKind":cfg.provider_kind,
        "providerBase":cfg.provider_url,"tokenBudget":cfg.token_budget,
        "maxIterations":cfg.max_iterations,
        "goalTimeoutSecs":cfg.goal_timeout.as_secs(),
        "requestedInstances":tasks.iter().map(|task|task.id.as_str()).collect::<Vec<_>>(),
        "predictionFormat":"official_swebench_jsonl",
        "noOfficialResultsUntilDockerEvaluation":true,
        "results":stats
    });
    write_json(&cfg.output.join("manifest.json"),&manifest)?;
    if cfg.evaluate {
        // The official SWE-bench harness, not DOMINION, grades the patches.
        // Fresh run IDs avoid cached reports from different patch bytes.
        let digest=fs::read(&predictions).map(|v|format!("{:x}",Sha256::digest(&v)))
            .map_err(|e|e.to_string())?;
        let run_id=format!("dominion-verified-{}",&digest[..12]);
        let mut cmd=Command::new("python");
        cmd.args(["-m","swebench.harness.run_evaluation",
            "--dataset_name",DATASET,
            "--predictions_path",predictions.to_str().ok_or("prediction path not UTF-8")?,
            "--max_workers","2","--run_id",&run_id]);
        let output=cmd.output().await.map_err(|e|format!("official SWE-bench runner failed to start: {e}"))?;
        fs::write(cfg.output.join("official-evaluation.log"),
            [&output.stdout[..],&output.stderr[..]].concat()).map_err(|e|e.to_string())?;
        if !output.status.success(){
            return Err(format!("official SWE-bench evaluation failed (see official-evaluation.log), exit {}",output.status));
        }
        write_json(&cfg.output.join("official-evaluation-run.json"),
            &json!({"runId":run_id,"dataset":DATASET,"status":"official_evaluator_exited_successfully",
                "note":"Read official resolved/unresolved reports before claiming benchmark success"}))?;
    }
    println!("Predictions: {}",predictions.display());
    println!("Results: {}",cfg.output.join("manifest.json").display());
    Ok(())
}
#[tokio::main]
async fn main(){
    if let Err(error)=run().await{
        eprintln!("DOMINION SWE-bench: {error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn reject_duplicate_and_unsafe_swebench_instances(){
        let path=env::temp_dir().join(format!("dominion-swe-{}.jsonl",uuid::Uuid::new_v4()));
        let valid=json!({"instance_id":"astropy__astropy-14539",
            "repo":"astropy/astropy",
            "base_commit":"a".repeat(40),"problem_statement":"Repair issue"});
        fs::write(&path,format!("{valid}\n{valid}\n")).unwrap();
        assert!(load_tasks(&path,5).unwrap_err().contains("duplicate"));
        let bad=json!({"instance_id":"../../escape",
            "repo":"astropy/astropy","base_commit":"a".repeat(40),
            "problem_statement":"no"});
        fs::write(&path,format!("{bad}\n")).unwrap();
        assert!(load_tasks(&path,5).is_err());
        fs::remove_file(path).unwrap();
    }
    #[test] fn task_loader_preserves_official_issue_without_leaking_gold_patch(){
        let path=env::temp_dir().join(format!("dominion-swe-{}.jsonl",uuid::Uuid::new_v4()));
        let row=json!({"instance_id":"test__repo-1","repo":"test/repo",
            "base_commit":"a".repeat(40),"problem_statement":"real issue",
            "patch":"SECRET_GOLD_SOLUTION","test_patch":"HIDDEN_TESTS"});
        fs::write(&path,format!("{row}\n")).unwrap();
        let tasks=load_tasks(&path,5).unwrap();
        assert_eq!(tasks.len(),1);
        assert_eq!(tasks[0].problem,"real issue");
        assert!(!format!("{tasks:?}").contains("SECRET_GOLD"));
        assert!(!format!("{tasks:?}").contains("HIDDEN_TESTS"));
        fs::remove_file(path).unwrap();
    }
}
