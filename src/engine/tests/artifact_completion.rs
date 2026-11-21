//! Real Engine::drive completion gate regression, with a fake model only.
//! Git and the ordinary edit_file executor operate on actual on-disk files.
use super::*;
use axum::{extract::State,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};

type ModelState=(Arc<AtomicUsize>,bool,bool);
async fn false_then_repair_model(State((steps,repair,execute_tests)):State<ModelState>,
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
    }else if execute_tests&&step==3{
        json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"approved-test-1","type":"function","function":{
                "name":if execute_tests{"run_tests"}else{"shell"},"arguments":json!({"command":"python3 -m unittest discover"}).to_string()
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
async fn completion_scenario(repair:bool,run_tests:bool,execute_tests:bool)->(Run,String,usize){
    let port=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=port.local_addr().unwrap();
    let count=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let state=(count.clone(),repair,execute_tests);
        async move{axum::serve(port,Router::new()
            .route("/chat/completions",post(false_then_repair_model))
            .with_state(state)).await.unwrap();}
    });
    let root=std::env::temp_dir().join(format!("dominion-completion-gate-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    settings.shell_enabled=false;
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
        budget_krw:Some(500.0),max_steps:Some(if repair&&run_tests&&execute_tests{5}else if repair&&run_tests{4}else if repair{5}else{2})
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
    if run_tests {
        // The original file fails this checked-in test, the edited one passes.
        tokio::fs::write(dir.join("test_sum.py"),r#"import subprocess
import sys
import unittest
class SumTest(unittest.TestCase):
    def test_sum(self):
        actual = subprocess.check_output([sys.executable, "sum.py"], text=True).strip()
        self.assertEqual(actual, "15")
"#).await.unwrap();
    }
    git(&dir,&["add","."]).await;
    git(&dir,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
        "commit","-qm","baseline"]).await;
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let final_run=tokio::time::timeout(std::time::Duration::from_secs(8),async{
        loop{
            let state=store.get(id).await.unwrap();
            if state.terminal(){break state;}
            if execute_tests&&state.status==RunStatus::AwaitingApproval
                &&state.pending_approval.as_ref().is_some_and(|call|call.name=="run_tests")
                &&state.approval_decision.is_none(){
                engine.approve(id,true).await.unwrap();
            }
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
    let (run,file,requests)=completion_scenario(false,false,false).await;
    assert_eq!(run.status,RunStatus::Failed,"events: {:?}",run.events);
    assert_eq!(file,"print(sum(range(1, 5)))\n");
    assert_eq!(requests,2);
    assert_eq!(run.events.iter()
        .filter(|e|e.kind=="artifact_evidence_missing").count(),2);
    assert!(run.events.iter().all(|e|e.kind!="completed"));
}
#[tokio::test]
async fn false_completion_is_repaired_by_edit_file_and_only_then_completed(){
    let (run,file,requests)=completion_scenario(true,false,false).await;
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
    let (run,file,requests)=completion_scenario(true,true,false).await;
    assert_eq!(run.status,RunStatus::Failed,
        "a model assertion alone must not satisfy explicitly requested tests");
    assert_eq!(file,"print(sum(range(1, 6)))\n");
    assert_eq!(requests,4);
    assert!(run.events.iter().any(|e|e.kind=="artifact_evidence_checked"));
    assert!(run.events.iter().any(|e|e.kind=="test_evidence_missing"));
    assert!(run.events.iter().all(|e|e.kind!="completed"));
}


#[tokio::test]
async fn approved_scoped_test_runner_without_general_shell_allows_completion(){
    let (run,file,requests)=completion_scenario(true,true,true).await;
    assert_eq!(run.status,RunStatus::Completed,"events: {:?}",run.events);
    assert_eq!(file,"print(sum(range(1, 6)))\n");
    assert_eq!(requests,5);
    assert!(run.events.iter().any(|e|e.kind=="test_evidence_missing"));
    assert!(run.events.iter().any(|e|e.kind=="approval_required"
        &&e.message.contains("Test command")));
    assert!(run.events.iter().any(|e|e.kind=="decision"
        &&e.message=="User approved tool"));
    assert!(run.events.iter().any(|e|e.kind=="tool_result"
        &&e.detail.as_ref().is_some_and(|d|d["name"]=="run_tests"
            &&d["succeeded"]==true
            &&d["result"].as_str().is_some_and(|text|text.contains("exit status: 0")))));
    assert!(run.events.iter().any(|e|e.kind=="test_evidence_checked"));
    assert!(run.events.iter().any(|e|e.kind=="completed"));
}


/// The real Rust Engine runs a durable multi-step program. Only the fake
/// provider response is scripted; Git, file writes, sandbox, approval and
/// unit tests are all executed through production paths.
async fn program_edit_and_test_model(State(requests):State<Arc<AtomicUsize>>,
    Json(input):Json<Value>)->Json<Value>{
    let turn=requests.fetch_add(1,Ordering::SeqCst);
    let answer=if turn==0 {
        let names=input["tools"].as_array().unwrap().iter()
            .filter_map(|tool|tool.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"run_program"),
            "multi-file task must expose durable program DSL");
        assert!(names.contains(&"run_tests"),
            "test command must not depend on global shell setting");
        assert!(!names.contains(&"shell"));
        let program=json!({"steps":[
            {"id":"inspect","tool":"read_file",
                "arguments":{"path":"sum.py"}},
            {"id":"edit","tool":"edit_file",
                "arguments":{"path":"sum.py","old_text":"range(1, 5)",
                    "new_text":"range(1, 6)"}},
            {"id":"write","tool":"write_file",
                "arguments":{"path":"status.txt","content":"verified\n"}},
            {"id":"test","tool":"run_tests",
                "arguments":{"command":"python3 -m unittest discover"}}
        ]});
        json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"program-one-turn","type":"function","function":{
                "name":"run_program","arguments":program.to_string()
            }}]})
    } else {
        json!({"role":"assistant","content":"Both files updated and test execution confirmed."})
    };
    let finish=if answer.get("tool_calls").is_some(){"tool_calls"}else{"stop"};
    Json(json!({"choices":[{"finish_reason":finish,"message":answer}],
        "usage":{"prompt_tokens":60,"completion_tokens":16}}))
}

