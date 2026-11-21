//! Real Engine::drive completion gate regression, with a fake model only.
//! Git and the ordinary edit_file executor operate on actual on-disk files.
use super::*;
use axum::{extract::State,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};

type ModelState=(Arc<AtomicUsize>,bool);
async fn false_then_repair_model(State((steps,repair)):State<ModelState>,
    Json(input):Json<Value>)->Json<Value>{
    let step=steps.fetch_add(1,Ordering::SeqCst);
    let correction_seen=input["messages"].as_array().is_some_and(|messages|
        messages.iter().any(|m|m["role"]=="assistant"&&m["content"].as_str()
            .is_some_and(|s|s.starts_with("Verifier requested corrections.")
                &&s.contains("No changed source or artifact files"))));
    if step==0{
        let names=input["tools"].as_array().unwrap().iter()
            .filter_map(|t|t.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"edit_file"));
        assert!(!names.contains(&"run_program"),
            "one-file benchmark should not burn context on 12-step DAG schemas");
    }
    let reply=if repair && step==1 && correction_seen{
        json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"real-edit-1","type":"function","function":{
                "name":"edit_file","arguments":json!({
                    "path":"sum.py","old_text":"range(1, 5)",
                    "new_text":"range(1, 6)"}).to_string()
            }}]})
    }else{
        json!({"role":"assistant","content":"The Python task is complete."})
    };
    let finish=if reply.get("tool_calls").is_some(){"tool_calls"}else{"stop"};
    Json(json!({"choices":[{"finish_reason":finish,"message":reply}],
        "usage":{"prompt_tokens":60,"completion_tokens":12}}))
}
async fn git(dir:&std::path::Path,args:&[&str]){
    let output=tokio::process::Command::new("git").args(args)
        .current_dir(dir).output().await.unwrap();
    assert!(output.status.success(),"git: {}",
        String::from_utf8_lossy(&output.stderr));
}
async fn completion_scenario(repair:bool,run_tests:bool)->(Run,String,usize){
    let port=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=port.local_addr().unwrap();
    let count=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let state=(count.clone(),repair);
        async move{axum::serve(port,Router::new()
            .route("/chat/completions",post(false_then_repair_model))
            .with_state(state)).await.unwrap();}
    });
    let root=std::env::temp_dir().join(format!("dominion-completion-gate-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.base_url=format!("http://{address}");
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(
        settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
    let mut run=Run::new(CreateRun{
        instruction:if run_tests{"Change sum.py to print 15 instead of 10 and run tests"}else{
            "Change sum.py to print 15 instead of 10"}.into(),
        budget_krw:Some(500.0),max_steps:Some(if repair&&run_tests{4}else if repair{5}else{2})
    },5).unwrap();
    run.plan=Some(String::new());
    run.repository=Some("fixture".into());
    run.required_changed_paths=vec!["sum.py".into()];
    run.messages=vec![
        json!({"role":"system","content":"Use tools for real changes"}),
        json!({"role":"user","content":run.instruction})
    ];
    let id=run.id;
    let dir=settings.workspace_dir.join(id.to_string());
    tokio::fs::create_dir_all(&dir).await.unwrap();
    git(&dir,&["init","-q"]).await;
    tokio::fs::write(dir.join("sum.py"),"print(sum(range(1, 5)))\n").await.unwrap();
    git(&dir,&["add","sum.py"]).await;
    git(&dir,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
        "commit","-qm","baseline"]).await;
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let final_run=tokio::time::timeout(std::time::Duration::from_secs(8),async{
        loop{
            let state=store.get(id).await.unwrap();
            if state.terminal(){break state;}
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.expect("native Rust Run did not become terminal");
    let text=tokio::fs::read_to_string(dir.join("sum.py")).await.unwrap();
    let requests=count.load(Ordering::SeqCst);
    for _ in 0..100 {
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    server.abort();
    drop(engine);drop(store);
    tokio::fs::remove_dir_all(root).await.unwrap();
    (final_run,text,requests)
}
#[tokio::test]
async fn unmodified_sum_py_cannot_complete_without_actual_artifact_receipt(){
    let (run,file,requests)=completion_scenario(false,false).await;
    assert_eq!(run.status,RunStatus::Failed,"events: {:?}",run.events);
    assert_eq!(file,"print(sum(range(1, 5)))\n");
    assert_eq!(requests,2);
    assert_eq!(run.events.iter()
        .filter(|e|e.kind=="artifact_evidence_missing").count(),2);
    assert!(run.events.iter().all(|e|e.kind!="completed"));
}
#[tokio::test]
async fn false_completion_is_repaired_by_edit_file_and_only_then_completed(){
    let (run,file,requests)=completion_scenario(true,false).await;
    assert_eq!(run.status,RunStatus::Completed,"events: {:?}",run.events);
    assert_eq!(file,"print(sum(range(1, 6)))\n");
    assert_eq!(requests,3);
    assert_eq!(run.events.iter()
        .filter(|e|e.kind=="artifact_evidence_missing").count(),1);
    assert_eq!(run.events.iter()
        .filter(|e|e.kind=="artifact_evidence_checked").count(),1);
    assert_eq!(run.events.iter().filter(|e|e.kind=="tool_result").count(),1);
    assert!(run.events.iter().any(|e|e.kind=="completed"));
    assert_eq!(run.input_tokens,180);
    assert_eq!(run.output_tokens,36);
}

#[tokio::test]
async fn completed_edit_cannot_skip_explicit_owner_test_requirement(){
    let (run,file,requests)=completion_scenario(true,true).await;
    assert_eq!(run.status,RunStatus::Failed,
        "a model assertion alone must not satisfy explicitly requested tests");
    assert_eq!(file,"print(sum(range(1, 6)))\n");
    assert_eq!(requests,4);
    assert!(run.events.iter().any(|e|e.kind=="artifact_evidence_checked"));
    assert!(run.events.iter().any(|e|e.kind=="test_evidence_missing"));
    assert!(run.events.iter().all(|e|e.kind!="completed"));
}
