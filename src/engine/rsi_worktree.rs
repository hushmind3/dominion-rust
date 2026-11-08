//! Evidence-producing, isolated source worktree for model-driven DOMINION revisions.
//! A worktree protects the live checkout from ordinary workspace writes; it is
//! NOT a process/OS security sandbox. No candidate is automatically promoted.
use crate::engine::{jobs::JobContext, rsi, tools};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::{Path,PathBuf},time::Duration};
use tokio::process::Command;

const GIT_LIMIT: Duration = Duration::from_secs(30);
const CHECK_LIMIT: Duration = Duration::from_secs(360);
// tools::run_command retains at most 80_004 bytes of stdout. Reject anything
// near that boundary rather than hashing/approving a silently truncated diff.
const MAX_CAPTURE_BYTES: usize = 79_000;

pub struct Worktree {
    pub source: PathBuf,
    pub path: PathBuf,
    pub base: String,
}
fn digest(value:&str)->String {
    format!("{:x}",Sha256::digest(value.as_bytes()))
}
async fn run(mut command: Command, limit: Duration) -> Result<std::process::Output,String> {
    tools::run_command(&mut command, limit).await
}
async fn git(dir:&Path, args:&[&str])->Result<String,String>{
    let mut command=Command::new("git");
    command.current_dir(dir).args(args);
    let output=run(command,GIT_LIMIT).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).chars().take(900).collect());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