#[tokio::test]
async fn multi_file_program_runs_approved_tests_in_two_model_turns(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let count=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let counter=Arc::clone(&count);
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(program_edit_and_test_model))
                .with_state(counter)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-multi-program-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    settings.shell_enabled=false;
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
        instruction:"Edit sum.py and create status.txt; then run tests".into(),
        budget_krw:Some(500.0),max_steps:Some(6)
    },6).unwrap();
    run.plan=Some(String::new());
    run.repository=Some("fixture".into());
    run.required_changed_paths=vec!["sum.py".into(),"status.txt".into()];
    run.messages=vec![
        json!({"role":"system","content":"Use the available durable tools"}),
        json!({"role":"user","content":run.instruction})
    ];
    let id=run.id;
    let dir=settings.workspace_dir.join(id.to_string());
    tokio::fs::create_dir_all(&dir).await.unwrap();
    git(&dir,&["init","-q"]).await;
    tokio::fs::write(dir.join("sum.py"),"print(sum(range(1, 5)))\n").await.unwrap();
    tokio::fs::write(dir.join("test_sum.py"),r#"import subprocess
import sys
import unittest
class SumTest(unittest.TestCase):
    def test_sum(self):
        actual = subprocess.check_output([sys.executable, "sum.py"], text=True).strip()
        self.assertEqual(actual, "15")
        print("PROGRAM_OUTPUT_START")
        print("X" * 39000)
        print("PROGRAM_OUTPUT_END")
"#).await.unwrap();
    git(&dir,&["add","."]).await;
    git(&dir,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
        "commit","-qm","baseline"]).await;
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let finished=tokio::time::timeout(std::time::Duration::from_secs(25),async{
        loop {
            let state=store.get(id).await.unwrap();
            if state.terminal(){break state;}
            if state.status==RunStatus::AwaitingApproval
                &&state.pending_approval.as_ref().is_some_and(|tool|tool.name=="run_tests")
                &&state.approval_decision.is_none(){
                engine.approve(id,true).await.unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.expect("bounded multi-file program did not settle");
    assert_eq!(finished.status,RunStatus::Completed,"events: {:?}",finished.events);
    assert_eq!(count.load(Ordering::SeqCst),2,
        "inspection, edit, file output and tests must require only one planning model turn plus final response");
    assert_eq!(tokio::fs::read_to_string(dir.join("sum.py")).await.unwrap(),
        "print(sum(range(1, 6)))\n");
    assert_eq!(tokio::fs::read_to_string(dir.join("status.txt")).await.unwrap(),
        "verified\n");
    assert!(finished.program_session.is_none());
    assert_eq!(finished.events.iter().filter(|event|
        event.kind=="program_step_ready").count(),4);
    assert_eq!(finished.events.iter().filter(|event|
        event.kind=="approval_required").count(),1);
    assert!(finished.events.iter().any(|event|
        event.kind=="program_completed"
            &&event.detail.as_ref().is_some_and(|data|data["successful"]==4
                &&data["failed"]==0&&data["model_calls_inside"]==0)));
    let test_receipt=finished.messages.iter().find(|message|
        message["role"]=="tool" &&
        message["tool_call_id"]=="program:program-one-turn:test")
        .and_then(|message|message["content"].as_str())
        .expect("actual test receipt must remain in the durable Run");
    assert!(test_receipt.contains("output truncated:"),
        "noisy test output should be bounded with explicit omission");
    assert!(test_receipt.contains("PROGRAM_OUTPUT_END"),
        "the tail of the test output must survive");
    assert!(test_receipt.contains("exit status: 0"),
        "successful test execution must keep its real exit status");
    assert!(finished.events.iter().any(|event|event.kind=="test_evidence_checked"));
    assert!(finished.events.iter().any(|event|event.kind=="artifact_evidence_checked"));
    assert!(finished.events.iter().all(|event|
        !(event.kind=="tool_started"&&event.detail.as_ref()
            .is_some_and(|data|data["name"]=="shell"))));
    server.abort();
    drop(engine);drop(store);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
