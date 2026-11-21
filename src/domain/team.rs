//! Versioned, owner-authored team DAG persisted inside a native Run.
//! Pure graph operations do not spawn agents, execute tools or grant access.
use serde::{Deserialize,Serialize};
use std::collections::{HashMap,HashSet};
use uuid::Uuid;
use super::{Run,RunStatus};

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum TeamRole{Investigator,Writer,Reviewer}
impl TeamRole{
    pub fn read_only(self)->bool{!matches!(self,Self::Writer)}
    pub fn profile(self)->&'static str{match self{
        Self::Investigator=>"inspection",Self::Writer=>"implementation",
        Self::Reviewer=>"verification"
    }}
}
/// Owner-authored, optional post-run evidence requirements. A worker's
/// natural-language claim of completion never satisfies a receipt check.
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum TaskAcceptanceCheck{
    /// Requires an actual successful receipt from the single native harness.
    SuccessfulTool{name:String},
    /// Requires an exact PASS receipt from the configured model auditor.
    /// A plain "completed" Run or model final answer is insufficient.
    AuditorPassed,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TaskSpec{
    pub id:String,
    pub instruction:String,
    #[serde(default)]pub depends_on:Vec<String>,
    pub workspace:String,
    pub role:TeamRole,
    pub budget_krw:f64,
    pub max_steps:usize,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub acceptance_checks:Vec<TaskAcceptanceCheck>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TeamTask{
    #[serde(flatten)]pub spec:TaskSpec,
    /// Deterministic per-team work assignment, allocated BEFORE any model
    /// invocation, survives failed launches and process restarts.
    pub run_id:Uuid,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TeamGraph{
    pub schema_version:u32,
    pub tasks:Vec<TeamTask>,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum TeamTaskState{Pending,Ready,Active,ApprovalRequired,Blocked,Completed,Failed,Cancelled}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TeamTaskView{
    pub id:String,
    pub run_id:Uuid,
    pub role:TeamRole,
    pub depends_on:Vec<String>,
    pub workspace:String,
    pub status:TeamTaskState,
    /// Actual completed Writer with verified artifact waiting for a SHA-256
    /// owner patch review. Do not conflate tool permission prompts with
    /// completed Git changes awaiting adoption.
    #[serde(default)]pub patch_review_ready:bool,
    /// The persisted owner-selected provider/model route (if bound).
    #[serde(default,skip_serializing_if="Option::is_none")]pub provider_id:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]pub model_name:Option<String>,
    /// Empty for accepted or not-yet-finished workers. Only real receipts
    /// are used; unfulfilled requirements block dependent tasks.
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub unmet_acceptance:Vec<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TeamView{
    pub root_run_id:Uuid,
    pub tasks:Vec<TeamTaskView>,
    pub completed:usize,
    pub all_complete:bool,
    pub failed:bool,
}

impl TeamGraph{
    pub fn new(specs:Vec<TaskSpec>,budget_krw:f64)->Result<Self,String>{
        if specs.is_empty()||specs.len()>16{return Err("team needs 1..16 tasks".into());}
        if !budget_krw.is_finite()||!(0.0..=1_000_000.0).contains(&budget_krw){
            return Err("invalid team total budget".into());
        }
        let mut names=HashSet::new();
        let mut allocated=0.0;
        for task in &specs{
            let ident=|s:&str|!s.is_empty()&&s.len()<=64
                &&s.bytes().all(|x|x.is_ascii_alphanumeric()||x==b'_'||x==b'-');
            if !ident(&task.id)||!names.insert(task.id.clone()){
                return Err("team task IDs must be unique ASCII identifiers".into());
            }
            if task.instruction.trim().is_empty()||task.instruction.len()>20_000
                ||task.workspace.trim().is_empty()||task.workspace.len()>160
                ||task.workspace.contains('\0') || task.workspace.contains(".."){
                return Err("invalid task instruction or workspace scope".into());
            }
            if !(1..=100).contains(&task.max_steps)
                ||!task.budget_krw.is_finite()||task.budget_krw<0.0{
                return Err("invalid team task step or budget cap".into());
            }
            if task.acceptance_checks.len()>8 {
                return Err("at most 8 team task acceptance checks".into());
            }
            let mut seen_checks=HashSet::new();
            for check in &task.acceptance_checks{
                let key=match check{
                    TaskAcceptanceCheck::SuccessfulTool{name}=>{
                        if name.is_empty()||name.len()>128||!name.bytes().all(|b|
                            b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'|b'.'|b':')){
                            return Err("invalid team acceptance tool name".into());
                        }
                        format!("tool:{name}")
                    }
                    TaskAcceptanceCheck::AuditorPassed=>"auditor:pass".into()
                };
                if !seen_checks.insert(key){return Err("duplicate team acceptance check".into());}
            }
            allocated+=task.budget_krw;
            if !allocated.is_finite(){return Err("invalid team budget sum".into());}
        }
        if allocated>budget_krw+0.000001{
            return Err("team task reservations exceed owner budget".into());
        }
        let by_id=specs.iter().enumerate()
            .map(|(i,t)|(t.id.as_str(),i)).collect::<HashMap<_,_>>();
        for task in &specs{
            let mut deps=HashSet::new();
            for dep in &task.depends_on{
                if dep==&task.id||!by_id.contains_key(dep.as_str())||!deps.insert(dep){
                    return Err("invalid, duplicate or self task dependency".into());
                }
            }
        }
        fn visit(i:usize,specs:&[TaskSpec],by_id:&HashMap<&str,usize>,
            marks:&mut [u8])->Result<(),String>{
            if marks[i]==1{return Err("team task dependencies form a cycle".into());}
            if marks[i]==2{return Ok(());}
            marks[i]=1;
            for dep in &specs[i].depends_on{
                visit(by_id[dep.as_str()],specs,by_id,marks)?;
            }
            marks[i]=2;Ok(())
        }
        let mut marks=vec![0;specs.len()];
        for i in 0..specs.len(){visit(i,&specs,&by_id,&mut marks)?;}
        Ok(Self{schema_version:1,tasks:specs.into_iter().map(|spec|
            TeamTask{spec,run_id:Uuid::new_v4()}).collect()})
    }
    /// Pure proof evaluation: never infer a tool call from an answer, a
    /// requested tool call, an error receipt or a failed reconciliation.
    pub fn unmet_acceptance(task:&TeamTask,run:&Run)->Vec<String>{
        if run.status!=RunStatus::Completed{
            return Vec::new();
        }
        task.spec.acceptance_checks.iter().filter_map(|check|match check{
            TaskAcceptanceCheck::SuccessfulTool{name}=>{
                let observed=run.events.iter().any(|event|{
                    if !matches!(event.kind.as_str(),"tool_result"|"tool_reconciled"){
                        return false;
                    }
                    let Some(detail)=event.detail.as_ref() else{return false};
                    if detail.get("name").and_then(serde_json::Value::as_str)!=Some(name){
                        return false;
                    }
                    match detail.get("succeeded").and_then(serde_json::Value::as_bool){
                        Some(true)=>true,Some(false)=>false,
                        None=>event.kind=="tool_result", // legacy read batch
                    }
                });
                (!observed).then(||format!("successful_tool:{name}"))
            }
            TaskAcceptanceCheck::AuditorPassed=>{
                (!run.events.iter().any(|event|event.kind=="verification"
                    &&event.message.trim()=="PASS")).then(||"auditor_passed".into())
            }
        }).collect()
    }
    /// A read-only worker may run in parallel. A writer holds a named
    /// workspace lease until terminal; no parallel writer can claim it.
    /// Dependencies must be completed, never merely queued or paused.
    pub fn view(&self,root:Uuid,states:&HashMap<Uuid,RunStatus>)->TeamView{
        self.view_with_acceptance(root,states,&HashMap::new())
    }
    pub fn view_with_acceptance(&self,root:Uuid,states:&HashMap<Uuid,RunStatus>,
        unmet:&HashMap<Uuid,Vec<String>>)->TeamView{
        self.view_with_repository_lease(root,states,unmet,false)
    }
    /// A repository-bound Team shares ONE integration target even when
    /// writers declare distinct logical workspace labels. Until an earlier
    /// patch is accepted or rejected, running another writer from the same
    /// base commit creates needless merge conflicts and stale evidence.
    /// Read-only investigators and reviewers remain parallelizable.
    pub fn view_with_repository_lease(&self,root:Uuid,
        states:&HashMap<Uuid,RunStatus>,unmet:&HashMap<Uuid,Vec<String>>,
        shared_repository:bool)->TeamView{
        let by_id=self.tasks.iter().map(|t|(t.spec.id.as_str(),t.run_id))
            .collect::<HashMap<_,_>>();
        let mut active_writers=HashSet::new();
        for task in &self.tasks{
            if task.spec.role==TeamRole::Writer &&
                states.get(&task.run_id).is_some_and(|s|!matches!(s,
                    RunStatus::Completed|RunStatus::Failed|RunStatus::Cancelled)){
                let lease=if shared_repository{"__shared_git_repository__"}
                    else{task.spec.workspace.as_str()};
                active_writers.insert(lease);
            }
        }
        let mut claimed=active_writers;
        let mut tasks=Vec::with_capacity(self.tasks.len());
        for task in &self.tasks{
            let done=task.spec.depends_on.iter().all(|dep|
                states.get(&by_id[dep.as_str()])==Some(&RunStatus::Completed));
            let failed_dependency=task.spec.depends_on.iter().any(|dep|
                states.get(&by_id[dep.as_str()]).is_some_and(|status|matches!(status,
                    RunStatus::Failed|RunStatus::Cancelled)));
            let status=match states.get(&task.run_id){
                Some(RunStatus::Completed)=>TeamTaskState::Completed,
                Some(RunStatus::Failed)=>TeamTaskState::Failed,
                Some(RunStatus::Cancelled)=>TeamTaskState::Cancelled,
                Some(RunStatus::AwaitingApproval)=>TeamTaskState::ApprovalRequired,
                Some(RunStatus::Paused)=>TeamTaskState::Blocked,
                Some(_)=>TeamTaskState::Active,
                None if failed_dependency=>TeamTaskState::Blocked,
                None if !done=>TeamTaskState::Pending,
                None if task.spec.role==TeamRole::Writer &&
                    !claimed.insert(if shared_repository{"__shared_git_repository__"}
                        else{task.spec.workspace.as_str()})=>TeamTaskState::Pending,
                None=>TeamTaskState::Ready,
            };
            tasks.push(TeamTaskView{
                id:task.spec.id.clone(),run_id:task.run_id,role:task.spec.role,
                depends_on:task.spec.depends_on.clone(),
                workspace:task.spec.workspace.clone(),status,patch_review_ready:false,
                provider_id:None,model_name:None,
                unmet_acceptance:unmet.get(&task.run_id).cloned().unwrap_or_default(),
            });
        }
        let completed=tasks.iter().filter(|t|t.status==TeamTaskState::Completed).count();
        let failed=tasks.iter().any(|t|matches!(t.status,
            TeamTaskState::Failed|TeamTaskState::Cancelled));
        TeamView{root_run_id:root,all_complete:completed==tasks.len(),
            failed,completed,tasks}
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn item(id:&str,depends_on:&[&str],role:TeamRole)->TaskSpec{
        TaskSpec{id:id.into(),instruction:format!("Do {id}"),
            depends_on:depends_on.iter().map(|s|s.to_string()).collect(),
            workspace:"repo-a".into(),role,budget_krw:100.0,max_steps:5,
            acceptance_checks:Vec::new()}
    }
    #[test]fn rejects_cycles_bad_references_and_unfunded_graphs(){
        assert!(TeamGraph::new(vec![
            item("a",&["b"],TeamRole::Writer),
            item("b",&["a"],TeamRole::Reviewer)],500.0).is_err());
        assert!(TeamGraph::new(vec![item("a",&["missing"],TeamRole::Writer)],500.0).is_err());
        assert!(TeamGraph::new(vec![
            item("a",&[],TeamRole::Writer),item("b",&[],TeamRole::Writer)],199.0).is_err());
    }
    #[test]fn dependencies_and_writer_leases_are_independent(){
        let graph=TeamGraph::new(vec![
            item("research",&[],TeamRole::Investigator),
            item("writer1",&["research"],TeamRole::Writer),
            item("writer2",&["research"],TeamRole::Writer),
            item("verify",&["writer1","writer2"],TeamRole::Reviewer)],500.0).unwrap();
        let root=Uuid::new_v4();let mut states=HashMap::new();
        let a=graph.view(root,&states);
        assert_eq!(a.tasks.iter().filter(|t|t.status==TeamTaskState::Ready).count(),1);
        states.insert(graph.tasks[0].run_id,RunStatus::Completed);
        let a=graph.view(root,&states);
        assert_eq!(a.tasks[1].status,TeamTaskState::Ready);
        assert_eq!(a.tasks[2].status,TeamTaskState::Pending);
        states.insert(graph.tasks[1].run_id,RunStatus::AwaitingApproval);
        let b=graph.view(root,&states);
        assert_eq!(b.tasks[1].status,TeamTaskState::ApprovalRequired);
        assert_eq!(b.tasks[2].status,TeamTaskState::Pending);
        states.insert(graph.tasks[1].run_id,RunStatus::Completed);
        let b=graph.view(root,&states);
        assert_eq!(b.tasks[2].status,TeamTaskState::Ready);
        states.insert(graph.tasks[2].run_id,RunStatus::Failed);
        assert_eq!(graph.view(root,&states).tasks[3].status,TeamTaskState::Blocked);
    }
    #[test]fn repository_writer_lease_serializes_distinct_labels_until_patch_adoption(){
        let mut first=item("writer_a",&[],TeamRole::Writer);
        let mut second=item("writer_b",&[],TeamRole::Writer);
        first.workspace="frontend".into();
        second.workspace="backend".into();
        let graph=TeamGraph::new(vec![
            first,second,item("research",&[],TeamRole::Investigator)],300.0).unwrap();
        let root=Uuid::new_v4();
        let mut states=HashMap::new();
        let empty=HashMap::new();
        // An unbound Team can schedule independent named workspaces.
        let independent=graph.view_with_acceptance(root,&states,&empty);
        assert_eq!(independent.tasks[0].status,TeamTaskState::Ready);
        assert_eq!(independent.tasks[1].status,TeamTaskState::Ready);
        // A single Git repository has one shared patch-integration lease.
        let initial=graph.view_with_repository_lease(root,&states,&empty,true);
        assert_eq!(initial.tasks[0].status,TeamTaskState::Ready);
        assert_eq!(initial.tasks[1].status,TeamTaskState::Pending);
        assert_eq!(initial.tasks[2].status,TeamTaskState::Ready);
        states.insert(graph.tasks[0].run_id,RunStatus::AwaitingApproval);
        let pending=graph.view_with_repository_lease(root,&states,&empty,true);
        assert_eq!(pending.tasks[1].status,TeamTaskState::Pending);
        states.insert(graph.tasks[0].run_id,RunStatus::Completed);
        let released=graph.view_with_repository_lease(root,&states,&empty,true);
        assert_eq!(released.tasks[1].status,TeamTaskState::Ready);
    }
    #[test]fn acceptance_requires_actual_successful_receipt_and_explicit_audit(){
        let mut spec=item("research",&[],TeamRole::Investigator);
        spec.acceptance_checks=vec![
            TaskAcceptanceCheck::SuccessfulTool{name:"read_file".into()},
            TaskAcceptanceCheck::AuditorPassed,
        ];
        let graph=TeamGraph::new(vec![spec],100.0).unwrap();
        let task=&graph.tasks[0];
        let mut run=Run::new(super::super::CreateRun{
            instruction:"Inspect".into(),budget_krw:Some(100.0),max_steps:Some(2)},2).unwrap();
        run.status=RunStatus::Completed;
        run.answer=Some("I called read_file and passed audit".into());
        assert_eq!(TeamGraph::unmet_acceptance(task,&run).len(),2);
        run.event("tool_error","read_file failed",Some(serde_json::json!({
            "name":"read_file","succeeded":false})));
        run.event("tool_reconciled","read_file failed reconciliation",
            Some(serde_json::json!({"name":"read_file","succeeded":false})));
        assert_eq!(TeamGraph::unmet_acceptance(task,&run).len(),2);
        run.event("tool_result","read_file completed",Some(serde_json::json!({
            "name":"read_file","call_id":"genuine-1","succeeded":true})));
        assert_eq!(TeamGraph::unmet_acceptance(task,&run),
            vec!["auditor_passed".to_string()]);
        run.event("verification","PASS",None);
        assert!(TeamGraph::unmet_acceptance(task,&run).is_empty());
    }
    #[test]fn invalid_and_duplicate_acceptance_rules_are_rejected(){
        let mut spec=item("x",&[],TeamRole::Writer);
        spec.acceptance_checks=vec![
            TaskAcceptanceCheck::SuccessfulTool{name:"run_shell".into()},
            TaskAcceptanceCheck::SuccessfulTool{name:"run_shell".into()},
        ];
        assert!(TeamGraph::new(vec![spec.clone()],100.0).is_err());
        spec.acceptance_checks=vec![TaskAcceptanceCheck::SuccessfulTool{
            name:"../escape".into()}];
        assert!(TeamGraph::new(vec![spec],100.0).is_err());
    }
    #[test]fn graph_serialization_keeps_stable_run_assignment(){
        let g=TeamGraph::new(vec![item("a",&[],TeamRole::Investigator)],100.0).unwrap();
        let rest:TeamGraph=serde_json::from_slice(&serde_json::to_vec(&g).unwrap()).unwrap();
        assert_eq!(g.tasks[0].run_id,rest.tasks[0].run_id);
        assert_eq!(rest.schema_version,1);
    }
}
