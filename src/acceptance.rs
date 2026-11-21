//! Deterministic pre-completion evidence gate for repository work.
//!
//! A nonempty final model response is not proof of a file change.
//! Only the durable owner contract and a read-only Git/worktree inspection
//! affect this gate; neither model prose nor self-reported tool output may
//! declare artifacts present. Semantic correctness remains with explicit
//! acceptance checks and independent benchmark verifiers.
use crate::{config::Settings,domain::Run};
use serde_json::{json,Value};
use std::{collections::HashSet,path::{Component,Path}};
use tokio::{process::Command,time::{timeout,Duration}};

#[derive(Debug,Clone)]
pub struct Evidence {
    pub inspected:bool,
    pub satisfied:bool,
    pub changed:Vec<String>,
    pub required:Vec<String>,
    pub missing:Vec<String>,
    pub summary:String
}
impl Evidence{
    pub fn json(&self)->Value{
        json!({"inspected":self.inspected,"satisfied":self.satisfied,
            "changedPaths":self.changed,"requiredChangedPaths":self.required,
            "unmetPaths":self.missing,"semanticTestsVerified":false})
    }
}
pub fn validate_paths(paths:&[String])->Result<(),String>{
    if paths.len()>24{return Err("at most 24 required changed files".into());}
    let mut seen=HashSet::new();
    for path in paths{
        if path.is_empty()||path.len()>512||path.contains('\\')||
            !Path::new(path).components().all(|component|matches!(component,Component::Normal(_)))||
            !seen.insert(path){
            return Err(format!("invalid or duplicated required changed path: {path}"));
        }
    }
    Ok(())
}
fn mutation_intent(instruction:&str)->bool{
    let text=instruction.to_lowercase();
    let verbs=["write","create","edit","modify","update","fix ","fix:","repair","implement",
        "change","replace","generate","produce","correct","make ","build ","patch",
        "작성","수정","생성","구현","고쳐","고치","바꿔","만들","출력하게","저장해"];
    verbs.iter().any(|word|text.contains(word))
        && !["do not modify","without modifying","read-only","read only",
            "수정하지 마","변경하지 마"].iter().any(|word|text.contains(word))
}
/// An unambiguous single user-named source is a strong file target even
/// when the caller did not supply structured acceptance. Do not assume that
/// every mentioned document is a requested output (e.g. read A, write B).
fn one_named_target(instruction:&str)->Vec<String>{
    let mut paths=HashSet::new();
    for token in instruction.split_whitespace(){
        let part=token.trim_matches(|c:char|!c.is_alphanumeric()
            &&c!='.'&&c!='/'&&c!='_'&&c!='-');
        let suffix=part.rsplit('.').next().unwrap_or("");
        if !matches!(suffix,"py"|"rs"|"js"|"ts"|"tsx"|"jsx"|"md"|"txt"|"json"|
            "yaml"|"yml"|"toml"|"css"|"html"|"sh"|"go"|"java"|"c"|"cpp"){
            continue;
        }
        let candidate=part.to_string();
        if validate_paths(&[candidate.clone()]).is_ok() && !candidate.starts_with('.'){
            paths.insert(candidate);
        }
    }
    if paths.len()==1{paths.into_iter().collect()}else{vec![]}
}
fn meaningful(path:&str)->bool{
    !(path.starts_with(".dominion-benchmark-") || path.starts_with(".git/")
      ||path.starts_with("target/")||path.starts_with("node_modules/")
      ||path.starts_with("__pycache__/")||path.starts_with(".venv/")
      ||path.contains("/__pycache__/")||path.ends_with(".pyc"))
}
fn porcelain_changed(output:&[u8])->Result<Vec<String>,String>{
    let mut entries=output.split(|b|*b==0).filter(|x|!x.is_empty());
    let mut names=HashSet::new();
    while let Some(record)=entries.next(){
        // Git porcelain XY are status bytes, not arbitrary leading text.
        // Bad status data must never be treated as proof of a source edit.
        let valid=|code:u8|matches!(code,b' '|b'M'|b'A'|b'D'|b'R'|b'C'|b'U'|b'T'|b'?'|b'!');
        if record.len()<4||record[2]!=b' '||!valid(record[0])||!valid(record[1]){
            return Err("invalid git porcelain status record".into());
        }
        let name=String::from_utf8(record[3..].to_vec())
            .map_err(|_|"non-UTF8 changed repository path".to_string())?;
        if record[0]==b'R'||record[0]==b'C'||record[1]==b'R'||record[1]==b'C'{
            let _=entries.next().ok_or("git rename record missing old name")?;
        }
        if meaningful(&name){names.insert(name);}
    }
    let mut names=names.into_iter().collect::<Vec<_>>();
    names.sort();
    Ok(names)
}
pub async fn inspect(settings:&Settings,run:&Run)->Result<Evidence,String>{
    validate_paths(&run.required_changed_paths)?;
    if run.repository.is_none() || run.read_only ||
        (!mutation_intent(&run.instruction) && run.required_changed_paths.is_empty()){
        return Ok(Evidence{inspected:false,satisfied:true,changed:vec![],
            required:vec![],missing:vec![],summary:"non-mutating or non-repository run".into()});
    }
    let root=settings.workspace_dir.join(run.id.to_string());
    let child=timeout(Duration::from_secs(12),Command::new("git")
        .args(["status","--porcelain=v1","-z","--untracked-files=all"])
        .current_dir(&root).kill_on_drop(true).output()).await
        .map_err(|_|"repository completion inspection timed out".to_string())?
        .map_err(|e|format!("repository completion inspection failed: {e}"))?;
    if !child.status.success(){
        return Err(format!("repository completion inspection cannot read Git status: {}",
            String::from_utf8_lossy(&child.stderr).chars().take(500).collect::<String>()));
    }
    let changed=porcelain_changed(&child.stdout)?;
    let required=if run.required_changed_paths.is_empty(){
        one_named_target(&run.instruction)
    }else{run.required_changed_paths.clone()};
    let changed_set=changed.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut missing=Vec::new();
    for target in &required{
        let path=root.join(target);
        let regular=tokio::fs::symlink_metadata(&path).await
            .map(|m|m.is_file()&&!m.file_type().is_symlink()).unwrap_or(false);
        if !changed_set.contains(target.as_str())||!regular{
            missing.push(target.clone());
        }
    }
    let satisfied=!changed.is_empty()&&missing.is_empty();
    let summary=if changed.is_empty(){
        "No changed source or artifact files exist in this Run's Git worktree. Do not declare completion. Make the required edit using a real tool and inspect it.".to_string()
    }else if !missing.is_empty(){
        format!("The required output files were not actually updated in this Run's worktree: {}. Inspect exact paths and correct the artifact before finishing.",missing.join(", "))
    }else{"Observed local Git changes in the isolated Run worktree; semantic acceptance still requires tests.".into()};
    Ok(Evidence{inspected:true,satisfied,changed,required,missing,summary})
}