fn valid_sha(value:&str)->bool {
    value.len()==40 && value.bytes().all(|byte|byte.is_ascii_hexdigit())
}
pub async fn prepare(context:&JobContext,input:&Value)->Result<Worktree,String>{
    let base=input["rsiBaseCommit"].as_str().filter(|v|valid_sha(v))
        .ok_or("자기개선 시작 시 기준 Git 커밋이 고정되지 않았습니다.")?;
    let raw=input["rsiSourceDir"].as_str().ok_or("자기개선 원본 경로가 없습니다.")?;
    let source=std::fs::canonicalize(raw).map_err(|e|e.to_string())?;
    if !source.join("Cargo.toml").is_file() || !source.join("src/engine/rsi.rs").is_file() {
        return Err("DOMINION 소스가 아닙니다.".into());
    }
    if git(&source,&["rev-parse","--abbrev-ref","HEAD"]).await?.trim()!="dominion-sub" {
        return Err("원본 소스 브랜치가 dominion-sub가 아닙니다.".into());
    }
    if git(&source,&["rev-parse","HEAD"]).await?.trim()!=base {
        return Err("작업을 기다리는 동안 원본 커밋이 바뀌었습니다. 다시 시작하세요.".into());
    }
    if !git(&source,&["status","--porcelain","--untracked-files=normal"]).await?.is_empty() {
        return Err("원본에 다른 변경사항이 있어 자기개선 작업을 시작하지 않았습니다.".into());
    }
    let path=std::env::temp_dir().join(format!("dominion-rsi-{}",uuid::Uuid::new_v4()));
    let name=path.to_str().ok_or("분리 작업공간 경로를 읽을 수 없습니다.")?;
    // Detached Git worktree, not a new branch; the running server checkout stays pristine.
    git(&source,&["worktree","add","--detach",name,base]).await?;
    if let Err(error)=context.checkpoint(json!({
        "rsiWorkspace":name,"rsiBaseCommit":base,
        "rsiPhase":"model_editing","rsiIsolation":"detached_git_worktree_not_os_sandbox"
    })) {
        let _=git(&source,&["worktree","remove","--force",name]).await;
        return Err(error);
    }
    Ok(Worktree{source,path,base:base.to_string()})
}
fn classify(patch:&str)->&'static str {
    if patch.lines().any(|line|line.starts_with("diff --git ")
        && (line.contains("src/engine/rsi.rs ")
            || line.contains("src/engine/rsi_worktree.rs ")
            || line.contains("src/api/rsi.rs "))) {
        "improver"
    } else if patch.lines().filter(|line|line.starts_with("diff --git "))
        .all(|line|line.contains("src/engine/memory")
            || line.contains("src/engine/algorithms.rs")
            || line.contains("src/engine/room_state.rs")) {
        "algorithm"
    } else {"harness"}
}
fn check_result(label:&str, result:Result<std::process::Output,String>)->Value {
    match result {
        Ok(output)=>json!({
            "label":label,"ran":true,"passed":output.status.success(),
            "exitCode":output.status.code(),
            "stdout":String::from_utf8_lossy(&output.stdout).chars().rev().take(2600).collect::<String>().chars().rev().collect::<String>(),
            "stderr":String::from_utf8_lossy(&output.stderr).chars().rev().take(2600).collect::<String>().chars().rev().collect::<String>()
        }),
        Err(error)=>json!({"label":label,"ran":false,"passed":false,"error":error}),
    }
}
async fn validate(worktree:&Worktree,patch:&str)->Vec<Value> {
    let mut checks=Vec::new();
    let rust_change=patch.lines().any(|line|line.starts_with("diff --git a/src/")||line.starts_with("diff --git a/tests/"));
    let web_change=patch.lines().any(|line|line.starts_with("diff --git a/web/"));
    if rust_change {
        let mut command=Command::new("cargo");
        command.current_dir(&worktree.path).args(["check","--all-targets"]);
        checks.push(check_result("cargo check --all-targets",run(command,CHECK_LIMIT).await));
    }
    if web_change {
        let installed=worktree.source.join("web/node_modules");
        let deps=worktree.path.join("web/node_modules");
        #[cfg(unix)]
        if installed.is_dir() && !deps.exists() {
            let _=std::os::unix::fs::symlink(&installed,&deps);
        }
        if deps.exists() {
            let mut command=Command::new("npm");
            command.current_dir(worktree.path.join("web")).args(["run","typecheck"]);
            checks.push(check_result("npm run typecheck",run(command,Duration::from_secs(120)).await));
        } else {
            checks.push(json!({"label":"npm run typecheck","ran":false,"passed":false,
                "error":"원본 web/node_modules가 없어 독립 타입 검사를 실행하지 않았습니다."}));
        }
    }
    checks
}
pub async fn finish(
    context:&JobContext, job:&Value, worktree:&Worktree, model_result:Result<Option<Value>,String>,
    session_id:&str,
)->Result<Value,String>{
    let input=&job["input"];
    // Include newly created files without staging actual content; only the
    // disposable worktree index is touched, never the real branch/index.
    git(&worktree.path,&["add","-N","-A","."]).await?;
    // Preserve the final newline: unlike SHA/status output, Git patches
    // must not be trimmed or git apply may reject them as corrupt.
    let mut diff_command=Command::new("git");
    diff_command.current_dir(&worktree.path)
        .args(["diff","--no-ext-diff","--no-color","--binary","HEAD"]);
    let raw=run(diff_command,GIT_LIMIT).await?;
    if !raw.status.success(){
        return Err(String::from_utf8_lossy(&raw.stderr).to_string());
    }
    let patch=String::from_utf8_lossy(&raw.stdout).to_string();
    let patch_size=patch.len();
    let patch_sha=digest(&patch);
    let mut candidate_id=Value::Null;
    let mut capture_error=String::new();
    let mut checks=Vec::<Value>::new();
    if patch.trim().is_empty(){
        capture_error="모델이 실제 소스 파일을 변경하지 않았습니다.".into();
    } else if patch_size>=MAX_CAPTURE_BYTES {
        capture_error="코드 변경량이 79KB를 초과해 증거를 안전하게 수집하지 않았습니다.".into();
    } else {
        let objective=input["rsiObjective"].as_str().unwrap_or("DOMINION 자체 코드 개선");
        let hypothesis=if objective.chars().count()>=15 {objective.to_string()}
            else {format!("DOMINION 실행 엔진의 결함 개선: {objective}")};
        // Cases were fixed by the API before the model ran. Registration is
        // NOT a claim that baseline/candidate paired tests have been run.
        let proposal=json!({
            "target":classify(&patch),"baseCommit":worktree.base,
            "patch":patch,"hypothesis":hypothesis,
            "cases":input["rsiFrozenCases"],"oracleVersion":"rsi-worktree-v1",
            "generatorModel":input["model"],"generatorEffort":input["reasoningEffort"],
            "generatorSourceCommit":worktree.base
        });
        match rsi::propose(&context.state,&proposal) {
            Ok(mut candidate)=>{
                candidate["rsiJobId"]=json!(context.job_id);
                candidate["origin"]="model_driven_detached_worktree".into();
                candidate["pairedComparisonComplete"]=json!(false);
                candidate["reviewRequired"]=json!(true);
                candidate_id=candidate["id"].clone();
                context.state.store.put("rsi_candidate",&candidate);
                checks=validate(worktree,&patch).await;
            }
            Err(err)=>capture_error=format!("후보 등록 거부: {err}"),
        }
    }
    let source_clean=git(&worktree.source,&["status","--porcelain","--untracked-files=normal"])
        .await.map(|v|v.is_empty()).unwrap_or(false);
    let model_error=model_result.as_ref().err().cloned().unwrap_or_default();
    let all_checks_pass=!checks.is_empty()&&checks.iter().all(|item|item["passed"]==true);
    let result=json!({
        "id":format!("rsi-run-{}",context.job_id),
        "jobId":context.job_id,"sessionId":session_id,
        "baseCommit":worktree.base,"candidateId":candidate_id,
        "sourceClean":source_clean,"diffSha256":patch_sha,"diffBytes":patch_size,
        "checks":checks,"checksPassed":all_checks_pass,
        "pairedComparisonComplete":false,"autoMerged":false,"autoDeployed":false,
        "modelError":model_error,"captureError":capture_error,
        "modelMessageId":model_result.ok().flatten().and_then(|m|m["id"].as_str().map(str::to_string)),
        "createdAt":chrono::Utc::now().to_rfc3339(),
    });
    context.state.store.put("rsi_run",&result);
    context.state.store.audit("rsi.run_evidence",json!({
        "jobId":context.job_id,"candidateId":candidate_id,
        "patchSha256":patch_sha,"sourceClean":source_clean,"checksPassed":all_checks_pass,
        "modelError":model_error,"captureError":capture_error
    }));
    context.checkpoint(json!({"rsiPhase":"review","rsiEvidenceId":result["id"],
        "rsiCandidateId":candidate_id}))?;
    let (status,reason)=run_status(
        candidate_id.is_string(),all_checks_pass,source_clean,
        &model_error,&capture_error,
    );
    Ok(json!({
        "status":status,"reason":reason,
        "sessionId":session_id,
        "rsi":result,
    }))
}

