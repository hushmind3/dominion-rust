//! Real native Run and team-DAG integration: no test-only coordinator.
use super::*;
use crate::{domain::team::{TaskSpec,TeamRole,TeamTaskState},
    sdk::{Dominion,RunOptions}};
use crate::store::Store;
use crate::engine::tests::DenyOnlyServices;
use crate::runtime_services::CapabilitySupport;
use std::time::Duration;

fn spec(id:&str,depends_on:&[&str],role:TeamRole)->TaskSpec{
    TaskSpec{
        id:id.to_string(),
        instruction:format!("Analyze and report: {id}"),
        depends_on:depends_on.iter().map(|x|x.to_string()).collect(),
        workspace:"isolated-repo".into(),role,budget_krw:30.0,max_steps:3,
        acceptance_checks:Vec::new(),
    }
}
async fn mock_engine()->(Arc<Engine>,Arc<Store>,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!("dominion-task-team-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.addons_enabled=false;
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let repo:Arc<dyn crate::ports::RunRepository>=store.clone();
    let mcp=Arc::new(crate::mcp::Mcp::new(Vec::new()).unwrap());
    let engine=Engine::new_with_runtime_services(settings,repo,groups,mcp,
        Arc::new(crate::algorithms::DefaultAlgorithm::default()),
        Arc::new(dominion_agent_algo::ReActLoop),
        Arc::new(DenyOnlyServices{tool_calling:CapabilitySupport::Supported})).unwrap();
    (engine,store,root)
}
#[tokio::test]
async fn durable_team_runs_real_workers_in_dependency_order_and_never_launches_root(){
    let (engine,store,path)=mock_engine().await;
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let root=sdk.create_task_team("Implement then review".into(),vec![
        spec("research",&[],TeamRole::Investigator),
        spec("implement",&["research"],TeamRole::Writer),
        spec("review",&["implement"],TeamRole::Reviewer),
    ],90.0,RunOptions::default()).await.unwrap();
    assert_eq!(root.status,RunStatus::Paused);
    assert_eq!(root.reserved_budget_krw,90.0);
    assert!(engine.launch(root.id).await.is_err(),
        "a team coordinator must never start the LLM loop");
    let graph=root.team.as_ref().unwrap();
    let ids=graph.tasks.iter().map(|t|t.run_id).collect::<Vec<_>>();
    assert_eq!(sdk.task_team(root.id).await.unwrap().tasks[0].status,TeamTaskState::Ready);
    for (idx,expected_role) in [TeamRole::Investigator,TeamRole::Writer,TeamRole::Reviewer]
        .into_iter().enumerate(){
        sdk.advance_task_team(root.id).await.unwrap();
        let child=sdk.get(ids[idx]).await.unwrap();
        assert_eq!(child.parent_run_id,Some(root.id));
        assert_eq!(child.read_only,expected_role.read_only());
        assert_eq!(child.delegation_profile,expected_role.profile());
        // Child completion automatically awakens the next DAG layer. The
        // next worker can legitimately start before this assertion runs.
        let settled=sdk.wait(ids[idx],Duration::from_secs(12)).await.unwrap();
        assert_eq!(settled.status,RunStatus::Completed,
            "the actual native harness must complete this worker");
        assert_eq!(settled.answer.as_deref(),Some("mocked model result"));
    }
    // The worker settlement event is async. Advance again to finalize only
    // after all child receipts have actually settled into the parent Run.
    let mut completed=false;
    for _ in 0..100{
        sdk.advance_task_team(root.id).await.unwrap();
        if sdk.get(root.id).await.unwrap().status==RunStatus::Completed{
            completed=true;break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(completed,"root must complete only after all child receipts settle");
    let final_root=sdk.get(root.id).await.unwrap();
    assert_eq!(final_root.settled_child_ids.len(),3);
    assert_eq!(final_root.reserved_budget_krw,0.0);
    let reopen=Store::open(path.join("data/runs")).await.unwrap();
    assert_eq!(reopen.get(root.id).await.unwrap().team.unwrap().tasks[1].run_id,ids[1]);
    drop(reopen);drop(sdk);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn cancellation_of_team_root_prevents_later_work_and_keeps_original_task_ids(){
    let (engine,store,path)=mock_engine().await;
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let root=sdk.create_task_team("Cancel this team".into(),vec![
        spec("writer",&[],TeamRole::Writer),
        spec("verify",&["writer"],TeamRole::Reviewer),
    ],60.0,RunOptions::default()).await.unwrap();
    let ids=root.team.as_ref().unwrap().tasks.iter().map(|t|t.run_id)
        .collect::<Vec<_>>();
    sdk.cancel(root.id).await.unwrap();
    assert_eq!(sdk.get(root.id).await.unwrap().status,RunStatus::Cancelled);
    sdk.advance_task_team(root.id).await.unwrap();
    assert!(store.get(ids[0]).await.is_none());
    assert!(store.get(ids[1]).await.is_none());
    assert_eq!(sdk.task_team(root.id).await.unwrap().tasks[0].run_id,ids[0]);
    drop(sdk);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn one_owner_tick_auto_completes_all_dag_layers_via_existing_wake_queue(){
    let (engine,store,path)=mock_engine().await;
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let root=sdk.create_task_team("Automatic DAG lifecycle".into(),vec![
        spec("research",&[],TeamRole::Investigator),
        spec("write",&["research"],TeamRole::Writer),
        spec("audit",&["write"],TeamRole::Reviewer),
    ],90.0,RunOptions::default()).await.unwrap();
    sdk.advance_task_team(root.id).await.unwrap(); // exactly ONE owner tick
    let mut terminal=false;
    for _ in 0..200 {
        if sdk.get(root.id).await.unwrap().status==RunStatus::Completed {
            terminal=true;break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let observed=sdk.get(root.id).await.unwrap();
    assert!(terminal,"settlement must signal next layer without manual SDK ticks");
    let graph=observed.team.as_ref().unwrap();
    let mut prior_end=0u64;
    for task in &graph.tasks{
        let child=store.get(task.run_id).await.expect("the DAG worker was never launched");
        assert_eq!(child.status,RunStatus::Completed);
        assert!(child.created_at_ms>=prior_end,
            "a dependency must finish before its dependant is created");
        prior_end=child.updated_at_ms;
        if !task.spec.depends_on.is_empty(){
            let handoff=child.events.iter().find(|e|e.kind=="team_dependency_handoff")
                .expect("successor must persist prior-worker evidence");
            let content=child.chat_context.first().and_then(|v|v["content"].as_str())
                .expect("source-linked handoff must be part of native model context");
            assert!(content.contains("source_run_id"));
            for parent_name in &task.spec.depends_on{
                let source=graph.tasks.iter().find(|t|&t.spec.id==parent_name).unwrap();
                assert!(content.contains(&source.run_id.to_string()));
            }
            assert!(handoff.detail.as_ref().unwrap()["evidence_chars"].as_u64().unwrap()>0);
            assert_eq!(child.chat_context[0]["role"],"assistant",
                "lower-trust worker output must not be promoted into owner role");
        }
    }
    assert_eq!(observed.events.iter().filter(|e|e.kind=="team_started").count(),1);
    assert_eq!(observed.events.iter().filter(|e|e.kind=="team_completed").count(),1);
    assert_eq!(observed.settled_child_ids.len(),3);
    drop(sdk);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn missing_owner_required_receipt_fails_team_and_never_launches_dependant(){
    use crate::domain::team::TaskAcceptanceCheck;
    let (engine,store,path)=mock_engine().await;
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let mut required=spec("research",&[],TeamRole::Investigator);
    required.acceptance_checks.push(TaskAcceptanceCheck::SuccessfulTool{
        name:"read_file".into()});
    let root=sdk.create_task_team("Gate evidence before writer".into(),vec![
        required,spec("write",&["research"],TeamRole::Writer)
    ],60.0,RunOptions::default()).await.unwrap();
    let ids=root.team.as_ref().unwrap().tasks.iter().map(|t|t.run_id)
        .collect::<Vec<_>>();
    sdk.advance_task_team(root.id).await.unwrap();
    let settled=sdk.wait(ids[0],Duration::from_secs(12)).await.unwrap();
    assert_eq!(settled.status,RunStatus::Completed,
        "the model may claim completion without an actual tool receipt");
    let mut failed=false;
    for _ in 0..200{
        if sdk.get(root.id).await.unwrap().status==RunStatus::Failed{
            failed=true;break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(failed,"the owner acceptance rule must fail closed");
    assert!(store.get(ids[1]).await.is_none(),
        "no downstream Writer may start from unsupported predecessor claims");
    let view=sdk.task_team(root.id).await.unwrap();
    assert_eq!(view.tasks[0].status,TeamTaskState::Failed);
    assert_eq!(view.tasks[0].unmet_acceptance,vec!["successful_tool:read_file"]);
    assert_eq!(view.tasks[1].status,TeamTaskState::Blocked);
    let persisted=sdk.get(root.id).await.unwrap();
    assert!(persisted.events.iter().any(|e|e.kind=="team_failed"
        &&e.detail.as_ref().is_some_and(|v|v["unmet_acceptance"]
            .as_array().is_some_and(|a|!a.is_empty()))));
    drop(sdk);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn repository_team_requires_review_and_digest_approval_before_reviewer_sees_code(){
    use crate::domain::{Run,CreateRun};
    let (engine,store,path)=mock_engine().await;
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let source=engine.settings.workspace_dir.join("projects").join("team-demo");
    tokio::fs::create_dir_all(&source).await.unwrap();
    async fn git(dir:&std::path::Path,args:&[&str]){
        let output=tokio::process::Command::new("git").args(args)
            .current_dir(dir).output().await.unwrap();
        assert!(output.status.success(),"git command failed: {}",
            String::from_utf8_lossy(&output.stderr));
    }
    git(&source,&["init","-q"]).await;
    tokio::fs::write(source.join("main.rs"),"fn main() {}\n").await.unwrap();
    git(&source,&["add","main.rs"]).await;
    git(&source,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
        "commit","-qm","base"]).await;

    let root=sdk.create_task_team_from_repository("Patch then review".into(),vec![
        spec("writer",&[],TeamRole::Writer),
        spec("review",&["writer"],TeamRole::Reviewer),
    ],60.0,RunOptions::default(),"projects/team-demo").await.unwrap();
    let graph=root.team.as_ref().unwrap();
    let writer_id=graph.tasks[0].run_id;
    let reviewer_id=graph.tasks[1].run_id;
    let original=root.base_commit.as_ref().unwrap().clone();
    crate::workspace::attach_at(&engine.settings,writer_id,
        "projects/team-demo",Some(&original)).await.unwrap();

    // Simulate a genuinely completed native Writer and its independent
    // artifact gate: no artificial user approval, test bypass or fake patch.
    let mut writer=Run::new(CreateRun{instruction:"Edit main.rs".into(),
        budget_krw:Some(30.0),max_steps:Some(3)},3).unwrap();
    writer.id=writer_id;
    writer.parent_run_id=Some(root.id);
    writer.status=RunStatus::Completed;
    writer.repository=Some("projects/team-demo".into());
    writer.base_commit=Some(original.clone());
    writer.delegation_profile="implementation".into();
    writer.answer=Some("Changed main.rs".into());
    writer.event("artifact_evidence_checked","Real Git diff inspected",
        Some(json!({"inspected":true,"satisfied":true})));
    tokio::fs::write(engine.settings.workspace_dir
        .join(writer_id.to_string()).join("main.rs"),"fn main() { println!(\"ok\"); }\\n")
        .await.unwrap();
    store.put(writer).await.unwrap();
    sdk.advance_task_team(root.id).await.unwrap();
    let view=sdk.task_team(root.id).await.unwrap();
    assert_eq!(view.tasks[0].status,TeamTaskState::ApprovalRequired);
    assert_eq!(view.tasks[1].status,TeamTaskState::Pending);
    assert!(store.get(reviewer_id).await.is_none());

    let preview=sdk.preview_task_team_patch(root.id,"writer").await.unwrap();
    assert!(preview.can_apply);
    assert_eq!(preview.changed_paths,vec!["main.rs"]);
    assert!(sdk.approve_task_team_patch(root.id,"writer","incorrect").await.is_err());
    assert!(store.get(reviewer_id).await.is_none(),
        "a wrong preview digest must never unlock Reviewer");
    assert_eq!(tokio::fs::read_to_string(source.join("main.rs"))
        .await.unwrap(),"fn main() {}\n");
    sdk.approve_task_team_patch(root.id,"writer",&preview.sha256).await.unwrap();
    assert_eq!(tokio::fs::read_to_string(engine.settings.workspace_dir
        .join(root.id.to_string()).join("main.rs")).await.unwrap(),
        "fn main() { println!(\"ok\"); }\\n");
    assert_eq!(tokio::fs::read_to_string(source.join("main.rs"))
        .await.unwrap(),"fn main() {}\n");

    let reviewer=store.get(reviewer_id).await.unwrap();
    assert_eq!(reviewer.parent_run_id,Some(root.id));
    assert!(reviewer.read_only);
    assert!(reviewer.repository.is_some());
    assert_eq!(tokio::fs::read_to_string(engine.settings.workspace_dir
        .join(reviewer_id.to_string()).join("main.rs")).await.unwrap(),
        "fn main() { println!(\"ok\"); }\\n");
    assert!(sdk.approve_task_team_patch(root.id,"writer",&preview.sha256).await.is_err(),
        "the same approved patch must never be applied twice");
    let saved=sdk.get(root.id).await.unwrap();
    assert!(saved.in_flight.is_none());
    assert_eq!(saved.events.iter().filter(|e|e.kind=="team_patch_integrated").count(),1);
    assert_eq!(saved.events.iter().filter(|e|e.kind=="team_patch_apply_intent").count(),1);
    drop(sdk);drop(engine);drop(store);
    let worktrees=[root.id,writer_id,reviewer_id];
    for id in worktrees {
        git(&source,&["worktree","remove","--force",
            &path.join("workspace").join(id.to_string()).to_string_lossy()]).await;
    }
    tokio::fs::remove_dir_all(path).await.unwrap();
}
