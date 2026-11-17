//! Extracted regression tests; production modules stay compact.
use super::*;
    use crate::{domain::{CreateRun, ReconcileTool}, group::Groups};
    #[tokio::test]
    async fn child_budget_settlement_is_durable_and_idempotent(){
        let base=std::env::temp_dir().join(format!("dominion-settlement-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=base.join("runtime");
        settings.workspace_dir=base.join("workspace");
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
        let mut parent=Run::new(CreateRun{instruction:"parent".into(),budget_krw:Some(500.0),max_steps:Some(2)},2).unwrap();
        parent.reserved_budget_krw=200.0;
        let mut child=Run::new(CreateRun{instruction:"child".into(),budget_krw:Some(200.0),max_steps:Some(2)},2).unwrap();
        child.parent_run_id=Some(parent.id);
        child.status=RunStatus::Completed;
        child.cost_krw=70.0;
        store.put(parent.clone()).await.unwrap();
        store.put(child.clone()).await.unwrap();
        engine.settle_and_wake_ancestors(parent.id).await.unwrap();
        let first=store.get(parent.id).await.unwrap();
        assert_eq!(first.reserved_budget_krw,0.0);
        assert_eq!(first.delegated_spent_krw,70.0);
        assert_eq!(first.settled_child_ids,vec![child.id]);
        engine.settle_and_wake_ancestors(parent.id).await.unwrap();
        let second=store.get(parent.id).await.unwrap();
        assert_eq!(second.delegated_spent_krw,70.0);
        drop(engine);drop(store);
        let reopened=Store::open(settings.data_dir.join("runs")).await.unwrap();
        assert_eq!(reopened.get(parent.id).await.unwrap().delegated_spent_krw,70.0);
        tokio::fs::remove_dir_all(base).await.unwrap();
    }


    #[tokio::test]
    async fn cancellation_of_uncertain_tool_completes_after_reconciliation(){
        let base=std::env::temp_dir().join(format!("dominion-cancel-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=base.join("data");
        settings.workspace_dir=base.join("workspace");
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings,store.clone(),groups).unwrap();
        let call=ToolCall{id:"write-1".into(),name:"write_file".into(),arguments:json!({"path":"a.txt","content":"original"})};
        let mut run=Run::new(CreateRun{instruction:"write".into(),budget_krw:Some(10.0),max_steps:None},5).unwrap();
        run.status=RunStatus::Running;
        run.in_flight=Some(call.clone());
        run.queued_calls.push(call);
        store.put(run.clone()).await.unwrap();
        let cancelled=engine.cancel(run.id).await.unwrap();
        assert_eq!(cancelled.status,RunStatus::Paused);
        assert!(cancelled.cancel_after_tool);
        let finalized=engine.reconcile(run.id,ReconcileTool{succeeded:true,result:"observed complete".into()}).await.unwrap();
        assert_eq!(finalized.status,RunStatus::Cancelled);
        assert!(finalized.in_flight.is_none());
        assert!(!finalized.cancel_after_tool);
        assert_eq!(finalized.messages.last().unwrap()["tool_call_id"],"write-1");
        tokio::fs::remove_dir_all(base).await.unwrap();
    }



    #[tokio::test]
    async fn approval_wake_survives_more_than_old_two_second_deadline(){
        let root=std::env::temp_dir().join(format!("dominion-late-wake-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings,store.clone(),groups).unwrap();
        let mut run=Run::new(CreateRun{instruction:"deny after long previous worker".into(),
            budget_krw:Some(50.0),max_steps:Some(1)},1).unwrap();
        run.plan=Some("Approval must survive a slow detach".into());
        run.steps=1;
        run.status=RunStatus::AwaitingApproval;
        run.pending_approval=Some(ToolCall{id:"slow-denial".into(),name:"shell".into(),
            arguments:json!({"command":"false"})});
        let id=run.id;
        store.put(run).await.unwrap();
        // Simulates the prior worker still winding down for longer than the
        // former 200 x 10 ms scheduler polling limit.
        engine.active.lock().await.insert(id);
        engine.approve(id,false).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
        assert_eq!(store.get(id).await.unwrap().approval_decision,Some(false));
        engine.active.lock().await.remove(&id);
        engine.active_notify.notify_waiters();
        let mut done=None;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let run=store.get(id).await.unwrap();
            if run.terminal(){done=Some(run);break;}
        }
        let state=done.expect("queued approval wake was lost after worker detach");
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_denied").count(),1);
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert!(state.in_flight.is_none());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn denied_approval_has_receipt_but_no_execution_intent(){
        let dir=std::env::temp_dir().join(format!("dominion-denial-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=dir.join("data");
        settings.workspace_dir=dir.join("workspace");
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings,store.clone(),groups).unwrap();
        let mut run=Run::new(CreateRun{instruction:"test denied shell".into(),
            budget_krw:Some(50.0),max_steps:Some(1)},1).unwrap();
        run.plan=Some("never execute denied command".into());run.steps=1;
        run.status=RunStatus::AwaitingApproval;
        let call=ToolCall{id:"deny-1".into(),name:"shell".into(),
            arguments:json!({"command":"touch should-never-exist"})};
        run.pending_approval=Some(call.clone());
        let id=run.id;
        store.put(run).await.unwrap();
        engine.approve(id,false).await.unwrap();
        let mut last=None;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let snapshot=store.get(id).await.unwrap();
            if snapshot.terminal(){last=Some(snapshot);break;}
        }
        let snapshot=last.expect("denied approval was not resumed");
        assert_eq!(snapshot.events.iter().filter(|e|e.kind=="tool_denied").count(),1);
        assert_eq!(snapshot.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert_eq!(snapshot.messages.last().unwrap()["tool_call_id"],"deny-1");
        assert!(snapshot.in_flight.is_none());
        assert!(snapshot.pending_approval.is_none());
        assert!(tokio::fs::metadata(dir.join("workspace").join(id.to_string())
            .join("should-never-exist")).await.is_err());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn uncertain_tool_is_not_replayed_without_reconciliation() {
        let root=std::env::temp_dir().join(format!("dominion-reconcile-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        tokio::fs::create_dir_all(&settings.workspace_dir).await.unwrap();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings,Arc::clone(&store),groups).unwrap();
        let call=ToolCall{id:"call-1".into(),name:"write_file".into(),arguments:json!({"path":"a.txt","content":"done"})};
        let mut run=Run::new(CreateRun{instruction:"Write a file".into(),budget_krw:Some(10.0),max_steps:None},5).unwrap();
        let id=run.id;
        run.status=RunStatus::Paused;
        run.in_flight=Some(call.clone());
        run.queued_calls.push(call);
        store.put(run).await.unwrap();
        assert!(engine.launch(id).await.unwrap_err().contains("reconcile"));
        let reconciled=engine.reconcile(id,ReconcileTool{succeeded:true,result:"confirmed".into()}).await.unwrap();
        assert_eq!(reconciled.status,RunStatus::Paused);
        assert!(reconciled.in_flight.is_none());
        assert!(reconciled.queued_calls.is_empty());
        assert_eq!(reconciled.messages.last().unwrap()["tool_call_id"],"call-1");
        assert!(reconciled.events.iter().any(|event|event.kind=="tool_reconciled"));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