/// Compilation and typecheck are only candidate-integrity evidence.
/// A completed run means the model produced a patch and verification ran
/// without error; it does NOT mean the patch improves runtime behavior.
fn run_status(
    has_candidate:bool,checks_passed:bool,source_clean:bool,
    model_error:&str,capture_error:&str,
)->(&'static str,&'static str){
    if !source_clean {return ("needs_review","original_source_changed");}
    if !model_error.is_empty(){return ("needs_review","model_error");}
    if !capture_error.is_empty()||!has_candidate {
        return ("needs_review","no_valid_candidate");
    }
    if !checks_passed{return ("needs_review","build_or_typecheck_failed");}
    ("completed","candidate_compiles_not_performance_verified")
}

/// Return a short, grounded repair instruction. Do not repeat blind model
/// calls for provider errors, missing diffs or checkout integrity failures.
pub fn repair_feedback(result:&Value)->Option<String>{
    if result["status"]!="needs_review"||result["reason"]!="build_or_typecheck_failed"
        ||result["rsi"]["sourceClean"]!=true
        ||!result["rsi"]["captureError"].as_str().unwrap_or("").is_empty()
        ||!result["rsi"]["modelError"].as_str().unwrap_or("").is_empty(){
        return None;
    }
    let checks=result["rsi"]["checks"].as_array()?;
    let failures=checks.iter().filter(|check|check["passed"]!=true)
        .map(|check|{
            let label=check["label"].as_str().unwrap_or("verification");
            let detail=check["stderr"].as_str().or_else(||check["error"].as_str())
                .unwrap_or("The verifier reported failure without further detail.");
            format!("{label}: {}",detail.chars().take(1100).collect::<String>())
        }).collect::<Vec<_>>();
    if failures.is_empty(){return None;}
    Some(format!(
        "이전 소스 변경 후 별도의 컴파일/타입 검사에서 오류가 발견됐습니다. 기존 변경을 읽고 실제 오류를 고쳐 다시 저장하세요. 검사 자체를 삭제하거나 우회하지 마세요. 수정 전과 무관한 파일은 건드리지 마세요. 테스트는 엔진이 다시 실행합니다.\n\n{}",
        failures.join("\n\n")
    ))
}
/// Apply a model-generated patch only after explicit owner review. A successful
/// compile is NOT proof of improved behavior. Never commit, push or deploy.
pub async fn apply_to_sub(state:&crate::state::AppState,id:&str,body:&Value)->Result<Value,String>{
    use std::sync::OnceLock;
    static APPLY_LOCK:OnceLock<tokio::sync::Mutex<()>>=OnceLock::new();
    let _lock=APPLY_LOCK.get_or_init(||tokio::sync::Mutex::new(())).lock().await;
    let mut candidate=state.store.get("rsi_candidate",id).ok_or("개선 후보를 찾을 수 없습니다.")?;
    if candidate["status"]!="proposed" || candidate["target"]=="improver" {
        return Err("독립 실험이 없는 개선기 자체 패치는 시험 적용할 수 없습니다.".into());
    }
    if body["confirmation"]!="APPLY_UNVERIFIED_TO_SUB"
        || body["baseCommit"]!=candidate["baseCommit"]
        || body["patchSha256"]!=candidate["patchSha256"] {
        return Err("기준 커밋·패치 해시·수동 적용 의사를 확인하세요.".into());
    }
    let job_id=candidate["rsiJobId"].as_str().ok_or("실행 증거가 없습니다.")?.to_string();
    let record_id=format!("rsi-run-{job_id}");
    let mut evidence=state.store.get("rsi_run",&record_id).ok_or("실제 실행 증거가 없습니다.")?;
    if evidence["candidateId"]!=candidate["id"] || evidence["diffSha256"]!=candidate["patchSha256"]
        || evidence["checksPassed"]!=true || evidence["sourceClean"]!=true
        || evidence["pairedComparisonComplete"]!=false {
        return Err("코드 출처 및 컴파일·타입 검사 통과 증거를 만족하지 못했습니다.".into());
    }
    let patch=candidate["patch"].as_str().ok_or("패치 본문이 없습니다.")?;
    if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {
        return Err("패치 해시가 저장된 값과 다릅니다.".into());
    }
    let status=crate::engine::dev::status(state).await;
    if status["git"]!=true || status["branch"]!="dominion-sub"
        || status["dirty"]!=0 || status["commit"]!=candidate["baseCommit"] {
        return Err("원본 작업 브랜치가 기준과 다르거나 수정 중이므로 적용하지 않았습니다.".into());
    }
    let remote=status["remote"].as_str().unwrap_or("");
    if !remote.is_empty() && !remote.contains("hushmind3/dominion-rust") {
        return Err("연결된 소스가 DOMINION Rust 저장소가 아닙니다.".into());
    }
    let source=status["config"]["sourceDir"].as_str().ok_or("소스 폴더가 연결되지 않았습니다.")?;
    let source=std::fs::canonicalize(source).map_err(|e|e.to_string())?;
    if !source.join("Cargo.toml").is_file() || !source.join("src/engine/rsi.rs").is_file(){
        return Err("DOMINION Rust 프로젝트 폴더가 아닙니다.".into());
    }
    let temporary=std::env::temp_dir().join(format!("dominion-rsi-patch-{}.diff",uuid::Uuid::new_v4()));
    std::fs::write(&temporary,patch).map_err(|e|e.to_string())?;
    let path=temporary.to_str().ok_or("임시 패치 경로 오류")?;
    let applied=async {
        git(&source,&["apply","--check",path]).await?;
        if !git(&source,&["status","--porcelain","--untracked-files=normal"]).await?.is_empty()
            || git(&source,&["rev-parse","HEAD"]).await?!=candidate["baseCommit"].as_str().unwrap_or("") {
            return Err("최종 적용 직전 기준 소스가 변경되어 중단했습니다.".into());
        }
        git(&source,&["apply",path]).await?;
        Ok::<(),String>(())
    }.await;
    let _=std::fs::remove_file(temporary);
    applied?;
    let now=chrono::Utc::now().to_rfc3339();
    candidate["status"]=json!("applied_to_sub");
    candidate["manualApplication"]=json!({
        "mode":"explicit_trial_without_paired_proof",
        "appliedAt":now,"sourceCommit":status["commit"],
        "requiresRebuild":true,"requiresCommit":true,"autoDeployed":false
    });
    evidence["manuallyApplied"]=json!(true);
    evidence["appliedAt"]=json!(now);
    state.store.put("rsi_run",&evidence);
    let saved=state.store.put("rsi_candidate",&candidate).ok_or("코드 적용 후 기록 저장에 실패했습니다.")?;
    state.store.audit("rsi.applied_trial_to_sub",
        json!({"candidateId":id,"jobId":job_id,"baseCommit":status["commit"]}));
    state.bus.record("rsi_candidate",&saved);
    Ok(saved)
}

