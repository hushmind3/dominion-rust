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
