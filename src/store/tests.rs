//! Extracted regression tests; production modules stay compact.
use super::*;
    use crate::domain::CreateRun;


    #[tokio::test]
    async fn durable_mutations_emit_exactly_one_event_without_polling_history(){
        use std::time::Duration;
        let root=std::env::temp_dir().join(format!("dominion-broadcast-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut changes=store.subscribe();
        let new=Run::new(CreateRun{instruction:"inspect source".into(),budget_krw:Some(50.0),max_steps:Some(2)},2).unwrap();
        let id=new.id;
        store.put(new).await.unwrap();
        let first=tokio::time::timeout(Duration::from_secs(1),changes.recv()).await.unwrap().unwrap();
        assert_eq!(first.id,id);
        assert_eq!(first.status,RunStatus::Queued);
        store.mutate(id,|run|{run.status=RunStatus::Paused;run.event("paused","operator pause",None);Ok(())}).await.unwrap();
        let second=tokio::time::timeout(Duration::from_secs(1),changes.recv()).await.unwrap().unwrap();
        assert_eq!(second.id,id);
        assert_eq!(second.status,RunStatus::Paused);
        assert!(second.updated_at_ms>=first.updated_at_ms);
        assert!(store.mutate(id,|_run|Err("reject".into())).await.is_err());
        assert!(tokio::time::timeout(Duration::from_millis(60),changes.recv()).await.is_err(),
            "failed mutation must not publish a phantom event");
        drop(changes);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn run_scoped_wait_ignores_unrelated_updates_and_wakes_on_fsync(){
        use tokio::time::{Duration,timeout};
        let root=std::env::temp_dir().join(format!("dominion-run-wait-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let watched=Run::new(CreateRun{instruction:"watch".into(),budget_krw:None,max_steps:None},2).unwrap();
        let other=Run::new(CreateRun{instruction:"other".into(),budget_krw:None,max_steps:None},2).unwrap();
        let id=watched.id;let other_id=other.id;
        store.put(watched).await.unwrap();store.put(other).await.unwrap();
        // Subscribe BEFORE reading the run; a transition on that boundary
        // must never disappear into an interval-poll race.
        let mut rx=store.subscribe();
        assert_eq!(store.get(id).await.unwrap().status,RunStatus::Queued);
        let follower=Arc::clone(&store);
        let mut pending=tokio::spawn(async move{follower.changed(id,&mut rx).await});
        store.mutate(other_id,|run|{run.event("other","irrelevant",None);Ok(())}).await.unwrap();
        assert!(timeout(Duration::from_millis(70),&mut pending).await.is_err(),
            "an unrelated Run must not wake the subscriber");
        store.mutate(id,|run|{run.status=RunStatus::Paused;
            run.event("paused","durably committed",None);Ok(())}).await.unwrap();
        assert!(timeout(Duration::from_secs(2),pending).await.unwrap().unwrap());
        assert_eq!(store.get(id).await.unwrap().status,RunStatus::Paused);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn unrelated_run_updates_proceed_while_another_run_is_locked(){
        use tokio::time::{timeout,Duration};
        let root=std::env::temp_dir().join(format!("dominion-per-run-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let a=Run::new(CreateRun{instruction:"a".into(),
            budget_krw:None,max_steps:None},2).unwrap();
        let b=Run::new(CreateRun{instruction:"b".into(),
            budget_krw:None,max_steps:None},2).unwrap();
        let a_id=a.id;
        let b_id=b.id;
        store.put(a).await.unwrap();
        store.put(b).await.unwrap();
        let locked=store.gate(a_id).await;
        let hold=locked.lock().await;
        let pending=Arc::clone(&store);
        let mut a_task=tokio::spawn(async move{
            pending.mutate(a_id,|run|{run.event("a","one",None);Ok(())}).await.unwrap()
        });
        tokio::task::yield_now().await;
        let b_committed=timeout(Duration::from_secs(3),store.mutate(b_id,|run|{
            run.event("b","independent progress",None);Ok(())
        })).await.expect("unrelated Run blocked by another Run's write gate").unwrap();
        assert_eq!(b_committed.events.last().unwrap().kind,"b");
        assert!(!a_task.is_finished(),"same-run gate did not order the blocked mutation");
        drop(hold);
        let a_committed=timeout(Duration::from_secs(3),&mut a_task).await.unwrap().unwrap();
        assert_eq!(a_committed.events.last().unwrap().kind,"a");
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn same_run_mutations_preserve_fsync_order_and_no_phantom_updates(){
        let root=std::env::temp_dir().join(format!("dominion-serial-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let run=Run::new(CreateRun{instruction:"serial".into(),
            budget_krw:None,max_steps:None},3).unwrap();
        let id=run.id;
        store.put(run).await.unwrap();
        let gate=store.gate(id).await;
        let held=gate.lock().await;
        let one=Arc::clone(&store);
        let first=tokio::spawn(async move{
            one.mutate(id,|r|{r.event("first","persist first",None);Ok(())}).await.unwrap()
        });
        tokio::task::yield_now().await;
        drop(held);
        first.await.unwrap();
        let two=Arc::clone(&store);
        two.mutate(id,|r|{r.event("second","persist second",None);Ok(())}).await.unwrap();
        assert_eq!(store.get(id).await.unwrap().events.iter().rev()
            .take(2).map(|v|v.kind.as_str()).collect::<Vec<_>>(),
            vec!["second","first"]);
        let reopened=Store::open(root.clone()).await.unwrap();
        // Recovery may append "recovered_ready" after the durable writes;
        // compare the persisted mutation receipts, not the last two events.
        assert_eq!(reopened.get(id).await.unwrap().events.iter().rev()
            .filter(|v|v.kind=="first"||v.kind=="second")
            .map(|v|v.kind.as_str()).collect::<Vec<_>>(),
            vec!["second","first"]);
        drop(reopened);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn overview_and_stats_do_not_include_model_transcript_payloads(){
        let root=std::env::temp_dir().join(format!("dominion-overview-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(crate::domain::CreateRun{
            instruction:"Inspect cheap overview".into(),budget_krw:None,max_steps:None
        },4).unwrap();
        run.messages.push(serde_json::json!({"role":"tool","content":"x".repeat(200_000)}));
        run.status=RunStatus::Running;
        let id=run.id;
        store.put(run).await.unwrap();
        let stats=store.stats().await;
        assert_eq!((stats.total,stats.active,stats.awaiting_approval),(1,1,0));
        let rows=store.headers().await;
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].id,id);
        assert_eq!(rows[0].instruction,"Inspect cheap overview");
        assert!(format!("{:?}",rows[0]).len()<1000,
            "overview unexpectedly copied historical tool transcripts");
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn startup_recovery_candidates_ignore_unrelated_full_transcripts(){
        let root=std::env::temp_dir().join(format!("dominion-recovery-ids-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut waiting=Run::new(CreateRun{
            instruction:"parent waiting for children".into(),
            budget_krw:None,max_steps:None
        },3).unwrap();
        waiting.status=RunStatus::Paused;
        waiting.waiting_for_children=true;
        let waiting_id=waiting.id;
        let mut recovered=Run::new(CreateRun{
            instruction:"safe interrupted read".into(),
            budget_krw:None,max_steps:None
        },3).unwrap();
        recovered.status=RunStatus::Paused;
        recovered.event("recovered_ready","Pure read may safely resume",None);
        let safe_id=recovered.id;
        let mut unresolved=Run::new(CreateRun{
            instruction:"unreconciled write".into(),
            budget_krw:None,max_steps:None
        },3).unwrap();
        unresolved.status=RunStatus::Paused;
        unresolved.event("tool_outcome_unknown","Do not replay external effect",None);
        store.put(waiting).await.unwrap();
        store.put(recovered).await.unwrap();
        store.put(unresolved).await.unwrap();
        let (suspended,safe)=store.recovery_candidates().await;
        assert_eq!(suspended,vec![waiting_id]);
        assert_eq!(safe,vec![safe_id]);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn child_lookup_does_not_include_unrelated_runs(){
        let root=std::env::temp_dir().join(format!("dominion-child-lookup-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let parent=Run::new(CreateRun{
            instruction:"parent".into(),budget_krw:Some(100.0),max_steps:Some(2)
        },2).unwrap();
        let id=parent.id;
        let mut child=Run::new(CreateRun{
            instruction:"child".into(),budget_krw:Some(10.0),max_steps:Some(2)
        },2).unwrap();
        child.parent_run_id=Some(id);
        let child_id=child.id;
        let unrelated=Run::new(CreateRun{
            instruction:"unrelated".into(),budget_krw:None,max_steps:None
        },2).unwrap();
        store.put(parent).await.unwrap();
        store.put(child).await.unwrap();
        store.put(unrelated.clone()).await.unwrap();
        assert!(store.has_children(id).await);
        assert!(!store.has_children(child_id).await);
        assert!(!store.has_children(unrelated.id).await);
        let children=store.children_of(id).await;
        assert_eq!(children.len(),1);
        assert_eq!(children[0].id,child_id);
        assert!(store.children_of(unrelated.id).await.is_empty());
        let mut grandchild=Run::new(CreateRun{
            instruction:"grandchild".into(),budget_krw:Some(5.0),max_steps:Some(2)
        },2).unwrap();
        grandchild.parent_run_id=Some(child_id);
        let grandchild_id=grandchild.id;
        store.put(grandchild).await.unwrap();
        let family=store.family_of(id).await;
        assert_eq!(family.len(),2);
        assert!(family.iter().any(|run|run.id==child_id));
        assert!(family.iter().any(|run|run.id==grandchild_id));
        assert!(store.family_of(unrelated.id).await.is_empty());
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn startup_does_not_rewrite_unchanged_historical_runs(){
        let root=std::env::temp_dir().join(format!("dominion-stable-history-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{
            instruction:"preserve completed history".into(),
            budget_krw:Some(20.0),max_steps:Some(2)
        },2).unwrap();
        run.status=RunStatus::Completed;
        let id=run.id;
        store.put(run).await.unwrap();
        drop(store);

        // A valid historical JSON snapshot may contain insignificant
        // whitespace. Reopening it must not replace the disk file.
        let path=root.join(format!("{id}.json"));
        let mut bytes=fs::read(&path).await.unwrap();
        bytes.extend_from_slice(b"\n  \n");
        fs::write(&path,&bytes).await.unwrap();
        let reopened=Store::open(root.clone()).await.unwrap();
        assert_eq!(reopened.get(id).await.unwrap().status,RunStatus::Completed);
        assert_eq!(fs::read(&path).await.unwrap(),bytes,
            "startup rewrote an unchanged historical Run");
        drop(reopened);
        fs::remove_dir_all(root).await.unwrap();
    }

    /// Long-running IO/recovery soak with a real fsync-backed Store and 16
    /// concurrent workers. It uses no model or remote API credits.
    /// cargo test --lib soak_parallel_durable_workloads -- --ignored --nocapture
    /// Continuous crash/approval/cancellation/replay stress, not an idle sleep.
    /// Repeatedly creates new persistent stores and replays actual journal
    /// transitions until a wall-clock deadline expires.
    #[tokio::test]
    #[ignore = "30s opt-in continuous recovery stress"]
    async fn continuous_recovery_and_cancellation_soak(){
        use crate::domain::ToolCall;
        use serde_json::json;
        use std::time::{Duration,Instant};
        let seconds=std::env::var("DOMINION_LONG_SOAK_SECONDS").ok()
            .and_then(|s|s.parse::<u64>().ok()).unwrap_or(30).clamp(5,90);
        let root=std::env::temp_dir().join(format!("dominion-continuous-{}",Uuid::new_v4()));
        let start=Instant::now();
        let mut cycles=0usize;
        let mut completed=0usize;
        let mut remote_uncertain=0usize;
        let mut safe_reads=0usize;
        let mut cancelled=0usize;
        while start.elapsed()<Duration::from_secs(seconds){
            let path=root.join(cycles.to_string());
            let store=Store::open(path.clone()).await.unwrap();
            let mut ids=Vec::new();
            for i in 0..12usize{
                let mut run=Run::new(CreateRun{
                    instruction:format!("durability cycle {cycles} run {i}"),
                    budget_krw:Some(100.0),max_steps:Some(8)
                },8).unwrap();
                let kind=i%3;
                let call=ToolCall{
                    id:format!("call-{cycles}-{i}"),
                    name:if kind==1{"read_file"}else{"write_file"}.into(),
                    arguments:json!({"path":"sample.txt","content":"payload"})
                };
                run.status=RunStatus::Running;
                run.queued_calls.push(call.clone());
                let id=run.id;
                store.put(run).await.unwrap();
                store.mutate(id,|run|journal::begin(run,&call)).await.unwrap();
                if kind==0 && i%2==0{
                    store.mutate(id,|run|{
                        run.cancel_after_tool=true;
                        run.status=RunStatus::Paused;
                        run.event("cancel_pending_reconciliation",
                            "Awaiting uncertain write outcome",None);
                        Ok(())
                    }).await.unwrap();
                }
                if kind==2{
                    store.mutate(id,|run|journal::finish(
                        run,&call,&Ok("confirmed written".into()),false
                    )).await.unwrap();
                }
                ids.push((id,kind,call));
            }
            drop(store);
            let restarted=Store::open(path.clone()).await.unwrap();
            for (id,kind,call) in ids{
                let run=restarted.get(id).await.unwrap();
                match kind{
                    0=>{
                        assert!(run.in_flight.is_some(),"remote write replayed");
                        assert_eq!(run.queued_calls.len(),1);
                        let result=restarted.mutate(id,|run|{
                            journal::finish(run,&call,&Ok("observed external write".into()),true)
                        }).await.unwrap();
                        assert!(result.in_flight.is_none());
                        remote_uncertain+=1;
                        if result.status==RunStatus::Cancelled{cancelled+=1;}
                    },
                    1=>{
                        assert!(run.in_flight.is_none(),"safe read did not recover");
                        assert_eq!(run.queued_calls.len(),1);
                        let receipt=restarted.mutate(id,|run|{
                            run.status=RunStatus::Running;
                            journal::begin(run,&call)?;
                            journal::finish(run,&call,&Ok("read repeated safely".into()),false)
                        }).await.unwrap();
                        assert!(receipt.queued_calls.is_empty());
                        safe_reads+=1;
                    },
                    2=>{
                        assert!(run.in_flight.is_none());
                        assert_eq!(run.messages.iter().filter(|m|m["role"]=="tool").count(),1);
                        completed+=1;
                    },
                    _=>unreachable!()
                }
            }
            drop(restarted);
            fs::remove_dir_all(path).await.unwrap();
            cycles+=1;
        }
        assert!(cycles>0);
        assert_eq!(remote_uncertain,cycles*4);
        assert_eq!(safe_reads,cycles*4);
        assert_eq!(completed,cycles*4);
        println!("{}",json!({
            "benchmark":"continuous_durable_recovery",
            "wall_seconds":start.elapsed().as_secs_f64(),
            "cycles":cycles,
            "runs":cycles*12,
            "unknown_mutations_reconciled":remote_uncertain,
            "safe_reads_retried":safe_reads,
            "cancelled_after_reconciliation":cancelled,
            "repeated_mutating_dispatches":0,
            "provider_cost_krw":0
        }));
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "disk-intensive reliability/performance test, run explicitly"]
    async fn soak_parallel_durable_workloads(){
        use crate::domain::ToolCall;
        use serde_json::json;
        use std::{sync::Arc,time::Instant};
        let start=Instant::now();
        let root=std::env::temp_dir().join(format!("dominion-soak-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let mut work=tokio::task::JoinSet::new();
        const TOTAL:usize=256;
        const WORKERS:usize=16;
        for worker in 0..WORKERS {
            let store=Arc::clone(&store);
            work.spawn(async move{
                let mut ids=Vec::new();
                let mut spans=Vec::new();
                for i in (worker..TOTAL).step_by(WORKERS){
                    let cycle=Instant::now();
                    let mut run=Run::new(CreateRun{
                        instruction:format!("soak {i}"),budget_krw:Some(20.0),max_steps:Some(4)
                    },4).unwrap();
                    let id=run.id;
                    let name=match i%4{
                        0|1=>"write_file",2=>"read_file",_=>"git_diff"
                    };
                    let call=ToolCall{id:format!("soak-tool-{i}"),name:name.into(),
                        arguments:json!({"path":"artifact.txt","content":"content"})};
                    run.status=RunStatus::Running;
                    run.queued_calls.push(call.clone());
                    store.put(run).await.unwrap();
                    store.mutate(id,|run|journal::begin(run,&call)).await.unwrap();
                    if i%4==0 {
                        store.mutate(id,|run|journal::finish(
                            run,&call,&Ok("simulated external receipt".into()),false)
                        ).await.unwrap();
                    }
                    spans.push(cycle.elapsed().as_millis() as u64);
                    ids.push((id,i));
                }
                (ids,spans)
            });
        }
        let mut ids=Vec::new();let mut latencies=Vec::new();
        while let Some(next)=work.join_next().await{
            let (mut chunk,mut samples)=next.unwrap();
            ids.append(&mut chunk);latencies.append(&mut samples);
        }
        assert_eq!(ids.len(),TOTAL);
        drop(store); // power-loss simulation: only snapshots survive
        let restored=Store::open(root.clone()).await.unwrap();
        let mut counts=[0usize;4];
        for (id,i) in &ids {
            let run=restored.get(*id).await.expect("persisted run disappeared");
            assert_eq!(run.status,RunStatus::Paused);
            match i%4 {
                0=>{
                    assert!(run.in_flight.is_none());
                    assert_eq!(run.messages.iter().filter(|m|m["role"]=="tool").count(),1);
                }
                1=>{
                    assert!(run.in_flight.is_some(),"uncertain mutating call replayed");
                    assert_eq!(run.events.last().unwrap().kind,"tool_outcome_unknown");
                }
                2|3=>{
                    assert!(run.in_flight.is_none(),"pure read not recovered");
                    assert_eq!(run.queued_calls.len(),1);
                    assert_eq!(run.events.last().unwrap().kind,"recovered_ready");
                }
                _=>unreachable!()
            }
            counts[i%4]+=1;
        }
        assert_eq!(counts,[64,64,64,64]);
        latencies.sort_unstable();
        let wall=start.elapsed().as_secs_f64().max(0.001);
        let p50=latencies[latencies.len()/2];
        let p95=latencies[latencies.len()*95/100];
        println!("{}",json!({
            "benchmark":"durable_tool_lifecycle_soak",
            "snapshot_count":TOTAL,
            "parallel_workers":WORKERS,
            "intent_and_receipt_snapshots":256*2+64,
            "recovered_safe_reads":128,
            "unconfirmed_writes_blocked":64,
            "observed_write_receipts":64,
            "wall_seconds":wall,
            "runs_per_second":TOTAL as f64/wall,
            "p50_run_persist_ms":p50,
            "p95_run_persist_ms":p95,
            "provider_api_cost_krw":0
        }));
        drop(restored);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn mutating_tool_crash_never_replays_or_drops_unknown_intent(){
        use crate::{domain::ToolCall,journal};
        use serde_json::json;
        let root=std::env::temp_dir().join(format!("dominion-crash-write-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{instruction:"edit".into(),budget_krw:None,max_steps:None},4).unwrap();
        run.status=crate::domain::RunStatus::Running;
        let call=ToolCall{id:"write-1".into(),name:"write_file".into(),
            arguments:json!({"path":"important.txt","content":"payload"})};
        run.queued_calls.push(call.clone());
        // Simulated crash after durable intent and before the tool receipt.
        journal::begin(&mut run,&call).unwrap();
        let id=run.id;store.put(run).await.unwrap();drop(store);
        let restarted=Store::open(root.clone()).await.unwrap();
        let snapshot=restarted.get(id).await.unwrap();
        assert_eq!(snapshot.status,crate::domain::RunStatus::Paused);
        assert_eq!(snapshot.in_flight.unwrap().id,call.id);
        assert_eq!(snapshot.events.last().unwrap().kind,"tool_outcome_unknown");
        // Explicit observation is the ONLY permissible completion.
        restarted.mutate(id,|run|{
            journal::finish(run,&call,&Ok("confirmed on disk".into()),true)
        }).await.unwrap();
        drop(restarted);
        let final_state=Store::open(root.clone()).await.unwrap().get(id).await.unwrap();
        assert!(final_state.in_flight.is_none());
        assert!(final_state.queued_calls.is_empty());
        assert_eq!(final_state.events.iter().filter(|e|e.kind=="tool_reconciled").count(),1);
        assert!(final_state.messages.last().unwrap()["content"].as_str().unwrap().contains("confirmed"));
        fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn read_only_tool_retries_queued_call_after_crash(){
        use crate::{domain::ToolCall,journal};
        use serde_json::json;
        let root=std::env::temp_dir().join(format!("dominion-crash-read-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{instruction:"inspect".into(),budget_krw:None,max_steps:None},4).unwrap();
        run.status=crate::domain::RunStatus::Running;
        let call=ToolCall{id:"read-1".into(),name:"read_file".into(),arguments:json!({"path":"hello.txt"})};
        run.queued_calls.push(call.clone());
        journal::begin(&mut run,&call).unwrap();
        let id=run.id;store.put(run).await.unwrap();drop(store);
        let restored=Store::open(root.clone()).await.unwrap().get(id).await.unwrap();
        assert_eq!(restored.status,crate::domain::RunStatus::Paused);
        assert!(restored.in_flight.is_none());
        assert_eq!(restored.queued_calls.len(),1);
        assert_eq!(restored.events.last().unwrap().kind,"recovered_ready");
        fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn failed_snapshot_write_does_not_publish_in_memory_state(){
        let root=std::env::temp_dir().join(format!("dominion-store-error-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let run=Run::new(CreateRun{instruction:"persist".into(),budget_krw:None,max_steps:None},4).unwrap();
        let id=run.id;store.put(run).await.unwrap();
        let target=root.join(format!("{id}.json"));
        fs::remove_file(&target).await.unwrap();
        fs::create_dir(&target).await.unwrap(); // Rename to directory must fail.
        assert!(store.mutate(id,|r|{r.status=crate::domain::RunStatus::Completed;Ok(())}).await.is_err());
        assert_eq!(store.get(id).await.unwrap().status,crate::domain::RunStatus::Queued);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn recovers_interrupted_execution() {
        let dir = std::env::temp_dir().join(format!("dominion-{}", Uuid::new_v4()));
        let store = Store::open(dir.clone()).await.unwrap();
        let mut run = Run::new(CreateRun { instruction: "test".into(), budget_krw: Some(0.0), max_steps: None }, 5).unwrap();
        run.status = RunStatus::Running;
        let id = run.id;
        store.put(run).await.unwrap();
        drop(store);
        let restored = Store::open(dir.clone()).await.unwrap().get(id).await.unwrap();
        assert_eq!(restored.status, RunStatus::Paused);
        assert!(restored.events.iter().any(|e| e.kind == "recovered_ready"));
        fs::remove_dir_all(dir).await.unwrap();
    }

    /// A completed transaction must not keep a strong lock for every Run ever
    /// touched. Meanwhile, a live transaction must retain its exact lock
    /// identity even when hundreds of unrelated entries are swept.
    #[tokio::test]
    async fn write_gate_cache_reaps_inactive_ids_without_splitting_a_live_lock(){
        use std::sync::Arc;
        use tokio::sync::oneshot;
        use tokio::time::{timeout,Duration};
        let root=std::env::temp_dir().join(format!("dominion-weak-gates-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let live_id=Uuid::new_v4();
        let live=store.gate(live_id).await;
        let live_weak=Arc::downgrade(&live);
        let held=live.lock().await;
        for _ in 0..384{
            let temporary=store.gate(Uuid::new_v4()).await;
            drop(temporary);
        }
        assert!(store.gates.lock().await.len()<=128,
            "inactive per-Run gates must not grow with completed historical Runs");
        let same=store.gate(live_id).await;
        assert!(Arc::ptr_eq(&live,&same),"sweep split an active Run transaction lock");
        let (tx,mut rx)=oneshot::channel();
        let waiter=tokio::spawn(async move{
            let _guard=same.lock().await;
            let _=tx.send(());
        });
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_err(),"same-Run waiter bypassed the held write gate");
        drop(held);
        timeout(Duration::from_secs(1),rx).await.unwrap().unwrap();
        waiter.await.unwrap();
        drop(live);
        assert!(live_weak.upgrade().is_none(),"completed write retained a strong gate");
        drop(store);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn startup_recovery_predicate_distinguishes_stable_and_interrupted_snapshots(){
        let mut run=Run::new(CreateRun{
            instruction:"startup gate".into(),budget_krw:None,max_steps:None
        },4).unwrap();
        assert!(journal::needs_recovery(&run));
        run.status=RunStatus::Completed;
        assert!(!journal::needs_recovery(&run));
        run.status=RunStatus::Paused;
        assert!(!journal::needs_recovery(&run));
        run.status=RunStatus::AwaitingApproval;
        assert!(!journal::needs_recovery(&run));
        run.approval_decision=Some(false);
        assert!(journal::needs_recovery(&run));
    }

    #[tokio::test]
    async fn strategy_lookup_projects_only_verified_same_repository_runs(){
        let root=std::env::temp_dir().join(format!("dominion-strategy-projection-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        for (repo,verified) in [("project/a",true),("project/b",true),("project/a",false)]{
            let mut run=Run::new(CreateRun{
                instruction:"Fix cargo unit test regression".into(),
                budget_krw:Some(50.0),max_steps:Some(4)
            },4).unwrap();
            run.repository=Some(repo.to_owned());
            run.strategy_learning=true;
            run.status=RunStatus::Completed;
            run.plan=Some("Read test errors, patch the correct source, verify cargo test".into());
            run.event("tool_result","write_file returned confirmation",None);
            run.event("verification",if verified{"PASS"}else{"REVISE"},None);
            store.put(run).await.unwrap();
        }
        let evidence=store.strategy_evidence("project/a").await;
        assert_eq!(evidence.len(),1);
        assert_eq!(evidence[0].repository,"project/a");
        assert!(store.strategy_evidence("global").await.is_empty());
        assert_eq!(store.list().await.len(),3,"history and receipts stay preserved");
        drop(store);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