pub async fn cleanup(worktree:&Worktree)->Result<(),String>{
    let name=worktree.path.to_str().ok_or("작업공간 경로 오류")?;
    git(&worktree.source,&["worktree","remove","--force",name]).await.map(|_|())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_covers_the_improver_harness_and_algorithm(){
        assert_eq!(classify("diff --git a/src/engine/rsi.rs b/src/engine/rsi.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/api/rsi.rs b/src/api/rsi.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/room_state.rs b/src/engine/room_state.rs\n"),"algorithm");
        assert_eq!(classify("diff --git a/web/src/pages/Home.tsx b/web/src/pages/Home.tsx\n"),"harness");
    }
    #[test]
    fn failing_verification_never_completes_and_only_valid_failures_retry(){
        assert_eq!(run_status(true,false,true,"",""),("needs_review","build_or_typecheck_failed"));
        assert_eq!(run_status(true,true,true,"",""),("completed","candidate_compiles_not_performance_verified"));
        assert_eq!(run_status(true,true,false,"","").0,"needs_review");
        assert_eq!(run_status(true,true,true,"provider timeout","").0,"needs_review");
        assert_eq!(run_status(false,true,true,"","invalid diff").0,"needs_review");
        let failing=json!({"status":"needs_review","reason":"build_or_typecheck_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"cargo check --all-targets","passed":false,"stderr":"error[E0308]: wrong type"}]}});
        assert!(repair_feedback(&failing).unwrap().contains("error[E0308]"));
        let mut no_retry=failing;
        no_retry["rsi"]["modelError"]=json!("provider unavailable");
        assert!(repair_feedback(&no_retry).is_none());
    }
    #[test]
    fn requires_complete_sha(){
        assert!(valid_sha("1234567890abcdef1234567890abcdef12345678"));
        assert!(!valid_sha("bad"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detached_worktree_collects_a_real_patch_without_touching_source(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        use std::process::Command as SyncCommand;
        fn git_local(path:&Path,args:&[&str])->String {
            let out=SyncCommand::new("git").current_dir(path).args(args).output().unwrap();
            assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        let root=std::env::temp_dir().join(format!("rsi-proof-{}",uuid::Uuid::new_v4()));
        let source=root.join("checkout");
        let data=root.join("data");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::create_dir_all(source.join("web/src/pages")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"sample\"\nversion=\"0.1.0\"\n").unwrap();
        std::fs::write(source.join("src/engine/rsi.rs"),"// source\n").unwrap();
        std::fs::write(source.join("web/src/pages/Demo.tsx"),"export const version = 1\n").unwrap();
        git_local(&source,&["init","-q"]);
        git_local(&source,&["checkout","-b","dominion-sub"]);
        git_local(&source,&["add","."]);
        git_local(&source,&["-c","user.email=ci@example.com","-c","user.name=CI","commit","-qm","base"]);
        let base=git_local(&source,&["rev-parse","HEAD"]);
        let cfg=Config{port:0,data_dir:data.clone(),web_dir:data.join("web"),workspace:data.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source.to_str().unwrap()}));
        let input=json!({
            "rsiBaseCommit":base,"rsiSourceDir":source.to_str().unwrap(),
            "rsiObjective":"DOMINION 화면 렌더링 검증 대상 소스를 수정한다",
            "model":"test-model",
            "rsiFrozenCases":[
                {"id":"memory","domain":"memory","holdout":true,"filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},
                {"id":"algorithm","domain":"algorithm","holdout":true,"filter":"engine::algorithms::tests::generation_uses_evidence_and_returns_executable_kernels"}
            ]
        });
        let job=state.store.put("job",&json!({"kind":"prompt","status":"running","input":input})).unwrap();
        let context=JobContext::new(state.clone(),job["id"].as_str().unwrap().to_string());
        let worktree=prepare(&context,&job["input"]).await.unwrap();
        assert_ne!(worktree.path,source);
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        std::fs::write(worktree.path.join("web/src/pages/Demo.tsx"),"export const version = 2\n").unwrap();
        let outcome=finish(&context,&job,&worktree,Ok(Some(json!({"id":"m1"}))),"session-rsi").await.unwrap();
        assert_eq!(outcome["status"],"needs_review");
        assert_eq!(outcome["reason"],"build_or_typecheck_failed");
        let candidate_id=outcome["rsi"]["candidateId"].as_str().unwrap();
        let candidate=state.store.get("rsi_candidate",candidate_id).unwrap();
        assert_eq!(candidate["status"],"proposed");
        assert_eq!(candidate["pairedComparisonComplete"],false);
        assert!(candidate["patch"].as_str().unwrap().contains("version = 2"));
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 1\n");
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        assert!(git_local(&source,&["status","--porcelain"]).is_empty());
        let request=json!({"confirmation":"APPLY_UNVERIFIED_TO_SUB",
            "baseCommit":base,"patchSha256":candidate["patchSha256"]});
        // Typecheck could not run without installed web dependencies. The
        // manual apply gate must refuse even an explicit user confirmation.
        assert!(apply_to_sub(&state,candidate_id,&request).await.is_err());
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 1\n");
        // Simulate a separately completed successful verifier receipt to
        // exercise the Git apply path. This is NOT a measured model benchmark.
        let record_id=format!("rsi-run-{}",context.job_id);
        let mut proof=state.store.get("rsi_run",&record_id).unwrap();
        proof["checksPassed"]=json!(true);
        state.store.put("rsi_run",&proof);
        let applied=apply_to_sub(&state,candidate_id,&request).await.unwrap();
        assert_eq!(applied["status"],"applied_to_sub");
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 2\n");
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        assert!(!git_local(&source,&["status","--porcelain"]).is_empty());
        cleanup(&worktree).await.unwrap();
        assert!(!worktree.path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn rsi_model_cannot_use_absolute_write_or_shell_tools(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!("rsi-permission-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"rsi-session","rsiRun":true}));
        let ctx=tools::ToolContext{
            workspace:&root,store:&state.store,state:&state,session_id:"rsi-session",
            agent_id:None,tools_enabled:true,tools_execution_enabled:true
        };
        let outside=root.join("outside.txt");
        assert!(tools::execute(&ctx,"file_write",&json!({"path":outside.to_str().unwrap(),"content":"forbidden"}),"run").await.is_err());
        assert!(tools::execute(&ctx,"shell_exec",&json!({"command":"touch outside.txt"}),"run").await.is_err());
        assert!(tools::execute(&ctx,"workspace_write",&json!({"path":".git","content":"tamper"}),"run").await.is_err());
        assert!(tools::execute(&ctx,"workspace_write",&json!({"path":"Cargo.toml","content":"tamper"}),"run").await.is_err());
        assert!(!outside.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
