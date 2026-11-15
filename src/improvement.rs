use crate::{domain::now_ms, provider::ModelClient};
use serde::{Deserialize,Serialize};
use serde_json::json;
use std::{collections::{HashMap,HashSet},path::{Component,Path,PathBuf},process::Stdio,sync::Arc,time::Duration};
use tokio::{fs,io::{AsyncReadExt},process::Command,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum ImprovementStatus { AwaitingApproval, Running, Evaluated, Rejected, Adopted, Failed }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Measurement { pub tests_passed:bool, pub score:Option<f64>, pub log:String }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Improvement {
    pub id:Uuid, pub parent:Option<Uuid>, pub repository:String, pub target_file:String,
    pub instruction:String, pub test_command:String, pub score_command:Option<String>,
    pub status:ImprovementStatus, pub base_commit:Option<String>,pub candidate_commit:Option<String>,
    pub baseline:Option<Measurement>,pub candidate:Option<Measurement>,pub proposal:Option<String>,
    pub reflection:Option<String>,pub accepted:bool,pub cost_krw:f64,pub error:Option<String>,
    pub created_at_ms:u64,pub updated_at_ms:u64,
}
#[derive(Deserialize)]
pub struct NewImprovement {
    pub repository:String, pub target_file:String, pub instruction:String,
    pub test_command:String, pub score_command:Option<String>, pub parent:Option<Uuid>,
}
pub struct Improvements {
    root:PathBuf, workspace:PathBuf,
    items:Mutex<HashMap<Uuid,Improvement>>,active:Mutex<HashSet<Uuid>>
}
fn relative(raw:&str)->Result<&Path,String>{
    if raw.is_empty()||raw.len()>1024||raw.as_bytes().contains(&0){return Err("invalid relative path".into());}
    let path=Path::new(raw);
    if path.components().any(|c|!matches!(c,Component::Normal(_)|Component::CurDir)){return Err("relative paths only; parent traversal forbidden".into());}
    Ok(path)
}
async fn execute(program:&str,args:&[&str],dir:&Path,timeout:u64)->Result<String,String>{
    let mut child=if program=="sh" {
        let script=args.last().ok_or("shell script missing")?;
        crate::sandbox::script(dir,script)?
    }else{
        let mut command=Command::new(program);
        command.args(args).current_dir(dir).env_clear().env("PATH","/usr/local/bin:/usr/bin:/bin")
            .env("HOME",dir);
        command
    };
    child.stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child=child.spawn().map_err(|e|format!("{program}: {e}"))?;
    let out=child.stdout.take().ok_or("stdout unavailable")?;
    let err=child.stderr.take().ok_or("stderr unavailable")?;
    let joined=tokio::time::timeout(Duration::from_secs(timeout),async {
        let left=async{let mut bytes=vec![];out.take(16_385).read_to_end(&mut bytes).await.map_err(|e|e.to_string())?;Ok::<_,String>(bytes)};
        let right=async{let mut bytes=vec![];err.take(16_385).read_to_end(&mut bytes).await.map_err(|e|e.to_string())?;Ok::<_,String>(bytes)};
        let status=async{child.wait().await.map_err(|e|e.to_string())};
        tokio::try_join!(left,right,status)
    }).await.map_err(|_|format!("{program} timed out"))??;
    let combined=format!("{}\n{}",String::from_utf8_lossy(&joined.0),String::from_utf8_lossy(&joined.1));
    if joined.0.len()>16_384||joined.1.len()>16_384{return Err(format!("{program} output exceeds 16 KB"));}
    if !joined.2.success(){return Err(format!("{program} exit {}: {}",joined.2,combined.chars().take(3000).collect::<String>()));}
    Ok(combined.trim().to_string())
}
async fn git(dir:&Path,args:&[&str])->Result<String,String>{execute("git",args,dir,45).await}
async fn score(dir:&Path,command:&str)->Result<String,String>{
    // Arbitrary shell is opt-in via DOMINION_ENABLE_SHELL and per-experiment approval.
    execute("sh",&["-lc",command],dir,100).await
}
fn parse_score(text:&str)->Result<f64,String>{
    let value=text.lines().rev().find_map(|line|line.trim().parse::<f64>().ok()).ok_or("score_command must print a numeric score on its own line")?;
    if !value.is_finite(){return Err("non-finite score".into());}
    Ok(value)
}
fn is_better(baseline:&Measurement,candidate:&Measurement)->bool{
    candidate.tests_passed && (!baseline.tests_passed ||
        matches!((baseline.score,candidate.score),(Some(a),Some(b)) if b>a+0.000001))
}
impl Improvements {
    pub async fn open(root:PathBuf,workspace:PathBuf)->Result<Self,String>{
        fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        fs::create_dir_all(&workspace).await.map_err(|e|e.to_string())?;
        let mut items=HashMap::new();
        let mut entries=fs::read_dir(&root).await.map_err(|e|e.to_string())?;
        while let Some(entry)=entries.next_entry().await.map_err(|e|e.to_string())?{
            if entry.path().extension().and_then(|s|s.to_str())!=Some("json"){continue;}
            let mut item:Improvement=serde_json::from_slice(&fs::read(entry.path()).await.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            if item.status==ImprovementStatus::Running {
                item.status=ImprovementStatus::Failed;item.error=Some("Interrupted evaluation; create a new descendant trial".into());
            }
            items.insert(item.id,item);
        }
        Ok(Self{root,workspace,items:Mutex::new(items),active:Mutex::new(HashSet::new())})
    }
    async fn put(&self,item:Improvement)->Result<Improvement,String>{
        let mut lock=self.items.lock().await;
        let tmp=self.root.join(format!("{}.tmp",item.id));
        let file=self.root.join(format!("{}.json",item.id));
        fs::write(&tmp,serde_json::to_vec(&item).map_err(|e|e.to_string())?).await.map_err(|e|e.to_string())?;
        fs::rename(tmp,file).await.map_err(|e|e.to_string())?;
        lock.insert(item.id,item.clone());Ok(item)
    }
    pub async fn get(&self,id:Uuid)->Option<Improvement>{self.items.lock().await.get(&id).cloned()}
    pub async fn list(&self)->Vec<Improvement>{
        let mut values:Vec<_>=self.items.lock().await.values().cloned().collect();
        values.sort_by_key(|v|std::cmp::Reverse(v.created_at_ms));values
    }
    pub async fn create(&self,input:NewImprovement)->Result<Improvement,String>{
        relative(&input.repository)?;relative(&input.target_file)?;
        if input.instruction.trim().is_empty()||input.instruction.len()>8000{return Err("invalid improvement instruction".into());}
        if input.test_command.trim().is_empty()||input.test_command.len()>2048{return Err("invalid test command".into());}
        if let Some(command)=&input.score_command{if command.trim().is_empty()||command.len()>2048{return Err("invalid score command".into());}}
        if let Some(parent)=input.parent{
            let parent=self.get(parent).await.ok_or("parent experiment not found")?;
            if parent.status!=ImprovementStatus::Adopted{return Err("parent trial must be adopted before a descendant".into());}
            if parent.repository!=input.repository || parent.target_file!=input.target_file {return Err("descendant repository and file must match parent".into());}
        }
        let root=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repository=root.join(relative(&input.repository)?);
        let actual=repository.canonicalize().map_err(|e|format!("repository inaccessible: {e}"))?;
        if !actual.starts_with(&root)||!actual.join(".git").exists(){return Err("repository must be a Git checkout inside configured workspace".into());}
        let status=ImprovementStatus::AwaitingApproval;
        let now=now_ms();
        self.put(Improvement{id:Uuid::new_v4(),parent:input.parent,repository:input.repository,target_file:input.target_file,
            instruction:input.instruction,test_command:input.test_command,score_command:input.score_command,status,
            base_commit:None,candidate_commit:None,baseline:None,candidate:None,proposal:None,reflection:None,
            accepted:false,cost_krw:0.0,error:None,created_at_ms:now,updated_at_ms:now}).await
    }
    pub async fn start(self:&Arc<Self>,model:ModelClient,id:Uuid,shell_enabled:bool)->Result<Improvement,String>{
        if !shell_enabled{return Err("DOMINION_ENABLE_SHELL=true is required to execute approved evaluation commands".into());}
        {let mut busy=self.active.lock().await;if !busy.insert(id){return Err("evaluation already active".into());}}
        let Some(mut item)=self.get(id).await else {self.active.lock().await.remove(&id);return Err("trial not found".into());};
        if item.status!=ImprovementStatus::AwaitingApproval{self.active.lock().await.remove(&id);return Err("trial is not awaiting approval".into());}
        item.status=ImprovementStatus::Running;item.updated_at_ms=now_ms();
        if let Err(e)=self.put(item.clone()).await{self.active.lock().await.remove(&id);return Err(e);}
        let service=Arc::clone(self);
        tokio::spawn(async move{
            let result=service.evaluate(&model,item).await;
            if let Err(error)=result {
                if let Some(mut item)=service.get(id).await {
                    item.status=ImprovementStatus::Failed;item.error=Some(error);item.updated_at_ms=now_ms();
                    let _=service.put(item).await;
                }
            }
            service.active.lock().await.remove(&id);
        });
        self.get(id).await.ok_or("trial missing after start".into())
    }
    async fn evaluate(&self,model:&ModelClient,mut item:Improvement)->Result<(),String>{
        let repo=self.workspace.canonicalize().map_err(|e|e.to_string())?.join(relative(&item.repository)?)
            .canonicalize().map_err(|e|e.to_string())?;
        let head=git(&repo,&["rev-parse","HEAD"]).await?;
        let head=head.trim().to_string();
        if !git(&repo,&["status","--porcelain"]).await?.is_empty(){return Err("repository must have a clean working tree".into());}
        item.base_commit=Some(head.clone());self.put(item.clone()).await?;
        let trial_root=self.root.join(item.id.to_string());
        fs::create_dir_all(&trial_root).await.map_err(|e|e.to_string())?;
        let trial_root=trial_root.canonicalize().map_err(|e|e.to_string())?;
        let reference=trial_root.join("baseline");
        let candidate=trial_root.join("candidate");
        for destination in [&reference,&candidate]{
            let path=destination.to_str().ok_or("invalid worktree path")?;
            git(&repo,&["worktree","add","--detach",path,&head]).await?;
        }
        let result=self.evaluate_worktrees(model,&mut item,&reference,&candidate).await;
        for dir in [&reference,&candidate] {
            if let Some(path)=dir.to_str(){let _=git(&repo,&["worktree","remove","--force",path]).await;}
        }
        result
    }
    async fn evaluate_worktrees(&self,model:&ModelClient,item:&mut Improvement,reference:&Path,candidate:&Path)->Result<(),String>{
        let file=relative(&item.target_file)?;
        let source=candidate.join(file);
        let actual=source.canonicalize().map_err(|e|format!("candidate target unavailable: {e}"))?;
        if !actual.starts_with(candidate){return Err("target escapes candidate worktree".into());}
        if !actual.is_file(){return Err("target must be a regular file".into());}
        if fs::metadata(&actual).await.map_err(|e|e.to_string())?.len()>100_000{return Err("target file exceeds 100 KB".into());}
        let original=fs::read_to_string(&actual).await.map_err(|e|e.to_string())?;
        let inherited=if let Some(parent)=item.parent { self.get(parent).await.and_then(|p|p.reflection).unwrap_or_default() }else{String::new()};
        let prompts=vec![
            json!({"role":"system","content":"Propose one concrete, improved version of the supplied source file. Return ONLY the complete new source text, without markdown fences. Do not alter tests or evaluation scripts. Prefer minimal maintainable improvements. You are optimizing a fixed program against independent tests and a frozen scoring command. Do not game the metric or weaken safety checks."}),
            json!({"role":"user","content":format!("Improvement objective:\n{}\nInherited measured lessons:\n{}\nFile: {}\nCurrent contents:\n{}",item.instruction,inherited,item.target_file,original)})
        ];
        let reply=model.complete(&prompts,&[],false).await?;
        if reply.text.is_empty() || reply.text.len()>150_000 || reply.text==original {return Err("model did not propose a distinct bounded candidate".into());}
        let text=reply.text.trim().strip_prefix("```").map(|s|s.trim_start_matches(|c:char|c!='\n').trim_start_matches('\n').trim_end_matches("```").trim().to_string()).unwrap_or(reply.text);
        if text.is_empty(){return Err("candidate is empty".into());}
        fs::write(&actual,&text).await.map_err(|e|e.to_string())?;
        git(candidate,&["add","--",&item.target_file]).await?;
        execute("git",&["-c","user.name=Dominion","-c","user.email=dominion@localhost","commit","-m","dominion: candidate improvement"],candidate,45).await?;
        item.candidate_commit=Some(git(candidate,&["rev-parse","HEAD"]).await?.trim().to_string());
        item.proposal=Some(format!("{} bytes -> {} bytes",original.len(),text.len()));
        item.cost_krw=reply.cost_krw;
        self.put(item.clone()).await?;
        let baseline=self.measure(reference,&item.test_command,item.score_command.as_deref()).await;
        let candidate_measure=self.measure(candidate,&item.test_command,item.score_command.as_deref()).await;
        let a=baseline?;
        let b=candidate_measure?;
        let accepted=is_better(&a,&b);
        item.reflection=Some(format!("Previous score {:?}; candidate score {:?}; baseline tests: {}; candidate tests: {}; adopted recommendation: {}. Baseline log: {} Candidate log: {}",
            a.score,b.score,a.tests_passed,b.tests_passed,accepted,a.log.chars().take(1000).collect::<String>(),b.log.chars().take(1000).collect::<String>()));
        item.baseline=Some(a);item.candidate=Some(b);item.accepted=accepted;
        item.status=if accepted{ImprovementStatus::Evaluated}else{ImprovementStatus::Rejected};
        item.updated_at_ms=now_ms();
        self.put(item.clone()).await?;
        Ok(())
    }
    async fn measure(&self,dir:&Path,tests:&str,scoring:Option<&str>)->Result<Measurement,String>{
        let result=score(dir,tests).await;
        let tests_passed=result.is_ok();
        let log=result.unwrap_or_else(|error|error);
        let value=if tests_passed {
            if let Some(scoring)=scoring {Some(parse_score(&score(dir,scoring).await?)?)} else {None}
        }else{None};
        Ok(Measurement{tests_passed,score:value,log:log.chars().take(2000).collect()})
    }
    pub async fn adopt(&self,id:Uuid)->Result<Improvement,String>{
        if self.active.lock().await.contains(&id){return Err("trial still active".into());}
        let mut item=self.get(id).await.ok_or("trial not found")?;
        if item.status!=ImprovementStatus::Evaluated || !item.accepted{return Err("candidate has not outperformed frozen evaluation".into());}
        let root=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repo=root.join(relative(&item.repository)?).canonicalize().map_err(|e|e.to_string())?;
        if !repo.starts_with(&root){return Err("repository escapes workspace".into());}
        let head=git(&repo,&["rev-parse","HEAD"]).await?;
        if Some(head.trim())!=item.base_commit.as_deref(){return Err("repository HEAD changed since evaluation; refusing adoption".into());}
        if !git(&repo,&["status","--porcelain"]).await?.is_empty(){return Err("repository has uncommitted changes".into());}
        let commit=item.candidate_commit.as_deref().ok_or("missing candidate commit")?;
        execute("git",&["-c","user.name=Dominion","-c","user.email=dominion@localhost","cherry-pick",commit],&repo,45).await?;
        item.status=ImprovementStatus::Adopted;item.updated_at_ms=now_ms();
        self.put(item).await
    }
    pub async fn descendant(&self,id:Uuid)->Result<Improvement,String>{
        let parent=self.get(id).await.ok_or("trial not found")?;
        self.create(NewImprovement{
            repository:parent.repository,target_file:parent.target_file,
            instruction:parent.instruction,test_command:parent.test_command,
            score_command:parent.score_command,parent:Some(id)
        }).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn objective_scoring_requires_real_gain(){
        let base=Measurement{tests_passed:true,score:Some(0.8),log:String::new()};
        let worse=Measurement{tests_passed:true,score:Some(0.8),log:String::new()};
        let better=Measurement{tests_passed:true,score:Some(0.9),log:String::new()};
        assert!(!is_better(&base,&worse));assert!(is_better(&base,&better));
        assert!(!is_better(&base,&Measurement{tests_passed:false,score:Some(1.0),log:String::new()}));
    }
    #[test]
    fn rejects_traversal(){assert!(relative("../outside").is_err());assert!(relative("/root").is_err());}
}

#[cfg(test)]
mod full_cycle_tests {
    use super::*;
    use crate::config::Settings;
    use axum::{routing::post,Json,Router};
    use serde_json::Value;
    async fn proposal(Json(_request):Json<Value>)->Json<Value>{
        Json(json!({"choices":[{"message":{"role":"assistant","content":"1\n"}}],"usage":{"prompt_tokens":5,"completion_tokens":1}}))
    }
    #[tokio::test]
    async fn adopts_only_a_scored_improvement_and_inherits_reflection() {
        if Command::new("git").arg("--version").output().await.is_err(){return;}
        let root=std::env::temp_dir().join(format!("dominion-improvement-{}",Uuid::new_v4()));
        let workspace=root.join("workspace");
        let repo=workspace.join("repo");
        fs::create_dir_all(&repo).await.unwrap();
        // The RSI integration test must exercise the enforced runtime when
        // supported; do not silently disable it for test convenience.
        let probe=crate::sandbox::script(&repo,"true");
        let can_sandbox=match probe {
            Ok(mut command)=>command.status().await.map(|s|s.success()).unwrap_or(false),
            Err(_)=>false
        };
        if !can_sandbox {let _=fs::remove_dir_all(&root).await;return;}
        git(&repo,&["init"]).await.unwrap();
        fs::write(repo.join("score.txt"),"0\n").await.unwrap();
        git(&repo,&["add","score.txt"]).await.unwrap();
        execute("git",&["-c","user.name=Test","-c","user.email=test@example.com","commit","-m","base"],&repo,30).await.unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(proposal))).await.unwrap();
        });
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{addr}");settings.main.api_key.clear();
        let model=ModelClient::new(&settings).unwrap();
        let service=Arc::new(Improvements::open(root.join("data"),workspace).await.unwrap());
        let item=service.create(NewImprovement{
            repository:"repo".into(),target_file:"score.txt".into(),instruction:"increase score".into(),
            test_command:"test -f score.txt".into(),score_command:Some("cat score.txt".into()),parent:None
        }).await.unwrap();
        service.start(model,item.id,true).await.unwrap();
        let mut result=None;
        for _ in 0..150 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let current=service.get(item.id).await.unwrap();
            if current.status!=ImprovementStatus::Running{result=Some(current);break;}
        }
        let result=result.expect("evaluation timed out");
        assert_eq!(result.status,ImprovementStatus::Evaluated,"error: {:?}",result.error);
        assert!(result.accepted);
        assert_eq!(result.baseline.as_ref().unwrap().score,Some(0.0));
        assert_eq!(result.candidate.as_ref().unwrap().score,Some(1.0));
        // A persisted evaluation result can become visible just before the
        // worker releases its active marker. Wait for the actual detach
        // boundary before asserting adoption; avoid a timing-dependent test.
        for _ in 0..100 {
            if !service.active.lock().await.contains(&item.id) {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!service.active.lock().await.contains(&item.id),"evaluator did not detach");
        let adopted=service.adopt(item.id).await.unwrap();
        assert_eq!(adopted.status,ImprovementStatus::Adopted);
        assert_eq!(fs::read_to_string(repo.join("score.txt")).await.unwrap(),"1\n");
        let next=service.descendant(item.id).await.unwrap();
        assert_eq!(next.parent,Some(item.id));
        server.abort();
        fs::remove_dir_all(root).await.unwrap();
    }
}
