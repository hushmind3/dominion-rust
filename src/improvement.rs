use crate::{domain::now_ms, provider::ModelClient};
use serde::{Deserialize,Serialize};
use serde_json::json;
use std::{collections::{HashMap,HashSet},path::{Component,Path,PathBuf},process::Stdio,sync::Arc,time::Duration};
use tokio::{fs,io::{AsyncReadExt,AsyncWriteExt},process::Command,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum ImprovementStatus { AwaitingApproval, Running, Evaluated, Rejected, Adopted, Failed }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Measurement { pub tests_passed:bool, pub score:Option<f64>, pub log:String }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Improvement {
    pub id:Uuid, pub parent:Option<Uuid>, pub repository:String, pub target_file:String,
    /// Explicit bounded set of source files; an empty value loads legacy single-file trials.
    #[serde(default)]
    pub target_files:Vec<String>,
    #[serde(default)]
    pub attempts:usize,
    pub instruction:String, pub test_command:String, pub score_command:Option<String>,
    pub status:ImprovementStatus, pub base_commit:Option<String>,pub candidate_commit:Option<String>,
    pub baseline:Option<Measurement>,pub candidate:Option<Measurement>,pub proposal:Option<String>,
    pub reflection:Option<String>,pub accepted:bool,pub cost_krw:f64,pub error:Option<String>,
    pub created_at_ms:u64,pub updated_at_ms:u64,
}
#[derive(Deserialize)]
pub struct NewImprovement {
    pub repository:String, pub target_file:String,
    #[serde(default)]
    pub target_files:Option<Vec<String>>,
    pub instruction:String, pub test_command:String, pub score_command:Option<String>, pub parent:Option<Uuid>,
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

fn target_paths(primary:&str,requested:Option<Vec<String>>)->Result<Vec<String>,String>{
    relative(primary)?;
    let files=requested.unwrap_or_else(||vec![primary.to_owned()]);
    if files.is_empty()||files.len()>8||!files.iter().any(|path|path==primary){
        return Err("RSI requires 1..8 source files including the primary target".into());
    }
    let mut unique=HashSet::new();
    for file in &files {
        relative(file)?;
        if !unique.insert(file.clone()){return Err("duplicate RSI target file".into());}
        let path=Path::new(file);
        let protected=path.components().any(|part|matches!(part,Component::Normal(s)
            if matches!(s.to_str(),Some(".github"|"tests"|"__tests__"|"fixtures"))))
            ||path.file_name().and_then(|s|s.to_str())
                .is_some_and(|name|name.starts_with("test_")||name.ends_with("_test.rs")
                    ||name.ends_with(".spec.ts")||name.ends_with(".test.ts"));
        if protected{return Err(format!("RSI evaluation/test files cannot be candidate targets: {file}"));}
    }
    Ok(files)
}
fn decode_proposal(text:&str,sources:&[(String,String)])->Result<Vec<(String,String)>,String>{
    if text.is_empty()||text.len()>400_000{return Err("candidate response is empty or oversized".into());}
    let stripped=text.trim();
    let json=if let Some(data)=stripped.strip_prefix("```json"){
        data.trim().strip_suffix("```").unwrap_or(data).trim()
    }else{stripped};
    let structured=serde_json::from_str::<serde_json::Value>(json).ok()
        .filter(|value|value.get("files").is_some());
    if let Some(value)=structured {
        let files=value.get("files").and_then(|v|v.as_array())
            .ok_or("candidate JSON files must be an array")?;
        if files.len()!=sources.len(){return Err("candidate must include every designated source file".into());}
        let mut received=HashMap::new();
        for entry in files {
            let path=entry.get("path").and_then(|v|v.as_str()).ok_or("candidate path missing")?;
            let content=entry.get("content").and_then(|v|v.as_str()).ok_or("candidate content missing")?;
            if received.insert(path.to_owned(),content.to_owned()).is_some(){
                return Err("duplicate candidate path".into());
            }
        }
        if received.keys().any(|path|!sources.iter().any(|(expected,_)|expected==path)){
            return Err("candidate attempted to edit a non-designated file".into());
        }
        let mut output=Vec::new();
        for (path,_) in sources {
            output.push((path.clone(),received.remove(path).ok_or("candidate source missing")?));
        }
        return Ok(output);
    }
    if sources.len()!=1{
        return Err("multi-file RSI candidate must use a JSON files array".into());
    }
    // Backward compatibility: single-file adapters may return plain source,
    // optionally fenced in markdown.
    let content=if let Some(data)=stripped.strip_prefix("```"){
        data.split_once('\n').map(|(_,body)|body.trim_end_matches('`').trim_end_matches('\n'))
            .unwrap_or(stripped)
    }else{text};
    Ok(vec![(sources[0].0.clone(),content.to_owned())])
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

/// Persist RSI judgments and their candidate/baseline evidence before exposing
/// a new state to SDK/API consumers. In particular, a completed adoption must
/// not be lost merely because the process died between a write and a rename.
async fn persist_improvement(root:&Path,item:&Improvement)->Result<(),String>{
    let file=root.join(format!("{}.json",item.id));
    let temporary=root.join(format!("{}-{}.tmp",item.id,Uuid::new_v4()));
    let result=async {
        let bytes=serde_json::to_vec(item).map_err(|e|e.to_string())?;
        let mut handle=fs::OpenOptions::new().write(true).create_new(true)
            .open(&temporary).await.map_err(|e|format!("RSI snapshot staging failed: {e}"))?;
        handle.write_all(&bytes).await.map_err(|e|format!("RSI snapshot write failed: {e}"))?;
        handle.sync_all().await.map_err(|e|format!("RSI snapshot fsync failed: {e}"))?;
        drop(handle);
        fs::rename(&temporary,&file).await.map_err(|e|format!("RSI snapshot rename failed: {e}"))?;
        #[cfg(unix)]
        {
            let dir=root.to_owned();
            tokio::task::spawn_blocking(move||std::fs::File::open(dir)
                .and_then(|file|file.sync_all()).map_err(|e|e.to_string()))
                .await.map_err(|e|format!("RSI directory sync task failed: {e}"))??;
        }
        Ok::<(),String>(())
    }.await;
    if result.is_err(){let _=fs::remove_file(&temporary).await;}
    result
}

impl Improvements {
    pub async fn open(root:PathBuf,workspace:PathBuf)->Result<Self,String>{
        fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        fs::create_dir_all(&workspace).await.map_err(|e|e.to_string())?;
        let mut items=HashMap::new();
        let mut recovered=Vec::new();
        let mut entries=fs::read_dir(&root).await.map_err(|e|e.to_string())?;
        while let Some(entry)=entries.next_entry().await.map_err(|e|e.to_string())?{
            if entry.path().extension().and_then(|s|s.to_str())!=Some("json"){continue;}
            let mut item:Improvement=serde_json::from_slice(&fs::read(entry.path()).await.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            if item.status==ImprovementStatus::Running {
                // An interrupted independent evaluation cannot be adopted or
                // silently retried: the candidate's exact outcome is unknown.
                item.status=ImprovementStatus::Failed;
                item.error=Some("Interrupted evaluation; outcome unknown and never auto-adopted".into());
                item.updated_at_ms=now_ms();
                recovered.push(item.clone());
            }
            items.insert(item.id,item);
        }
        // Recovery is durable before clients can observe the recovered state.
        for item in &recovered {persist_improvement(&root,item).await?;}
        Ok(Self{root,workspace,items:Mutex::new(items),active:Mutex::new(HashSet::new())})
    }
    async fn put(&self,item:Improvement)->Result<Improvement,String>{
        let mut lock=self.items.lock().await;
        persist_improvement(&self.root,&item).await?;
        lock.insert(item.id,item.clone());Ok(item)
    }
    pub async fn get(&self,id:Uuid)->Option<Improvement>{self.items.lock().await.get(&id).cloned()}
    pub async fn list(&self)->Vec<Improvement>{
        let mut values:Vec<_>=self.items.lock().await.values().cloned().collect();
        values.sort_by_key(|v|std::cmp::Reverse(v.created_at_ms));values
    }
    pub async fn create(&self,input:NewImprovement)->Result<Improvement,String>{
        relative(&input.repository)?;
        let target_files=target_paths(&input.target_file,input.target_files.clone())?;
        if input.instruction.trim().is_empty()||input.instruction.len()>8000{return Err("invalid improvement instruction".into());}
        if input.test_command.trim().is_empty()||input.test_command.len()>2048{return Err("invalid test command".into());}
        if let Some(command)=&input.score_command{if command.trim().is_empty()||command.len()>2048{return Err("invalid score command".into());}}
        if let Some(parent)=input.parent{
            let parent=self.get(parent).await.ok_or("parent experiment not found")?;
            if parent.status!=ImprovementStatus::Adopted{return Err("parent trial must be adopted before a descendant".into());}
            if parent.repository!=input.repository || parent.target_file!=input.target_file
                || (!parent.target_files.is_empty() && parent.target_files!=target_files) {
                return Err("descendant repository and file set must match parent".into());
            }
        }
        let root=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repository=root.join(relative(&input.repository)?);
        let actual=repository.canonicalize().map_err(|e|format!("repository inaccessible: {e}"))?;
        if !actual.starts_with(&root)||!actual.join(".git").exists(){return Err("repository must be a Git checkout inside configured workspace".into());}
        let status=ImprovementStatus::AwaitingApproval;
        let now=now_ms();
        self.put(Improvement{id:Uuid::new_v4(),parent:input.parent,repository:input.repository,target_file:input.target_file,
            target_files,attempts:0,
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
        let judge=trial_root.join("judge");
        let mut attached=Vec::new();
        for destination in [&reference,&candidate,&judge]{
            let path=destination.to_str().ok_or("invalid worktree path")?;
            if let Err(error)=git(&repo,&["worktree","add","--detach",path,&head]).await{
                for path in attached.into_iter().rev() {
                    let _=git(&repo,&["worktree","remove","--force",path]).await;
                }
                let _=fs::remove_dir(&trial_root).await;
                return Err(error);
            }
            attached.push(path);
        }
        let result=self.evaluate_worktrees(model,&mut item,&reference,&candidate,&judge).await;
        for path in attached.into_iter().rev() {
            if let Err(error)=git(&repo,&["worktree","remove","--force",path]).await {
                tracing::warn!(%error,path,"RSI temporary worktree cleanup incomplete");
            }
        }
        // Remove the now-empty trial directory, keeping only the durable JSON
        // experiment snapshot and accepted Git ref. No source checkout is touched.
        if let Err(error)=fs::remove_dir(&trial_root).await {
            tracing::warn!(%error,"RSI trial workspace cleanup incomplete");
        }
        result
    }

    /// Source changes originate from the model; acceptance originates only
    /// from a separate worktree and operator-frozen test/score commands.
    async fn evaluate_worktrees(&self,model:&ModelClient,item:&mut Improvement,
        reference:&Path,candidate:&Path,judge:&Path)->Result<(),String>{
        let paths=if item.target_files.is_empty(){
            vec![item.target_file.clone()]
        }else{item.target_files.clone()};
        let mut sources=Vec::<(String,String)>::new();
        let mut total=0usize;
        let workspace=candidate.canonicalize().map_err(|e|e.to_string())?;
        for path in paths {
            let relative=relative(&path)?;
            let actual=candidate.join(relative).canonicalize()
                .map_err(|e|format!("candidate target unavailable: {e}"))?;
            if !actual.starts_with(&workspace) || !actual.is_file(){
                return Err("candidate target escaped its isolated worktree".into());
            }
            let bytes=fs::metadata(&actual).await.map_err(|e|e.to_string())?.len() as usize;
            if bytes>100_000{return Err("a candidate source exceeds 100 KB".into());}
            total+=bytes;
            if total>180_000{return Err("candidate source set exceeds 180 KB".into());}
            let original=fs::read_to_string(&actual).await.map_err(|e|e.to_string())?;
            sources.push((path,original));
        }
        let inherited=if let Some(parent)=item.parent {
            self.get(parent).await.and_then(|p|p.reflection).unwrap_or_default()
        }else{String::new()};
        let baseline=self.measure(reference,&item.test_command,item.score_command.as_deref()).await?;
        item.baseline=Some(baseline.clone());
        self.put(item.clone()).await?;
        let mut prompts=vec![
            json!({"role":"system","content":"Improve the designated source files. For multiple files return ONLY JSON of shape {\"files\":[{\"path\":\"exact designated path\",\"content\":\"complete source text\"}]}. Include each specified path exactly once. For one source file, a complete plain-text file is accepted. Never edit tests, evaluation commands, configuration or other paths. Preserve functionality and user goals; a separate fixed judge checks your work."}),
            json!({"role":"user","content":format!(
                "Improvement goal:\n{}\nInherited measured lessons:\n{}\nExact authorized files and original source:\n{}\nThe tests and scoring commands are frozen; no changes outside these files are admitted.",
                item.instruction,inherited,serde_json::to_string(&sources).map_err(|e|e.to_string())?
            )})
        ];
        let base=item.base_commit.as_deref().ok_or("missing frozen base revision")?.to_owned();
        let mut last_problem=String::new();
        for attempt in 1..=3 {
            item.attempts=attempt;
            let reply=model.complete(&prompts,&[],false).await?;
            item.cost_krw+=reply.cost_krw;
            self.put(item.clone()).await?;
            // Reset and clean only the private disposable worktree created
            // for this trial, never the user's original source checkout.
            git(candidate,&["reset","--hard",&base]).await?;
            git(candidate,&["clean","-fd","--"]).await?;
            let changes=match decode_proposal(&reply.text,&sources){
                Ok(changes)=>changes,
                Err(error)=>{
                    last_problem=error;
                    prompts.push(json!({"role":"user","content":format!(
                        "Candidate format rejected: {}. Return only designated file contents.",last_problem
                    )}));
                    continue;
                }
            };
            if changes.iter().all(|(path,content)|sources.iter()
                .any(|(old_path,old)|old_path==path&&old==content)){
                last_problem="candidate did not change any source files".into();
                prompts.push(json!({"role":"user","content":last_problem}));
                continue;
            }
            let mut size=0usize;
            for (path,text) in &changes {
                size+=text.len();
                if text.len()>150_000||size>250_000{
                    return Err("candidate output source set exceeds limits".into());
                }
                let target=candidate.join(relative(path)?);
                let canonical=target.canonicalize().map_err(|e|e.to_string())?;
                if !canonical.starts_with(&workspace)||!canonical.is_file(){
                    return Err("candidate output escaped its isolated worktree".into());
                }
                fs::write(&canonical,text).await.map_err(|e|e.to_string())?;
            }
            let mut add=vec!["add","--"];
            for (path,_) in &changes {add.push(path.as_str());}
            git(candidate,&add).await?;
            execute("git",&["-c","user.name=Dominion","-c","user.email=dominion@localhost",
                "commit","-m","dominion: evaluated RSI source candidate"],candidate,45).await?;
            let commit=git(candidate,&["rev-parse","HEAD"]).await?;
            item.candidate_commit=Some(commit.clone());
            item.proposal=Some(format!("attempt {attempt}: {} files, {} source bytes",changes.len(),size));
            self.put(item.clone()).await?;
            // Judge has never received the candidate prompt and uses a clean
            // checkout of the original revision for every attempt.
            git(judge,&["reset","--hard",&base]).await?;
            git(judge,&["clean","-fdx","--"]).await?;
            git(judge,&["cherry-pick","--no-commit",&commit]).await?;
            // Commit the staged candidate state only as an in-memory Git tree
            // hash. The frozen judge's test/score commands are not permitted
            // to change that tracked source, with or without git add.
            let frozen_tree=git(judge,&["write-tree"]).await?;
            let measured=self.measure(judge,&item.test_command,item.score_command.as_deref()).await;
            let mut candidate_result=match measured {
                Ok(measurement)=>measurement,
                Err(error)=>Measurement{tests_passed:false,score:None,log:error}
            };
            let staged_unchanged=git(judge,&["write-tree"]).await
                .is_ok_and(|tree|tree==frozen_tree);
            let working_unchanged=git(judge,&["diff","--exit-code","--no-ext-diff","--"]).await.is_ok();
            if !staged_unchanged||!working_unchanged{
                candidate_result.tests_passed=false;
                candidate_result.score=None;
                candidate_result.log.push_str("\nREJECTED: evaluator changed frozen tracked source files");
            }
            let accepted=is_better(&baseline,&candidate_result);
            last_problem=format!(
                "Attempt {attempt}. Frozen tests passed: {}. Score: {:?} (baseline {:?}). Evidence: {}",
                candidate_result.tests_passed,candidate_result.score,baseline.score,
                candidate_result.log.chars().take(1000).collect::<String>()
            );
            item.candidate=Some(candidate_result);
            item.reflection=Some(format!(
                "Baseline tests: {}; candidate accepted: {}; {}",
                baseline.tests_passed,accepted,last_problem
            ));
            item.accepted=accepted;
            item.updated_at_ms=now_ms();
            if accepted {
                // Pin accepted detached commit so Git garbage collection cannot
                // discard it when the candidate worktree is removed.
                let pin=format!("refs/dominion/rsi/{}",item.id);
                git(candidate,&["update-ref",&pin,&commit]).await?;
                item.status=ImprovementStatus::Evaluated;
                self.put(item.clone()).await?;
                return Ok(());
            }
            item.status=ImprovementStatus::Running;
            self.put(item.clone()).await?;
            prompts.push(json!({"role":"user","content":format!(
                "Independent evaluator rejected the candidate: {}. Fix the source and retry without editing tests.",
                last_problem
            )}));
        }
        item.status=ImprovementStatus::Rejected;
        item.accepted=false;
        item.error=Some(format!("Three source attempts did not satisfy frozen evaluation: {last_problem}"));
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
        let pin=format!("refs/dominion/rsi/{}",item.id);
        // Existing serialized experiments without a pin remain compatible.
        if let Ok(actual)=git(&repo,&["rev-parse",&pin]).await{
            if actual!=commit{return Err("RSI accepted ref differs from frozen candidate".into());}
        }
        execute("git",&["-c","user.name=Dominion","-c","user.email=dominion@localhost","cherry-pick",commit],&repo,45).await?;
        item.status=ImprovementStatus::Adopted;item.updated_at_ms=now_ms();
        let adopted=self.put(item).await?;
        let _=git(&repo,&["update-ref","-d",&pin]).await;
        Ok(adopted)
    }
    pub async fn descendant(&self,id:Uuid)->Result<Improvement,String>{
        let parent=self.get(id).await.ok_or("trial not found")?;
        self.create(NewImprovement{
            repository:parent.repository,target_file:parent.target_file.clone(),
            target_files:Some(if parent.target_files.is_empty(){vec![parent.target_file.clone()]}
                else{parent.target_files.clone()}),
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
    #[test]
    fn preserves_existing_single_file_rsi_format(){
        let source=vec![("src/lib.rs".into(),"old".into())];
        assert_eq!(decode_proposal("new source\\n",&source).unwrap()[0].1,"new source\\n");
        assert!(target_paths("src/lib.rs",None).unwrap()==vec!["src/lib.rs"]);
    }

    #[test]
    fn parses_bounded_multifile_proposals_and_rejects_test_rewrites(){
        let sources=vec![
            ("src/a.rs".to_string(),"old a".to_string()),
            ("src/b.rs".to_string(),"old b".to_string())];
        let answer=r#"{"files":[{"path":"src/a.rs","content":"new a"},{"path":"src/b.rs","content":"new b"}]}"#;
        assert_eq!(decode_proposal(answer,&sources).unwrap().len(),2);
        assert!(decode_proposal("some code",&sources).is_err());
        assert!(decode_proposal(r#"{"files":[{"path":"src/a.rs","content":"new"},{"path":"tests/accept.rs","content":"cheat"}]}"#,&sources).is_err());
        assert!(target_paths("src/a.rs",Some(vec!["src/a.rs".into(),"tests/accept.rs".into()])).is_err());
        assert!(target_paths("src/a.rs",Some(vec!["src/a.rs".into(),"src/a.rs".into()])).is_err());
    }

    #[tokio::test]
    async fn recovered_running_rsi_trial_is_durably_failed_on_restart(){
        let root=std::env::temp_dir().join(format!("dominion-rsi-recover-{}",Uuid::new_v4()));
        let data=root.join("data");
        let workspace=root.join("workspace");
        fs::create_dir_all(&data).await.unwrap();
        fs::create_dir_all(&workspace).await.unwrap();
        let id=Uuid::new_v4();
        let item=Improvement{
            id,parent:None,repository:"repo".into(),target_file:"src/lib.rs".into(),
            target_files:vec![],attempts:0,
            instruction:"repair behavior".into(),test_command:"true".into(),
            score_command:None,status:ImprovementStatus::Running,
            base_commit:None,candidate_commit:None,
            baseline:None,candidate:None,proposal:None,reflection:None,
            accepted:false,cost_krw:0.0,error:None,created_at_ms:now_ms(),updated_at_ms:now_ms()
        };
        persist_improvement(&data,&item).await.unwrap();
        let first=Improvements::open(data.clone(),workspace.clone()).await.unwrap();
        let recovered=first.get(id).await.unwrap();
        assert_eq!(recovered.status,ImprovementStatus::Failed);
        assert!(recovered.error.as_deref().unwrap().contains("outcome unknown"));
        drop(first);
        let persisted:Improvement=serde_json::from_slice(&fs::read(data.join(format!("{id}.json"))).await.unwrap()).unwrap();
        assert_eq!(persisted.status,ImprovementStatus::Failed);
        let second=Improvements::open(data.clone(),workspace).await.unwrap();
        assert_eq!(second.get(id).await.unwrap().status,ImprovementStatus::Failed);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn failed_snapshot_staging_never_replaces_existing_trial(){
        let root=std::env::temp_dir().join(format!("dominion-rsi-write-{}",Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let id=Uuid::new_v4();
        let original=Improvement{
            id,parent:None,repository:"repo".into(),target_file:"file.txt".into(),
            target_files:vec![],attempts:0,
            instruction:"preserve state".into(),test_command:"true".into(),
            score_command:None,status:ImprovementStatus::AwaitingApproval,
            base_commit:None,candidate_commit:None,baseline:None,candidate:None,
            proposal:None,reflection:None,accepted:false,cost_krw:0.0,
            error:None,created_at_ms:now_ms(),updated_at_ms:now_ms()
        };
        persist_improvement(&root,&original).await.unwrap();
        let persisted=fs::read(root.join(format!("{id}.json"))).await.unwrap();
        let blocked=root.join(format!("{}-{}.tmp",id,Uuid::new_v4()));
        fs::create_dir(&blocked).await.unwrap();
        // A destination outside a valid snapshot store cannot be replaced.
        let impossible=blocked.join("not-a-directory");
        assert!(persist_improvement(&impossible,&original).await.is_err());
        assert_eq!(fs::read(root.join(format!("{id}.json"))).await.unwrap(),persisted);
        fs::remove_dir_all(root).await.unwrap();
    }

}

#[cfg(test)]
mod full_cycle_tests {
    use super::*;
    use crate::config::Settings;
    use axum::{extract::State,routing::post,Json,Router};
    use serde_json::Value;

    use std::sync::atomic::{AtomicUsize,Ordering};
    async fn repair_proposal(State(counter):State<Arc<AtomicUsize>>,Json(_request):Json<Value>)->Json<Value>{
        let turn=counter.fetch_add(1,Ordering::SeqCst);
        let files=if turn==0{
            json!({"files":[{"path":"score.txt","content":"0\n"},{"path":"helper.txt","content":"still-wrong\n"}]})
        }else{
            json!({"files":[{"path":"score.txt","content":"1\n"},{"path":"helper.txt","content":"ok\n"}]})
        };
        Json(json!({"choices":[{"message":{"role":"assistant","content":files.to_string()}}],
            "usage":{"prompt_tokens":10,"completion_tokens":20}}))
    }
    async fn failing_proposal(Json(_request):Json<Value>)->Json<Value>{
        let files=json!({"files":[{"path":"score.txt","content":"0\n"},
            {"path":"helper.txt","content":"still wrong\n"}]});
        Json(json!({"choices":[{"message":{"role":"assistant",
            "content":files.to_string()}}],
            "usage":{"prompt_tokens":5,"completion_tokens":10}}))
    }
    #[tokio::test]
    #[ignore = "explicit fixed-fixture shell test; run isolated with DOMINION_SHELL_SANDBOX=off"]
    async fn rejects_three_non_improving_candidates_without_adoption(){
        assert_eq!(std::env::var("DOMINION_SHELL_SANDBOX").unwrap(),"off");
        let root=std::env::temp_dir().join(format!("dominion-rsi-rejected-{}",Uuid::new_v4()));
        let repo=root.join("workspace").join("repo");
        fs::create_dir_all(&repo).await.unwrap();
        git(&repo,&["init","-q"]).await.unwrap();
        fs::write(repo.join("score.txt"),"0\n").await.unwrap();
        fs::write(repo.join("helper.txt"),"wrong\n").await.unwrap();
        git(&repo,&["add","score.txt","helper.txt"]).await.unwrap();
        execute("git",&["-c","user.name=Test","-c","user.email=test@example.com",
            "commit","-m","base"],&repo,30).await.unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/chat/completions",post(failing_proposal))).await.unwrap();
        });
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{addr}");settings.main.api_key.clear();
        let service=Arc::new(Improvements::open(root.join("state"),root.join("workspace")).await.unwrap());
        let item=service.create(NewImprovement{
            repository:"repo".into(),target_file:"score.txt".into(),
            target_files:Some(vec!["score.txt".into(),"helper.txt".into()]),
            instruction:"improve helper".into(),test_command:"test \"$(cat helper.txt)\" = ok".into(),
            score_command:Some("cat score.txt".into()),parent:None
        }).await.unwrap();
        service.start(ModelClient::new(&settings).unwrap(),item.id,true).await.unwrap();
        let mut terminal=None;
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let snapshot=service.get(item.id).await.unwrap();
            if snapshot.status!=ImprovementStatus::Running{terminal=Some(snapshot);break;}
        }
        let rejected=terminal.expect("negative RSI trial timed out");
        assert_eq!(rejected.status,ImprovementStatus::Rejected,"{:?}",rejected.error);
        assert_eq!(rejected.attempts,3);
        assert!(!rejected.accepted);
        assert!(service.adopt(item.id).await.is_err());
        assert_eq!(fs::read_to_string(repo.join("helper.txt")).await.unwrap(),"wrong\n");
        assert!(git(&repo,&["rev-parse",&format!("refs/dominion/rsi/{}",item.id)]).await.is_err());
        server.abort();
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "explicit fixed-fixture shell test; run isolated with DOMINION_SHELL_SANDBOX=off"]
    async fn repairs_multifile_candidate_against_separate_frozen_judge(){
        assert_eq!(std::env::var("DOMINION_SHELL_SANDBOX").as_deref(),Ok("off"));
        let root=std::env::temp_dir().join(format!("dominion-rsi-multifile-{}",Uuid::new_v4()));
        let workspace=root.join("workspace");let repo=workspace.join("repo");
        fs::create_dir_all(&repo).await.unwrap();
        git(&repo,&["init","-q"]).await.unwrap();
        fs::write(repo.join("score.txt"),"0\n").await.unwrap();
        fs::write(repo.join("helper.txt"),"wrong\n").await.unwrap();
        git(&repo,&["add","score.txt","helper.txt"]).await.unwrap();
        execute("git",&["-c","user.name=Test","-c","user.email=test@example.com",
            "commit","-m","base"],&repo,30).await.unwrap();
        let counter=Arc::new(AtomicUsize::new(0));
        let fixture=Arc::clone(&counter);
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/chat/completions",post(repair_proposal))
                .with_state(fixture)).await.unwrap();
        });
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{addr}");settings.main.api_key.clear();
        let service=Arc::new(Improvements::open(root.join("state"),workspace.clone()).await.unwrap());
        let input=NewImprovement{
            repository:"repo".into(),target_file:"score.txt".into(),
            target_files:Some(vec!["score.txt".into(),"helper.txt".into()]),
            instruction:"repair helper and improve score".into(),
            test_command:"test \"$(cat helper.txt)\" = ok".into(),
            score_command:Some("cat score.txt".into()),parent:None,
        };
        let created=service.create(input).await.unwrap();
        service.start(ModelClient::new(&settings).unwrap(),created.id,true).await.unwrap();
        let mut completed=None;
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let snapshot=service.get(created.id).await.unwrap();
            if snapshot.status!=ImprovementStatus::Running {completed=Some(snapshot);break;}
        }
        let evaluated=completed.expect("multi-file RSI did not finish");
        assert_eq!(evaluated.status,ImprovementStatus::Evaluated,"{:?}",evaluated.error);
        assert_eq!(counter.load(Ordering::SeqCst),2,"independent failed judge must trigger second proposal");
        assert_eq!(evaluated.attempts,2);
        assert!(evaluated.accepted);
        assert_eq!(evaluated.candidate.as_ref().unwrap().score,Some(1.0));
        assert_eq!(fs::read_to_string(repo.join("score.txt")).await.unwrap(),"0\n");
        assert_eq!(fs::read_to_string(repo.join("helper.txt")).await.unwrap(),"wrong\n");
        let pinned=git(&repo,&["rev-parse",&format!("refs/dominion/rsi/{}",created.id)]).await.unwrap();
        assert_eq!(Some(pinned),evaluated.candidate_commit);
        for _ in 0..100 {
            if !service.active.lock().await.contains(&created.id){break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let adopted=service.adopt(created.id).await.unwrap();
        assert_eq!(adopted.status,ImprovementStatus::Adopted);
        assert_eq!(fs::read_to_string(repo.join("score.txt")).await.unwrap(),"1\n");
        assert_eq!(fs::read_to_string(repo.join("helper.txt")).await.unwrap(),"ok\n");
        let descendant=service.descendant(created.id).await.unwrap();
        assert_eq!(descendant.target_files,vec!["score.txt","helper.txt"]);
        assert_eq!(descendant.parent,Some(created.id));
        server.abort();
        fs::remove_dir_all(root).await.unwrap();
    }

    async fn proposal(Json(_request):Json<Value>)->Json<Value>{
        Json(json!({"choices":[{"message":{"role":"assistant","content":"1\n"}}],"usage":{"prompt_tokens":5,"completion_tokens":1}}))
    }
    #[tokio::test]
    #[ignore = "explicit fixed-fixture shell test; run isolated with DOMINION_SHELL_SANDBOX=off"]
    async fn adopts_only_a_scored_improvement_and_inherits_reflection() {
        let git_status=Command::new("git").arg("--version").output().await.expect("Git is required for RSI");
        assert!(git_status.status.success(),"Git must be executable");
        let root=std::env::temp_dir().join(format!("dominion-improvement-{}",Uuid::new_v4()));
        let workspace=root.join("workspace");
        let repo=workspace.join("repo");
        fs::create_dir_all(&repo).await.unwrap();
        assert_eq!(std::env::var("DOMINION_SHELL_SANDBOX").as_deref(),Ok("off"),
            "this isolated CI fixture must explicitly opt in to local fixed commands");
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
            test_command:"test -f score.txt".into(),score_command:Some("cat score.txt".into()),parent:None,target_files:None
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
