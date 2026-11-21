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
async fn ordinary_agent_persists_matching_paid_model_intent_and_usage_before_completion(){
    let (engine,store,path)=mock_engine().await;
    let initial=Run::new(CreateRun{
        instruction:"Answer with one concise result".into(),
        budget_krw:Some(100.0),max_steps:Some(3)},3).unwrap();
    let id=initial.id;
    store.put(initial).await.unwrap();
    engine.launch(id).await.unwrap();
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let done=sdk.wait(id,Duration::from_secs(12)).await.unwrap();
    assert_eq!(done.status,RunStatus::Completed);
    let intents=done.events.iter().filter(|e|e.kind=="model_request_intent")
        .map(|e|e.seq).collect::<Vec<_>>();
    let settled=done.events.iter().filter(|e|e.kind=="model_request_settled")
        .map(|e|e.detail.as_ref().unwrap()["intent_seq"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert!(!intents.is_empty());
    assert_eq!(intents,settled,
        "each billable agent call must settle its exact intent with usage");
    assert!(done.input_tokens>0);
    assert!(crate::journal::unsettled_model_request(&done).is_none());
    let reloaded=Store::open(path.join("data/runs")).await.unwrap();
    assert!(crate::journal::unsettled_model_request(
        &reloaded.get(id).await.unwrap()).is_none());
    drop(reloaded);drop(sdk);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn crashed_agent_model_send_stays_uncertain_and_cannot_automatically_rebill(){
    let (engine,store,path)=mock_engine().await;
    let mut run=Run::new(CreateRun{
        instruction:"Do not repeat this paid provider call".into(),
        budget_krw:Some(100.0),max_steps:Some(3)},3).unwrap();
    run.status=RunStatus::Running;
    run.event("model_request_intent","Paid request was sent before the crash",
        Some(json!({"stage":"agent_turn","automatic_replay":false})));
    let id=run.id;
    let intent=crate::journal::unsettled_model_request(&run).unwrap();
    store.put(run).await.unwrap();
    let reloaded=Store::open(path.join("data/runs")).await.unwrap();
    let restored=reloaded.get(id).await.unwrap();
    assert_eq!(restored.status,RunStatus::Paused);
    assert_eq!(crate::journal::unsettled_model_request(&restored),Some(intent));
    assert!(restored.events.iter().any(|e|e.kind=="model_outcome_unknown"));
    assert!(engine.launch(id).await.is_err(),
        "neither a scheduler retry nor a user resume can reissue unknown billing");
    // A receipt for another request cannot clear the original fence.
    let mut forged=restored.clone();
    forged.event("model_request_settled","unrelated receipt",
        Some(json!({"intent_seq":intent+1})));
    assert_eq!(crate::journal::unsettled_model_request(&forged),Some(intent));
    drop(reloaded);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn worker_specific_provider_routing_is_owner_bound_and_survives_run_reload(){
    use crate::engine::WorkerModelRoute;
    use std::collections::HashMap;
    let (engine,store,path)=mock_engine().await;
    let model=engine.settings.main.name.clone();
    let mut routes=HashMap::new();
    routes.insert("writer".into(),WorkerModelRoute{
        provider_id:"dominion-native".into(),model:model.clone()
    });
    let unknown=HashMap::from([("not-in-graph".into(),WorkerModelRoute{
        provider_id:"dominion-native".into(),model:model.clone()
    })]);
    assert!(engine.create_task_team_with_routes("Invalid route".into(),
        vec![spec("writer",&[],TeamRole::Writer)],30.0,
        RunOptions::default(),None,unknown).await.is_err(),
        "a route for a non-existent Worker cannot become a real task");
    let invalid=HashMap::from([("writer".into(),WorkerModelRoute{
        provider_id:"dominion-native".into(),model:"unregistered-model".into()
    })]);
    assert!(engine.create_task_team_with_routes("Invalid model".into(),
        vec![spec("writer",&[],TeamRole::Writer)],30.0,
        RunOptions::default(),None,invalid).await.is_err(),
        "invalid provider/model bindings must fail before Team creation");
    let root=engine.create_task_team_with_routes("Owner-bound worker model".into(),
        vec![spec("writer",&[],TeamRole::Writer)],30.0,
        RunOptions::default(),None,routes).await.unwrap();
    assert!(root.events.iter().any(|e|e.kind=="team_worker_routes_bound"));
    let worker_id=root.team.as_ref().unwrap().tasks[0].run_id;
    engine.advance_task_team(root.id).await.unwrap();
    let child=store.get(worker_id).await.unwrap();
    assert_eq!(child.provider_id,"dominion-native");
    assert_eq!(child.model_name,model);
    assert_eq!(child.parent_run_id,Some(root.id));
    assert_eq!(child.events.iter().filter(|e|
        e.kind=="team_worker_route_bound").count(),1);
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let done=sdk.wait(worker_id,Duration::from_secs(12)).await.unwrap();
    assert_eq!(done.status,RunStatus::Completed);
    let reloaded=Store::open(path.join("data/runs")).await.unwrap();
    assert_eq!(reloaded.get(worker_id).await.unwrap().model_name,model);
    assert_eq!(reloaded.get(root.id).await.unwrap().events.iter()
        .filter(|e|e.kind=="team_worker_routes_bound").count(),1);
    drop(reloaded);drop(sdk);drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
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
    tokio::fs::write(engine.settings.workspace_dir
        .join(writer_id.to_string()).join("main.rs"),"fn main() { println!(\"ok\"); }\n")
        .await.unwrap();
    let evidence=crate::acceptance::inspect(&engine.settings,&writer).await.unwrap();
    assert!(evidence.inspected&&evidence.satisfied,"test must use an actual Git witness");
    writer.event("artifact_evidence_checked",evidence.summary.clone(),Some(evidence.json()));
    store.put(writer).await.unwrap();
    sdk.advance_task_team(root.id).await.unwrap();
    let view=sdk.task_team(root.id).await.unwrap();
    assert_eq!(view.tasks[0].status,TeamTaskState::ApprovalRequired);
    assert!(view.tasks[0].patch_review_ready,
        "only completed verified Writer patches may show the merge review action");
    assert!(!view.tasks[1].patch_review_ready);
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
        "fn main() { println!(\"ok\"); }\n");
    assert_eq!(tokio::fs::read_to_string(source.join("main.rs"))
        .await.unwrap(),"fn main() {}\n");

    let reviewer=store.get(reviewer_id).await.unwrap();
    assert_eq!(reviewer.parent_run_id,Some(root.id));
    assert!(reviewer.read_only);
    assert!(reviewer.repository.is_some());
    assert_eq!(tokio::fs::read_to_string(engine.settings.workspace_dir
        .join(reviewer_id.to_string()).join("main.rs")).await.unwrap(),
        "fn main() { println!(\"ok\"); }\n");
    assert!(sdk.approve_task_team_patch(root.id,"writer",&preview.sha256).await.is_err(),
        "the same approved patch must never be applied twice");
    let saved=sdk.get(root.id).await.unwrap();
    assert!(saved.in_flight.is_none());
    assert_eq!(saved.events.iter().filter(|e|e.kind=="team_patch_integrated").count(),1);
    assert_eq!(saved.events.iter().filter(|e|e.kind=="team_patch_apply_intent").count(),1);
    let reviewed=sdk.wait(reviewer_id,Duration::from_secs(12)).await.unwrap();
    assert_eq!(reviewed.status,RunStatus::Completed);
    // Run Completed precedes the worker teardown and the parent-budget
    // settlement. Do not remove worktrees while those tasks are active.
    let mut settled=false;
    for _ in 0..200{
        if !engine.active.lock().await.contains(&reviewer_id)
            && sdk.get(root.id).await.unwrap().settled_child_ids.contains(&reviewer_id){
            settled=true;break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(settled,"reviewer must settle before test teardown");
    drop(sdk);drop(engine);drop(store);
    let worktrees=[root.id,writer_id,reviewer_id];
    for id in worktrees {
        git(&source,&["worktree","remove","--force",
            &path.join("workspace").join(id.to_string()).to_string_lossy()]).await;
    }
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn uncertain_team_patch_requires_reverse_git_witness_and_is_not_auto_replayed(){
    use crate::domain::{CreateRun,Run,ToolCall};
    let (engine,store,path)=mock_engine().await;
    let sdk=Dominion::from_engine(Arc::clone(&engine));
    let source=engine.settings.workspace_dir.join("projects").join("recovery-demo");
    tokio::fs::create_dir_all(&source).await.unwrap();
    async fn git(dir:&std::path::Path,args:&[&str]){
        let output=tokio::process::Command::new("git").args(args)
            .current_dir(dir).output().await.unwrap();
        assert!(output.status.success(),"Git failed: {}",
            String::from_utf8_lossy(&output.stderr));
    }
    git(&source,&["init","-q"]).await;
    tokio::fs::write(source.join("verified.txt"),"baseline\n").await.unwrap();
    git(&source,&["add","verified.txt"]).await;
    git(&source,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
        "commit","-qm","baseline"]).await;
    let root=sdk.create_task_team_from_repository("Crash-safe patch review".into(),
        vec![spec("writer",&[],TeamRole::Writer)],30.0,
        RunOptions::default(),"projects/recovery-demo").await.unwrap();
    let writer_id=root.team.as_ref().unwrap().tasks[0].run_id;
    let sha=root.base_commit.clone().unwrap();
    crate::workspace::attach_at(&engine.settings,writer_id,
        "projects/recovery-demo",Some(&sha)).await.unwrap();
    let mut worker=Run::new(CreateRun{instruction:"Edit verified.txt".into(),
        budget_krw:Some(30.0),max_steps:Some(3)},3).unwrap();
    worker.id=writer_id;
    worker.parent_run_id=Some(root.id);
    worker.status=RunStatus::Completed;
    worker.repository=root.repository.clone();
    worker.base_commit=Some(sha.clone());
    tokio::fs::write(engine.settings.workspace_dir
        .join(writer_id.to_string()).join("verified.txt"),"approved change\n").await.unwrap();
    let proof=crate::acceptance::inspect(&engine.settings,&worker).await.unwrap();
    assert!(proof.satisfied&&proof.inspected);
    worker.event("artifact_evidence_checked",proof.summary.clone(),Some(proof.json()));
    store.put(worker).await.unwrap();
    sdk.advance_task_team(root.id).await.unwrap();
    let review=sdk.preview_task_team_patch(root.id,"writer").await.unwrap();
    assert!(review.can_apply);

    // Simulate a crash after the original approval was durably written but
    // before the tool receipt. Its effect may or may not have happened.
    let call=ToolCall{
        id:format!("team-patch-{writer_id}"),
        name:"team_patch_integrate".into(),
        arguments:json!({"task_id":"writer","child_run_id":writer_id,
            "patch_sha256":review.sha256})
    };
    store.mutate(root.id,|r|{
        r.in_flight=Some(call.clone());
        r.event("team_patch_apply_intent","Owner approved exact patch",Some(call.arguments.clone()));
        Ok(())
    }).await.unwrap();
    let reopened=Store::open(path.join("data/runs")).await.unwrap();
    let durable=reopened.get(root.id).await.unwrap();
    assert_eq!(durable.in_flight.as_ref().unwrap().name,"team_patch_integrate");
    drop(reopened);
    assert!(sdk.reconcile_task_team_patch(root.id,"writer",&review.sha256).await.is_err(),
        "missing actual changes cannot be mistaken for a completed patch");
    sdk.advance_task_team(root.id).await.unwrap();
    assert_eq!(sdk.task_team(root.id).await.unwrap().tasks[0].status,
        TeamTaskState::ApprovalRequired);
    assert_eq!(tokio::fs::read_to_string(engine.settings.workspace_dir
        .join(root.id.to_string()).join("verified.txt")).await.unwrap(),"baseline\n");

    // Git effect eventually observed: no repeat apply, only reverse-check.
    crate::workspace::merge_team_patch_verified(&engine.settings,root.id,writer_id,
        &sha,&sha,&review.sha256).await.unwrap();
    sdk.reconcile_task_team_patch(root.id,"writer",&review.sha256).await.unwrap();
    let outcome=sdk.get(root.id).await.unwrap();
    assert!(outcome.in_flight.is_none());
    assert_eq!(outcome.events.iter().filter(|e|e.kind=="team_patch_integrated").count(),1);
    assert!(outcome.events.iter().any(|e|e.kind=="team_patch_integrated"
        &&e.detail.as_ref().is_some_and(|v|v["reconciled"]==true)));
    assert!(sdk.reconcile_task_team_patch(root.id,"writer",&review.sha256).await.is_err(),
        "a reconciled intent must not become replayable");
    assert_eq!(tokio::fs::read_to_string(source.join("verified.txt")).await.unwrap(),
        "baseline\n");
    drop(sdk);drop(engine);drop(store);
    for id in [root.id,writer_id] {
        git(&source,&["worktree","remove","--force",
            &path.join("workspace").join(id.to_string()).to_string_lossy()]).await;
    }
    tokio::fs::remove_dir_all(path).await.unwrap();
}

#[tokio::test]
async fn uncertain_team_patch_cannot_be_marked_success_by_generic_reconcile(){
    let (engine,store,path)=mock_engine().await;
    let coordinator=engine.create_task_team("Do not forge Git integration".into(),
        vec![spec("writer",&[],TeamRole::Writer)],30.0,
        RunOptions::default()).await.unwrap();
    let id=coordinator.id;
    engine.store.mutate(id,|run|{
        run.in_flight=Some(ToolCall{
            id:"team-patch-test".into(),
            name:"team_patch_integrate".into(),
            arguments:json!({"task_id":"writer","patch_sha256":"a".repeat(64)})
        });
        run.event("team_patch_apply_intent","Uncertain Git integration",None);
        Ok(())
    }).await.unwrap();
    let result=engine.reconcile(id,ReconcileTool{
        succeeded:true,result:"Pretend it applied".into()
    }).await;
    assert!(result.unwrap_err().contains("Team patch reconciliation"),
        "ordinary reconciliation must not bypass SHA-256 and Git proof");
    let current=engine.store.get(id).await.unwrap();
    assert!(current.in_flight.is_some());
    assert!(!current.events.iter().any(|e|e.kind=="team_patch_integrated"));
    drop(engine);drop(store);
    tokio::fs::remove_dir_all(path).await.unwrap();
}