/// Deterministic evidence of actual test execution, not a model verdict.
#[derive(Debug,Clone)]
pub struct TestEvidence {
    pub satisfied:bool,
    pub last_test_seq:Option<u64>,
    pub last_mutation_seq:Option<u64>,
    pub summary:String,
}
impl TestEvidence {
    pub fn json(&self)->Value {
        json!({"satisfied":self.satisfied,"lastTestReceiptSeq":self.last_test_seq,
            "lastPotentialMutationSeq":self.last_mutation_seq,
            "testExecutionVerified":self.satisfied})
    }
}
fn tests_requested(instruction:&str)->bool {
    let text=instruction.to_lowercase();
    if ["do not run tests","don't run tests","without running tests",
        "테스트 실행하지","테스트 돌리지","테스트는 하지"]
        .iter().any(|negation|text.contains(negation)){return false;}
    ["run tests","run the tests","run unit tests","execute tests",
        "run pytest","run cargo test","run npm test","run the test suite",
        "테스트 실행","테스트를 실행","테스트 돌려","테스트를 돌려",
        "테스트까지 돌려","테스트까지 실행","테스트 통과시켜",
        "테스트를 통과","검증 테스트 실행"]
        .iter().any(|request|text.contains(request))
}
fn test_command(command:&str)->bool {
    let command=command.trim();
    // Compound commands can change sources after testing, and merely
    // echoing a test command is not evidence of executed tests.
    if command.is_empty()||command.chars().any(|c|
        matches!(c,';'|'&'|'|'|'\n'|'\r'|'>'|'<'|'\u{60}'))
        ||command.contains("$(") {return false;}
    let words=command.split_whitespace().collect::<Vec<_>>();
    if words.iter().any(|word|matches!(*word,
        "--help"|"-h"|"--version"|"-V"|"--list"|"-list"|"--no-run"|
        "--dry-run"|"--collect-only")){return false;}
    matches!(words.as_slice(),
        ["cargo","test",..] | ["pytest",..] | ["python","-m","pytest",..]
        | ["python3","-m","pytest",..] | ["python","-m","unittest",..]
        | ["python3","-m","unittest",..] | ["npm","test",..]
        | ["npm","run","test",..] | ["pnpm","test",..]
        | ["yarn","test",..] | ["bun","test",..]
        | ["go","test",..] | ["dotnet","test",..]
        | ["mvn","test",..] | ["gradle","test",..]
        | ["./gradlew","test",..] | ["node","--test",..]
        | ["npx","vitest","run",..] | ["npx","jest",..]
        | ["pnpm","exec","vitest","run",..])
}
fn potential_mutation(name:&str,arguments:&Value)->bool {
    match name {
        "list_files"|"read_file"|"search_files"|"git_status"|"git_diff"|
        "read_program"|"read_tool_receipt"|"search_tool_receipts"|
        "check_subagent"|"check_subagents" => false,
        "shell" => !arguments["command"].as_str().is_some_and(test_command),
        _ => true,
    }
}
/// A requested test with no fresh receipt rejects model-only completion and
/// asks the existing approval-gated tool path to run it.
pub fn requested_test_evidence(run:&Run)->Option<TestEvidence> {
    let explicitly_requested=tests_requested(&run.instruction)
        ||run.messages.iter().any(|message|message["role"]=="user"
            &&message["content"].as_str()
                .and_then(|text|text.strip_prefix("New user instruction:"))
                .is_some_and(tests_requested));
    if run.repository.is_none()||!explicitly_requested{return None;}
    let mut starts=std::collections::HashMap::<String,(u64,String,Value)>::new();
    let mut last_mutation=None::<u64>;
    let mut last_test=None::<u64>;
    for event in &run.events {
        if event.kind=="tool_started" {
            if let Some(detail)=event.detail.as_ref(){
                let name=detail["name"].as_str().unwrap_or("");
                let call_id=detail["id"].as_str().unwrap_or("");
                let arguments=detail.get("arguments").cloned().unwrap_or(Value::Null);
                if !call_id.is_empty(){
                    starts.insert(call_id.to_owned(),(event.seq,name.to_owned(),arguments.clone()));
                }
                if !name.is_empty()&&potential_mutation(name,&arguments){
                    last_mutation=Some(event.seq);
                }
            }
        } else if event.kind=="tool_result" {
            if let Some(detail)=event.detail.as_ref(){
                let call_id=detail["call_id"].as_str().unwrap_or("");
                if detail["name"]=="shell"&&detail["succeeded"]==true{
                    if let Some((start,name,arguments))=starts.get(call_id){
                        if name=="shell"&&test_command(arguments["command"].as_str().unwrap_or(""))
                            &&last_mutation.is_none_or(|mutation|mutation<*start){
                            last_test=Some(event.seq);
                        }
                    }
                }
            }
        }
    }
    let satisfied=last_test.is_some_and(|test|
        last_mutation.is_none_or(|mutation|mutation<test));
    let summary=if satisfied {
        "An approved test command succeeded after the last possible repository edit. This is executed-test evidence, not semantic proof.".to_string()
    }else{
        "Explicitly requested tests have no fresh successful execution receipt after the last possible repository change. Request approval for a real shell test command, inspect its exit status, fix failures, and rerun before claiming completion. Model prose is not a receipt.".to_string()
    };
    Some(TestEvidence{satisfied,last_test_seq:last_test,
        last_mutation_seq:last_mutation,summary})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn status_detection_requires_real_changes_not_model_prose(){
        assert_eq!(porcelain_changed(b" M sum.py\0?? hello.txt\0").unwrap(),
            vec!["hello.txt","sum.py"]);
        assert!(porcelain_changed(b"\0").unwrap().is_empty());
        assert!(porcelain_changed(b"?? .dominion-benchmark-usage.json\0").unwrap().is_empty());
        assert!(porcelain_changed(b"no separator\0").is_err());
    }
    #[test]
    fn one_explicit_task_target_is_detected_without_guessing_multiple_references(){
        assert_eq!(one_named_target("Change sum.py so the result is 15"),vec!["sum.py"]);
        assert_eq!(one_named_target("Create SUMMARY.md in repository root"),vec!["SUMMARY.md"]);
        assert!(one_named_target("Read README.md and edit parser.py").is_empty());
        assert!(mutation_intent("수정해서 15를 출력하게 하세요"));
        assert!(!mutation_intent("Inspect sum.py and explain its output"));
        assert!(validate_paths(&["../outside.py".into()]).is_err());
    }

    #[test]
    fn explicitly_requested_tests_need_fresh_successful_tool_receipt(){
        use serde_json::json;
        let mut run=Run::new(crate::domain::CreateRun{
            instruction:"Fix sum.py and run tests".into(),
            budget_krw:None,max_steps:None
        },5).unwrap();
        run.repository=Some("fixture".into());
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
        run.messages.push(json!({"role":"assistant","content":"All 49 tests passed."}));
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
        let started=|run:&mut Run,id:&str,name:&str,args:Value|{
            run.event("tool_started","intent",Some(json!({
                "id":id,"name":name,"arguments":args
            })));
        };
        let finished=|run:&mut Run,id:&str,success:bool|{
            run.event(if success{"tool_result"}else{"tool_error"},"done",
                Some(json!({"call_id":id,"name":"shell","succeeded":success})));
        };
        started(&mut run,"echo","shell",json!({"command":"echo cargo test"}));
        finished(&mut run,"echo",true);
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
        started(&mut run,"test1","shell",json!({"command":"cargo test --locked"}));
        finished(&mut run,"test1",true);
        assert!(requested_test_evidence(&run).unwrap().satisfied);
        started(&mut run,"edit","edit_file",json!({"path":"sum.py"}));
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
        started(&mut run,"test2","shell",json!({"command":"python -m pytest -q"}));
        finished(&mut run,"test2",false);
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
        started(&mut run,"test3","shell",json!({"command":"python -m pytest -q"}));
        finished(&mut run,"test3",true);
        assert!(requested_test_evidence(&run).unwrap().satisfied);
        started(&mut run,"compound","shell",json!({"command":"cargo test && echo changed"}));
        finished(&mut run,"compound",true);
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
    }
    #[test]
    fn test_execution_gate_only_applies_to_explicit_owner_requests(){
        use serde_json::json;
        let mut run=Run::new(crate::domain::CreateRun{
            instruction:"Explain the test approach for main.py".into(),
            budget_krw:None,max_steps:None
        },5).unwrap();
        run.repository=Some("fixture".into());
        assert!(requested_test_evidence(&run).is_none());
        run.messages.push(json!({"role":"user",
            "content":"New user instruction: 테스트를 실행하고 통과시켜"}));
        assert!(!requested_test_evidence(&run).unwrap().satisfied);
        run.messages.clear();
        run.instruction="Fix main.py and do not run tests".into();
        assert!(requested_test_evidence(&run).is_none());
    }

    #[tokio::test]
    async fn unchanged_or_wrong_path_cannot_satisfy_owner_acceptance_contract(){
        let root=std::env::temp_dir().join(format!("dominion-acceptance-{}",uuid::Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.workspace_dir=root.clone();
        let mut run=Run::new(crate::domain::CreateRun{
            instruction:"Fix sum.py so it produces 15".into(),budget_krw:Some(5.0),
            max_steps:Some(5)},5).unwrap();
        run.repository=Some("fixture".into());
        run.required_changed_paths=vec!["sum.py".into()];
        let worktree=root.join(run.id.to_string());
        tokio::fs::create_dir_all(&worktree).await.unwrap();
        let git=|args:&[&str]|{
            let dir=worktree.clone();
            let args=args.iter().map(|s|s.to_string()).collect::<Vec<_>>();
            async move{
                let output=Command::new("git").args(&args).current_dir(dir)
                    .output().await.unwrap();
                assert!(output.status.success(),"git failed: {}",
                    String::from_utf8_lossy(&output.stderr));
            }
        };
        git(&["init","-q"]).await;
        tokio::fs::write(worktree.join("sum.py"),"print(sum(range(1, 5)))\n").await.unwrap();
        git(&["add","."]).await;
        git(&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
            "commit","-qm","baseline"]).await;
        let before=inspect(&settings,&run).await.unwrap();
        assert!(!before.satisfied);
        assert!(before.changed.is_empty());
        tokio::fs::create_dir_all(worktree.join("docs")).await.unwrap();
        tokio::fs::write(worktree.join("docs/sum.py"),"print(15)").await.unwrap();
        let wrong=inspect(&settings,&run).await.unwrap();
        assert!(!wrong.satisfied);
        assert_eq!(wrong.missing,vec!["sum.py"]);
        tokio::fs::write(worktree.join("sum.py"),"print(sum(range(1, 6)))\n").await.unwrap();
        let correct=inspect(&settings,&run).await.unwrap();
        assert!(correct.satisfied);
        assert!(correct.changed.contains(&"sum.py".into()));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
