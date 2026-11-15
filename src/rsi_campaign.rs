//! Durable control plane for original-main RSI campaign screen.
//! Actual source changes, scoring and adoption are delegated only to the
//! already independent Improvements evaluator and its frozen Git worktrees.
use crate::{domain::now_ms,improvement::{Improvement,ImprovementStatus}};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},path::PathBuf};
use tokio::{fs,io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Attempt {
    pub trial_id:Uuid,
    pub attempt:usize,
    pub accepted:bool,
    pub selected:Option<String>,
    pub domain:String,
    pub generator_commit:Option<String>,
    pub accepted_commit:Option<String>,
    pub candidate_commit:Option<String>,
    pub focus_files:Vec<String>,
    pub error:Option<String>,
    pub baseline_score:Option<f64>,
    pub candidate_score:Option<f64>,
    pub model_calls:usize,
    pub tokens:u64,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Campaign {
    pub id:Uuid,
    pub status:String,
    pub phase:String,
    pub objective:String,
    pub provider_id:String,
    pub model:String,
    pub selection_policy:String,
    pub repository:String,
    pub test_command:String,
    pub score_command:String,
    pub max_attempts:usize,
    pub token_budget:u64,
    pub time_budget_seconds:u64,
    pub started_ms:u64,
    pub attempts:usize,
    pub accepted_generations:usize,
    pub tokens:u64,
    pub cost_krw:f64,
    pub current_trial:Option<Uuid>,
    pub trials:Vec<Uuid>,
    pub history:Vec<Attempt>,
    pub base_commit:Option<String>,
    pub head_commit:Option<String>,
    pub error:Option<String>,
    pub stop_reason:Option<String>,
    pub requested_cancel:bool,
}
impl Campaign {
    pub fn view(&self)->Value{
        let selected=self.history.last().and_then(|v|v.selected.as_deref());
        let candidate=self.history.last();
        let comparison=if self.phase=="waiting_for_choice"{
            candidate.map(|row|json!({
                "attempt":row.attempt,"candidateId":row.trial_id,
                "winner":if row.accepted{"candidate"}else{"baseline"},
                "canSelectCandidate":row.accepted,
                "selected":selected,
                "metrics":{"pairedCases":1,"regressions":0,
                    "measurements":[{"id":"native-frozen-judge","metrics":[]}]}
            }))
        }else{None};
        json!({
            "id":self.id,"status":self.status,"phase":self.phase,
            "objective":self.objective,"providerId":self.provider_id,"model":self.model,
            "attempts":self.attempts,"acceptedGenerations":self.accepted_generations,
            "tokens":self.tokens,"tokensEstimated":true,
            "costKrw":self.cost_krw,
            "modelCalls":self.history.iter().map(|a|a.model_calls).sum::<usize>(),
            "candidateCount":self.history.iter().filter(|a|a.candidate_commit.is_some()).count(),
            "verifiedSourceWrites":self.history.iter().filter(|a|a.candidate_commit.is_some()).count(),
            "systemSourceWrites":self.history.iter().map(|a|a.focus_files.len()).sum::<usize>(),
            "evaluationEpisodes":self.history.len(),
            "baseCommit":self.base_commit,"headCommit":self.head_commit,
            "sourceSnapshot":false,
            "selectedVersionRunning":self.head_commit.as_deref()
                .is_some_and(|sha|Some(sha)==option_env!("DOMINION_BUILD_COMMIT")),
            "error":self.error,"stopReason":self.stop_reason,
            "selectionPolicy":self.selection_policy,
            "limits":{"maxAttempts":self.max_attempts,
                "tokenBudget":self.token_budget,"timeBudgetSeconds":self.time_budget_seconds},
            "comparison":comparison,
            "activeWorker":self.current_trial.map(|id|json!({"id":id,
                "phase":self.phase,"focusFiles":candidate.map(|c|c.focus_files.clone())})),
            "history":self.history.iter().map(|h|json!({
                "attempt":h.attempt,
                "accepted":h.accepted && h.selected.as_deref()==Some("candidate"),
                "selection":h.selected,
                "domain":h.domain,"newFindings":0,
                "generatorCommit":h.generator_commit,"acceptedCommit":h.accepted_commit,
                "focusFiles":h.focus_files,
                "trialOutcome":{"status":if h.accepted{"evaluated"}else{"rejected"},
                    "verdict":if h.accepted{"independent_judge_pass"}else{"not_accepted"}},
                "error":h.error
            })).collect::<Vec<_>>(),
        })
    }
}
pub fn from_trial(trial:&Improvement,number:usize)->Attempt{
    Attempt{
        trial_id:trial.id,attempt:number,accepted:trial.accepted,
        selected:None,domain:trial.selection_reason.clone()
            .unwrap_or_else(||trial.target_file.clone()),
        generator_commit:trial.base_commit.clone(),accepted_commit:None,
        candidate_commit:trial.candidate_commit.clone(),
        focus_files:trial.target_files.clone(),error:trial.error.clone(),
        baseline_score:trial.baseline.as_ref().and_then(|m|m.score),
        candidate_score:trial.candidate.as_ref().and_then(|m|m.score),
        model_calls:trial.attempts,tokens:trial.input_tokens.saturating_add(trial.output_tokens)
    }
}
#[derive(Clone,Default,Serialize,Deserialize)]
struct Snapshot{campaigns:HashMap<Uuid,Campaign>}
pub struct Campaigns{
    file:PathBuf,state:Mutex<Snapshot>,active:Mutex<HashSet<Uuid>>
}
impl Campaigns{
    pub fn open(root:PathBuf)->Result<Self,String>{
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("rsi-campaigns.json");
        let mut state=if file.exists(){
            serde_json::from_slice::<Snapshot>(&std::fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("invalid RSI campaigns state: {e}"))?
        }else{Snapshot::default()};
        let mut changed=false;
        for campaign in state.campaigns.values_mut(){
            if matches!(campaign.status.as_str(),"running"|"queued"){
                campaign.status="needs_review".into();
                campaign.error=Some("Server restarted: reconcile the saved trial before continuing".into());
                changed=true;
            }
        }
        if changed{
            let bytes=serde_json::to_vec(&state).map_err(|e|e.to_string())?;
            let temp=root.join(format!(".rsi-recovery-{}.tmp",Uuid::new_v4()));
            let result=(||->Result<(),String>{
                use std::io::Write;
                let mut f=std::fs::OpenOptions::new().write(true).create_new(true)
                    .open(&temp).map_err(|e|e.to_string())?;
                f.write_all(&bytes).map_err(|e|e.to_string())?;
                f.sync_all().map_err(|e|e.to_string())?;
                std::fs::rename(&temp,&file).map_err(|e|e.to_string())?;
                #[cfg(unix)]std::fs::File::open(&root)
                    .and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
                Ok(())
            })();
            if result.is_err(){let _=std::fs::remove_file(temp);}
            result?;
        }
        Ok(Self{file,state:Mutex::new(state),active:Mutex::new(HashSet::new())})
    }
    async fn persist(&self,state:&Snapshot)->Result<(),String>{
        let data=serde_json::to_vec(state).map_err(|e|e.to_string())?;
        if data.len()>16*1024*1024{return Err("RSI campaigns exceed 16 MiB".into());}
        let parent=self.file.parent().ok_or("invalid RSI root")?;
        let temp=parent.join(format!(".rsi-campaigns-{}.tmp",Uuid::new_v4()));
        let outcome=async{
            let mut stage=fs::OpenOptions::new().write(true).create_new(true)
                .open(&temp).await.map_err(|e|e.to_string())?;
            stage.write_all(&data).await.map_err(|e|e.to_string())?;
            stage.sync_all().await.map_err(|e|e.to_string())?;
            fs::rename(&temp,&self.file).await.map_err(|e|e.to_string())?;
            #[cfg(unix)]{
                let root=parent.to_path_buf();
                tokio::task::spawn_blocking(move||std::fs::File::open(root)
                    .and_then(|file|file.sync_all())).await
                    .map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
            }
            Ok::<(),String>(())
        }.await;
        if outcome.is_err(){let _=fs::remove_file(temp).await;}
        outcome
    }
    pub async fn change<F>(&self,id:Uuid,mutate:F)->Result<Campaign,String>
    where F:FnOnce(&mut Campaign)->Result<(),String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        let item=next.campaigns.get_mut(&id).ok_or("RSI campaign not found")?;
        mutate(item)?;
        let updated=item.clone();
        self.persist(&next).await?;
        *guard=next;
        Ok(updated)
    }
    pub async fn create(&self,item:Campaign)->Result<Campaign,String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        if next.campaigns.len()>=128{return Err("maximum 128 RSI campaigns".into());}
        if next.campaigns.values().any(|existing|
            matches!(existing.status.as_str(),"queued"|"running")
            ||existing.phase=="waiting_for_choice"){
            return Err("another RSI campaign is active or awaiting version selection".into());
        }
        next.campaigns.insert(item.id,item.clone());
        self.persist(&next).await?;
        *guard=next;
        Ok(item)
    }
    pub async fn get(&self,id:Uuid)->Option<Campaign>{
        self.state.lock().await.campaigns.get(&id).cloned()
    }
    pub async fn list(&self)->Vec<Campaign>{
        let mut results=self.state.lock().await.campaigns.values().cloned().collect::<Vec<_>>();
        results.sort_by(|a,b|b.started_ms.cmp(&a.started_ms));results
    }
    pub async fn reserve(&self,id:Uuid)->Result<(),String>{
        let mut guard=self.active.lock().await;
        if !guard.insert(id){return Err("RSI campaign worker already running".into());}
        Ok(())
    }
    pub async fn release(&self,id:Uuid){self.active.lock().await.remove(&id);}
    pub async fn active(&self,id:Uuid)->bool{self.active.lock().await.contains(&id)}
}
