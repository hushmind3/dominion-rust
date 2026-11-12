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

/// launchd starts the engine with a minimal PATH that often omits
/// `~/.cargo/bin` and Homebrew. Resolve the tool explicitly so RSI can build
/// even when the service environment is narrower than the user's shell.
fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file())
}

pub(super) fn cargo_executable() -> PathBuf {
    if let Some(explicit) = std::env::var_os("DOMINION_CARGO") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join(".cargo/bin/cargo"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/cargo"),
        PathBuf::from("/usr/local/bin/cargo"),
        PathBuf::from("/usr/bin/cargo"),
    ]);
    first_existing(candidates).unwrap_or_else(|| PathBuf::from("cargo"))
}

pub(super) fn node_executable() -> PathBuf {
    if let Some(explicit) = std::env::var_os("DOMINION_NODE") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join(".volta/bin/node"));
        let nvm = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm) {
            for entry in entries.flatten() {
                candidates.push(entry.path().join("bin/node"));
            }
        }
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/usr/bin/node"),
    ]);
    first_existing(candidates).unwrap_or_else(|| PathBuf::from("node"))
}

pub(super) fn npm_executable() -> PathBuf {
    if let Some(explicit) = std::env::var_os("DOMINION_NPM") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join(".volta/bin/npm"));
        candidates.push(home.join(".local/bin/npm"));
        let nvm = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm) {
            for entry in entries.flatten() {
                candidates.push(entry.path().join("bin/npm"));
            }
        }
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/npm"),
        PathBuf::from("/usr/local/bin/npm"),
        PathBuf::from("/usr/bin/npm"),
    ]);
    first_existing(candidates).unwrap_or_else(|| PathBuf::from("npm"))
}
// tools::run_command retains at most 80_004 bytes of stdout. Reject anything
// near that boundary rather than hashing/approving a silently truncated diff.
const MAX_CAPTURE_BYTES: usize = 79_000;

pub fn metric_cases()->Vec<Value>{
    [
        ("memory-quality","memory","memory_ranking",false),
        ("long-recall-quality","memory","long_owner_recall",true),
        ("planning-quality","planning","planning_quality",true),
        ("search-quality","improver","improvement_search",true),
        ("group-chat-input-cost","context","group_chat_context_efficiency",true),
    ].into_iter().map(|(id,domain,name,holdout)|json!({
        "id":id,"domain":domain,"holdout":holdout,"filter":format!("rsi_frozen_oracle::{name}"),
        "metrics":[
            {"key":"quality","direction":"higher","minRelativeGain":0.02,"minAbsoluteGain":0.025},
            {"key":"elapsedMs","direction":"lower","minRelativeGain":0.2,"minAbsoluteGain":1.0,"maxRelativeRegression":0.25}
        ]
    })).map(|mut case|{
        if case["id"]=="planning-quality" {
            // Independent planning evaluation contains 13 varied cases.
            // Require at least two newly correct examples (~2/13 gain),
            // not one overfitted benchmark phrase.
            case["metrics"][0]["minRelativeGain"]=json!(0.2);
            case["metrics"][0]["minAbsoluteGain"]=json!(0.14);
        }
        if case["id"]=="long-recall-quality"{
            case["metrics"].as_array_mut().unwrap().push(json!({"key":"contextChars","direction":"lower",
                "minRelativeGain":0.1,"minAbsoluteGain":20.0,"maxRelativeRegression":0.1}));
        }
        if case["id"]=="group-chat-input-cost"{
            case["metrics"]=json!([
                {"key":"quality","direction":"higher","minRelativeGain":0.02},
                {"key":"contextChars","direction":"lower","minRelativeGain":0.05,"minAbsoluteGain":50.0,"maxRelativeRegression":0.05}
            ]);
        }
        case
    }).collect()
}

/// Evidence from earlier autonomous source runs, reintroduced to the next
/// generation. A successful build is explicitly NOT behavioral improvement.
pub fn recent_feedback(state:&crate::state::AppState)->String{
    let mut runs=state.store.all("rsi_run");
    runs.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    let mut lines=Vec::<String>::new();
    for run in runs.into_iter().take(5){
        let job_id=run["jobId"].as_str().unwrap_or("");
        let objective=state.store.get("job",job_id)
            .and_then(|job|job["input"]["rsiObjective"].as_str().map(str::to_string))
            .unwrap_or_else(||"목표 기록 없음".into());
        let checks_ok=run["checksPassed"]==true;
        let invalid=run["captureError"].as_str().unwrap_or("");
        let model_err=run["modelError"].as_str().unwrap_or("");
        let broken=run["checks"].as_array().map(|checks|checks.iter()
            .filter(|check|check["passed"]!=true)
            .filter_map(|check|check["label"].as_str())
            .take(3).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let result=if checks_ok{"정적 검사 통과(실제 성능 향상 미검증)"}
            else {"정적 검사 실패 또는 실행되지 않음"};
        let paired_verdict=run["paired"]["verdict"].as_str().unwrap_or("비교 안 함");
        let error=if !invalid.is_empty(){invalid}
            else if !model_err.is_empty(){model_err}
            else if !broken.is_empty(){broken.as_str()}else{"증거 없음"};
        lines.push(format!(
            "- 과거 목표: {}; 결과: {}; A/B 검증: {}; 오류: {}; 후보: {}",
            objective.chars().take(220).collect::<String>(),
            result,paired_verdict,error.chars().take(220).collect::<String>(),
            run["candidateId"].as_str().unwrap_or("없음"),
        ));
    }
    if lines.is_empty(){"과거 자기개선 실행 증거 없음".into()}
    else {lines.join("\n")}
}

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
    let campaign=crate::engine::rsi_evolve::campaign_source(&context.state,input,&source,base);
    if !campaign && !is_work_branch(git(&source,&["rev-parse","--abbrev-ref","HEAD"]).await?.trim()) {
        return Err("원본 소스 브랜치가 dominion-sub 작업 브랜치가 아닙니다.".into());
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
/// Resume a real compiler-rejected source patch in the *next detached
/// worktree*. The worker sees the exact failed bytes, edits them in place,
/// and the normal finish()/paired evaluator measures the corrected diff
/// against the unchanged original base. No source branch is ever modified.
pub async fn restore_failed_candidate(
    context:&JobContext,input:&Value,worktree:&Worktree,
)->Result<Option<Value>,String>{
    let Some(id)=input["rsiRepairCandidateId"].as_str()
        .filter(|id|!id.is_empty()) else {return Ok(None);};
    let candidate=context.state.store.get("rsi_candidate",id)
        .ok_or("후손의 실패 패치 기록이 사라졌습니다.")?;
    let parent=input["rsiRepairParentJobId"].as_str()
        .ok_or("실패 패치의 원래 실행 작업이 없습니다.")?;
    if candidate["id"]!=id || candidate["rsiJobId"]!=parent
        || candidate["baseCommit"].as_str()!=Some(worktree.base.as_str())
        || !matches!(candidate["status"].as_str(),Some("proposed"|"inconclusive")) {
        return Err("후손 복구 대상의 작업·기준 커밋·후보 상태가 다릅니다.".into());
    }
    let run=context.state.store.get("rsi_run",&format!("rsi-run-{parent}"))
        .ok_or("컴파일 실패를 관찰한 원본 실행 기록이 없습니다.")?;
    if run["candidateId"]!=id
        || run["baseCommit"].as_str()!=Some(worktree.base.as_str()) {
        return Err("복구 후보의 원본 소스·실행 영수증이 일치하지 않습니다.".into());
    }
    let compiler_failure=candidate["status"]=="proposed"
        && run["checksPassed"]==false
        && run["checks"].as_array().is_some_and(|checks|checks.iter().any(|check|
            check["passed"]==false && check["ran"]==true));
    let quality_plateau=candidate["status"]=="inconclusive"
        && run["checksPassed"]==true
        && crate::engine::rsi::verify_revisable_quality_receipt(
            &context.state,&candidate).is_ok();
    if !compiler_failure && !quality_plateau {
        return Err("검증된 컴파일 실패 또는 독립 측정된 무향상 후보만 후손이 복구할 수 있습니다.".into());
    }
    let repair_kind=if compiler_failure {"compiler_failure"}else{"quality_plateau"};
    let patch=candidate["patch"].as_str().ok_or("실패 후보의 패치가 없습니다.")?;
    if patch.is_empty() || patch.len()>=MAX_CAPTURE_BYTES
        || candidate["patchSha256"].as_str()!=Some(digest(patch).as_str()) {
        return Err("계승할 패치의 내용·해시가 원본과 다릅니다.".into());
    }
    if !candidate["paths"].as_array().is_some_and(|paths|
        !paths.is_empty() && paths.iter().all(|path|path.as_str()
            .is_some_and(crate::engine::rsi::safe_path))) {
        return Err("계승할 변경 파일의 경로가 유효하지 않습니다.".into());
    }
    // Use an external temporary patch: placing it inside the worktree would
    // make the patch file itself appear as a forged source change.
    let path=std::env::temp_dir()
        .join(format!("dominion-rsi-restore-{}.patch",uuid::Uuid::new_v4()));
    std::fs::write(&path,patch).map_err(|e|e.to_string())?;
    let patch_path=path.to_str().ok_or("임시 패치 파일 경로 오류")?;
    let result=async {
        git(&worktree.path,&["apply","--check",patch_path]).await?;
        git(&worktree.path,&["apply",patch_path]).await?;
        Ok::<_,String>(())
    }.await;
    let _=std::fs::remove_file(&path);
    result?;
    context.checkpoint(json!({"rsiPhase":"restored_revisable_source",
        "rsiRepairKind":repair_kind,
        "rsiRepairCandidateId":id,"rsiRepairBaseCommit":worktree.base}))?;
    let failures=run["checks"].as_array().into_iter().flatten()
        .filter(|check|check["passed"]==false).take(2)
        .map(|check|format!("{}: {}",check["label"].as_str().unwrap_or("cargo check"),
            check["stderr"].as_str().or_else(||check["error"].as_str())
                .unwrap_or("").chars().take(900).collect::<String>()))
        .collect::<Vec<_>>();
    Ok(Some(json!({"candidateId":id,"parentJobId":parent,
        "baseCommit":worktree.base,"paths":candidate["paths"],
        "repairKind":repair_kind,
        "qualityMetrics":if quality_plateau {json!({
            "pairedCases":candidate["evaluation"]["metrics"]["pairedCases"],
            "improvedHoldouts":candidate["evaluation"]["metrics"]["improvedHoldouts"],
            "regressions":candidate["evaluation"]["metrics"]["regressions"],
            "verdict":candidate["evaluation"]["verdict"]})}else{Value::Null},
        "compilerErrors":failures,
        "patchSha256":candidate["patchSha256"]})))
}

fn classify(patch:&str)->&'static str {
    if patch.lines().any(|line|line.starts_with("diff --git ")
        && (line.contains("src/engine/rsi.rs ")
            || line.contains("src/engine/rsi_worktree.rs ")
            || line.contains("src/engine/rsi_evolve.rs ")
            || line.contains("src/engine/rsi_metrics.rs ")
            || line.contains("src/api/rsi.rs "))) {
        "improver"
    } else if patch.lines().filter(|line|line.starts_with("diff --git "))
        .all(|line|line.contains("src/engine/memory")
            || line.contains("src/engine/algorithms.rs")
            || line.contains("src/engine/room_state.rs")) {
        "algorithm"
    } else {"harness"}
}
/// Retain the FIRST actionable compiler diagnostic as well as its final
/// summary. Cargo often prints several thousand bytes after the first
/// " --> path:line:column" location; tail-only truncation hid the source
/// location from the model's actual compiler-repair turn.
fn compiler_diagnostic_excerpt(value:&str)->String {
    const HEAD:usize=2600;
    const TAIL:usize=1800;
    let total=value.chars().count();
    if total<=HEAD+TAIL {return value.to_string();}
    let beginning=value.chars().take(HEAD).collect::<String>();
    let end=value.chars().skip(total-TAIL).collect::<String>();
    format!("{beginning}\n[{} diagnostic characters omitted]\n{end}",total-HEAD-TAIL)
}
fn check_result(label:&str, result:Result<std::process::Output,String>)->Value {
    match result {
        Ok(output)=>json!({
            "label":label,"ran":true,"passed":output.status.success(),
            "exitCode":output.status.code(),
            "stdout":String::from_utf8_lossy(&output.stdout).chars().rev().take(2600).collect::<String>().chars().rev().collect::<String>(),
            "stderr":compiler_diagnostic_excerpt(&String::from_utf8_lossy(&output.stderr))
        }),
        Err(error)=>json!({"label":label,"ran":false,"passed":false,"error":error}),
    }
}
async fn validate(worktree:&Worktree,patch:&str)->Vec<Value> {
    let mut checks=Vec::new();
    let rust_change=patch.lines().any(|line|line.starts_with("diff --git a/src/")||line.starts_with("diff --git a/tests/"));
    let web_change=patch.lines().any(|line|line.starts_with("diff --git a/web/"));
    if rust_change {
        let mut command=Command::new(cargo_executable());
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
            let mut command=Command::new(npm_executable());
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
    let mut unsafe_audit=Value::Null;
    let unchanged_inherited=input["rsiRepairCandidateId"].as_str()
        .and_then(|id|context.state.store.get("rsi_candidate",id))
        .is_some_and(|inherited|inherited["patchSha256"].as_str()==Some(patch_sha.as_str()));
    if patch.trim().is_empty(){
        capture_error="모델이 실제 소스 파일을 변경하지 않았습니다.".into();
    } else if unchanged_inherited {
        // A restored but unmodified broken patch is NOT a new model
        // implementation, regardless of how many tool calls were made.
        capture_error="복원한 개선 후보를 이번 세대에서 실제로 수정하지 않았습니다.".into();
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
            "cases":input["rsiFrozenCases"],"oracleVersion":"rsi-worktree-v2",
            "generatorModel":input["model"],"generatorEffort":input["reasoningEffort"],
            "generatorSourceCommit":worktree.base
        });
        match rsi::propose(&context.state,&proposal) {
            Ok(mut candidate)=>{
                candidate["rsiJobId"]=json!(context.job_id);
                candidate["origin"]="model_driven_detached_worktree".into();
                candidate["pairedComparisonComplete"]=json!(false);
                candidate["reviewRequired"]=json!(true);
                candidate["experimentCampaignId"]=input["rsiCampaignId"].clone();
                candidate["actualGeneratorCommit"]=input["rsiGeneratorCommit"].clone();
                candidate_id=candidate["id"].clone();
                context.state.store.put("rsi_candidate",&candidate);
                checks=validate(worktree,&patch).await;
                // An added `unsafe` construct is checked against the same
                // SAFETY rule as the CLI and the `unsafe_audit` tool. This is
                // an actual engine invocation, not a prompt instruction.
                if let Some(audit)=unsafe_audit_for_patch(&worktree.path,&patch) {
                    let check=unsafe_audit_check(&audit);
                    checks.push(check);
                    candidate["unsafeAudit"]=audit.clone();
                    context.state.store.put("rsi_candidate",&candidate);
                    unsafe_audit=audit;
                }
            }
            Err(err)=>capture_error=format!("후보 등록 거부: {err}"),
        }
    }
    let source_clean=git(&worktree.source,&["status","--porcelain","--untracked-files=normal"])
        .await.map(|v|v.is_empty()).unwrap_or(false);
    let model_error=model_result.as_ref().err().cloned().unwrap_or_default();
    let all_checks_pass=!checks.is_empty()&&checks.iter().all(|item|item["passed"]==true);
    let result=json!({
        "restoredParentCandidateId":input["rsiRepairCandidateId"],
        "restoredParentJobId":input["rsiRepairParentJobId"],
        "id":format!("rsi-run-{}",context.job_id),
        "jobId":context.job_id,"sessionId":session_id,
        "baseCommit":worktree.base,"candidateId":candidate_id,
        "sourceClean":source_clean,"diffSha256":patch_sha,"diffBytes":patch_size,
        "checks":checks,"checksPassed":all_checks_pass,
        "unsafeAudit":unsafe_audit,
        "pairedComparisonComplete":false,"autoMerged":false,"autoDeployed":false,
        "modelError":model_error,"captureError":capture_error,
        "modelMessageId":model_result.ok().flatten().and_then(|m|m["id"].as_str().map(str::to_string)),
        "createdAt":chrono::Utc::now().to_rfc3339(),
    });
    context.state.store.put("rsi_run",&result);
    context.state.store.audit("rsi.run_evidence",json!({
        "jobId":context.job_id,"candidateId":candidate_id,
        "patchSha256":patch_sha,"sourceClean":source_clean,"checksPassed":all_checks_pass,
        "unsafeAuditPassed":unsafe_audit["passed"],
        "modelError":model_error,"captureError":capture_error
    }));
    context.checkpoint(json!({"rsiPhase":"review","rsiEvidenceId":result["id"],
        "rsiCandidateId":candidate_id}))?;
    let (status,reason)=run_status(
        candidate_id.is_string(),all_checks_pass,
        unsafe_audit["passed"]==false,
        source_clean,&model_error,&capture_error,
    );
    Ok(json!({
        "status":status,"reason":reason,
        "sessionId":session_id,
        "rsi":result,
    }))
}

/// Runs the shared audit only when the patch adds an unsafe construct. The
/// automatic check is scoped to files the patch changed, so a pre-existing
/// issue in an unrelated file cannot block the candidate.
pub(crate) fn unsafe_audit_for_patch(workspace:&Path,patch:&str)->Option<Value>{
    if !crate::engine::unsafe_audit::needs_audit(patch){return None;}
    let mut paths=Vec::new();
    for line in patch.lines(){
        let Some(path)=line.strip_prefix("+++ b/") else{continue};
        if path=="dev/null"||!crate::engine::rsi::safe_path(path)||paths.iter().any(|item|item==path){continue;}
        paths.push(path.to_string());
    }
    if paths.is_empty(){
        return Some(crate::engine::unsafe_audit::audit_workspace(workspace));
    }
    let mut scanned=0u64;
    let mut findings=Vec::new();
    for relative in paths{
        let path=workspace.join(&relative);
        if !path.is_file(){continue;}
        let report=crate::engine::unsafe_audit::audit_path(&path);
        scanned+=report["scanned"].as_u64().unwrap_or(0);
        if let Some(rows)=report["findings"].as_array(){
            findings.extend(rows.iter().cloned());
        }
    }
    findings.sort_by(|a,b|a.as_str().cmp(&b.as_str()));
    Some(json!({
        "scanned":scanned,"findings":findings,"passed":findings.is_empty(),
        "scope":"changed_files",
    }))
}

/// Converts the audit report into the existing verification-check shape so a
/// missing SAFETY reason blocks the candidate through the normal repair loop.
pub(crate) fn unsafe_audit_check(audit:&Value)->Value{
    let passed=audit["passed"]==true;
    json!({
        "label":"unsafe_safety","passed":passed,
        "error":if passed {""} else {"unsafe 블록에 SAFETY 근거가 없습니다."},
        "findings":audit["findings"],
    })
}

/// Compilation and typecheck are only candidate-integrity evidence.
/// A completed run means the model produced a patch and verification ran
/// without error; it does NOT mean the patch improves runtime behavior.
fn run_status(
    has_candidate:bool,checks_passed:bool,audit_failed:bool,source_clean:bool,
    model_error:&str,capture_error:&str,
)->(&'static str,&'static str){
    if !source_clean {return ("needs_review","original_source_changed");}
    if !capture_error.is_empty()||!has_candidate {
        return ("needs_review",if !model_error.is_empty() {"model_error"}
            else {"no_valid_candidate"});
    }
    if audit_failed{return ("needs_review","unsafe_safety_failed");}
    // A model stream may fail AFTER a real tool-backed patch was written.
    // The compiler result is still actionable. Do not discard it in favor
    // of a transport error and strand the candidate without repair.
    if !checks_passed{return ("needs_review","build_or_typecheck_failed");}
    // Once a real source diff independently passes all integrity checks,
    // a late LLM stream failure cannot veto frozen paired evaluation.
    // Keep modelError in the run receipt; do not mistake transport for code.
    ("completed","candidate_compiles_not_performance_verified")
}

/// Return a short, grounded repair instruction. Do not repeat blind model
/// calls for provider errors, missing diffs or checkout integrity failures.
pub fn repair_feedback(result:&Value)->Option<String>{
    if result["status"]!="needs_review"
        ||!matches!(result["reason"].as_str(),
            Some("build_or_typecheck_failed"|"unsafe_safety_failed"))
        ||result["rsi"]["sourceClean"]!=true
        ||!result["rsi"]["captureError"].as_str().unwrap_or("").is_empty(){
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
/// Rehydrate the *exact* file that rustc says failed rather than asking
/// a small model to repair from a truncated stderr string. The source digest
/// and numbered lines match workspace_read_range's edit protocol. The
/// compiler message itself never chooses an unrestricted filesystem path.
pub fn compiler_repair_context(workspace:&Path,result:&Value)->Option<String>{
    let checks=result["rsi"]["checks"].as_array()?;
    let mut output=Vec::<String>::new();
    let mut seen=std::collections::BTreeSet::<String>::new();
    let root=std::fs::canonicalize(workspace).ok()?;
    for check in checks.iter().filter(|row|row["passed"]==false) {
        let diagnostic=check["stderr"].as_str()
            .or_else(||check["error"].as_str()).unwrap_or("");
        for raw_line in diagnostic.lines() {
            // rustc emits ANSI colors in GitHub Actions. Strip escape codes
            // before extracting its " --> path:line:column" source location.
            let mut clean=String::new();
            let mut escaping=false;
            for character in raw_line.chars() {
                if character=='\u{1b}' { escaping=true;continue; }
                if escaping {
                    if character.is_ascii_alphabetic(){escaping=false;}
                    continue;
                }
                clean.push(character);
            }
            let Some((_,location))=clean.split_once("-->") else {continue;};
            let Some(candidate)=location.split_whitespace().next() else {continue;};
            let Some((path_and_line,column))=candidate.rsplit_once(':') else {continue;};
            let Some((relative,line))=path_and_line.rsplit_once(':') else {continue;};
            if column.parse::<usize>().is_err(){continue;}
            let Ok(line)=line.parse::<usize>() else {continue;};
            if line==0 || !crate::engine::rsi::safe_path(relative)
                || !seen.insert(relative.to_string()) {continue;}
            let Ok(absolute)=std::fs::canonicalize(root.join(relative)) else {continue;};
            if !absolute.starts_with(&root) || !absolute.is_file(){continue;}
            let Ok(source)=std::fs::read_to_string(&absolute) else {continue;};
            if source.len()>1024*1024 {continue;}
            let lines=source.lines().collect::<Vec<_>>();
            if line>lines.len() || lines.is_empty(){continue;}
            let start=line.saturating_sub(9).max(1);
            let end=line.saturating_add(9).min(lines.len());
            // Do not prepend source line numbers to the repair payload.
            // The real-model log showed the model pasting decorated excerpts
            // directly into workspace_replace_range, corrupting Rust syntax.
            // Exact line locations and the full-file SHA remain separate.
            let excerpt=lines[start-1..end].join("\n");
            let sha=format!("{:x}",Sha256::digest(source.as_bytes()));
            output.push(format!(
                "실제 컴파일 오류 파일: {relative}; 오류 줄: {line}; \
                 현재 전체 파일 sourceSha256: {sha}; 검증된 원문 {start}~{end}줄 (아래 원문에는 줄 번호가 포함되지 않습니다):\n{excerpt}\n\
                 이 파일의 오류를 원문 기준으로 고치세요. newText에는 소스 코드만 넣고 줄 번호와 마크다운 장식은 넣지 마세요. 수정 도구: \
                 workspace_replace_range(path, startLine, endLine, newText, expectedSha256={sha}) \
                 또는 workspace_replace(path, oldText, newText). \
                 이전 변경을 유지하고 필요한 부분만 고치세요."
            ));
            if output.len()>=2{break;}
        }
        if output.len()>=2{break;}
    }
    if output.is_empty(){None}else{Some(output.join("\n\n"))}
}

/// Apply a model-generated patch only after explicit owner review. A successful
/// compile is NOT proof of improved behavior. Never commit, push or deploy.
pub async fn apply_to_sub(state:&crate::state::AppState,id:&str,body:&Value)->Result<Value,String>{
    use std::sync::OnceLock;
    static APPLY_LOCK:OnceLock<tokio::sync::Mutex<()>>=OnceLock::new();
    let _lock=APPLY_LOCK.get_or_init(||tokio::sync::Mutex::new(())).lock().await;
    let mut candidate=state.store.get("rsi_candidate",id).ok_or("개선 후보를 찾을 수 없습니다.")?;
    if !matches!(candidate["status"].as_str(),Some("proposed"|"inconclusive"|"verified")) {
        return Err("현재 상태에서는 적용할 수 없습니다.".into());
    }
    if !matches!(body["confirmation"].as_str(),Some("APPLY_UNVERIFIED_TO_SUB"|"APPLY_VERIFIED_TO_SUB"))
        || body["baseCommit"]!=candidate["baseCommit"]
        || body["patchSha256"]!=candidate["patchSha256"] {
        return Err("기준 커밋·패치 해시·수동 적용 의사를 확인하세요.".into());
    }
    let job_id=candidate["rsiJobId"].as_str().ok_or("실행 증거가 없습니다.")?.to_string();
    let record_id=format!("rsi-run-{job_id}");
    let mut evidence=state.store.get("rsi_run",&record_id).ok_or("실제 실행 증거가 없습니다.")?;
    if evidence["candidateId"]!=candidate["id"] || evidence["diffSha256"]!=candidate["patchSha256"]
        || evidence["checksPassed"]!=true || evidence["sourceClean"]!=true {
        return Err("코드 출처 및 컴파일·타입 검사 통과 증거를 만족하지 못했습니다.".into());
    }
    let verified=candidate["status"]=="verified"
        &&candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        &&candidate["evaluation"]["metrics"]["eligible"]==true
        &&evidence["pairedComparisonComplete"]==true
        &&candidate["evaluation"]["evidenceSha256"].is_string()
        &&candidate["evaluation"]["evidenceSha256"]==evidence["paired"]["reportSha256"];
    if (candidate["target"]=="improver"||body["confirmation"]=="APPLY_VERIFIED_TO_SUB")&&!verified {
        return Err("독립 비교에서 우세함이 확인된 개선기만 적용할 수 있습니다.".into());
    }
    if evidence["pairedComparisonComplete"]==true && candidate["evaluation"]["metrics"]["regressions"].as_u64()!=Some(0){
        return Err("비교에서 회귀가 발견된 후보는 적용할 수 없습니다.".into());
    }
    let patch=candidate["patch"].as_str().ok_or("패치 본문이 없습니다.")?;
    if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {
        return Err("패치 해시가 저장된 값과 다릅니다.".into());
    }
    let status=crate::engine::dev::status(state).await;
    let branch=status["branch"].as_str().unwrap_or("");
    if status["git"]!=true || !is_work_branch(branch)
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
        "mode":if verified{"explicit_verified_paired_application"}else{"explicit_trial_without_paired_proof"},
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

/// Publish the selected experimental lineage to the test branch. The main
/// branch is never a target. Every commit is checked against its measured
/// patch using a private Git index before the source checkout can move.
/// Work branches are the per-session `dominion-sub*` branches plus the
/// integration branch. Other branches, including production refs, are never
/// valid RSI sources or application targets.
pub(crate) fn is_work_branch(name: &str) -> bool {
    matches!(name, "dominion-champion" | "dominion-sub" | "dominion-sub1" | "dominion-sub2" | "dominion-sub3" | "dominion-sub4")
}

pub async fn apply_campaign_to_sub(state:&crate::state::AppState,id:&str,expected:&str)->Result<Value,String>{
    if !state.begin_session("rsi-source-application") {return Err("검증된 버전을 적용 중입니다.".into());}
    let _guard=state.session_guard("rsi-source-application");
    let mut campaign=state.store.get("rsi_campaign",id).ok_or("개선 작업을 찾지 못했습니다.")?;
    if !matches!(campaign["status"].as_str(),Some("completed"|"cancelled"|"needs_review"|"failed")) || campaign["headCommit"]==campaign["baseCommit"]
        || campaign["headCommit"]!=expected || !valid_sha(expected) {
        return Err("완료된 작업에서 선택한 검증 버전만 적용할 수 있습니다.".into());
    }
    let source=std::fs::canonicalize(campaign["sourceDir"].as_str().ok_or("소스 경로 없음")?).map_err(|e|e.to_string())?;
    let status=crate::engine::dev::status(state).await;
    let configured=status["config"]["sourceDir"].as_str().and_then(|s|std::fs::canonicalize(s).ok());
    let branch=status["branch"].as_str().unwrap_or("");
    if configured.as_ref()!=Some(&source) || !is_work_branch(branch) {
        return Err("연결된 DOMINION 검증 작업 브랜치에만 적용합니다.".into());
    }
    // The selected campaign must belong to this exact checkout. Never let a
    // previously finished campaign silently publish to another experiment.
    let expected_branch=campaign["sourceBranch"].as_str().unwrap_or(branch);
    if expected_branch!=branch {
        return Err("캠페인을 실행한 작업 브랜치와 적용 대상이 다릅니다.".into());
    }
    if !git(&source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty() {
        return Err("미저장 코드를 보존했습니다. 현재 소스를 정리한 뒤 반영하세요.".into());
    }
    if campaign["application"]["commit"]==expected && status["commit"]==expected {
        return Ok(campaign["application"].clone());
    }
    if campaign["sourceSnapshot"]==true || status["commit"]!=campaign["baseCommit"]
        || !git(&source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty() {
        return Err("작업 시작 이후 소스가 바뀌었거나 미저장 코드가 있어 보존했습니다. 현재 소스에서 다시 이어가세요.".into());
    }
    if state.store.all("job").iter().any(|j|matches!(j["kind"].as_str(),Some("rsi_evolve"|"rsi_generation"))
        && matches!(j["status"].as_str(),Some("queued"|"running"))) {
        return Err("진행 중인 발전 작업이 끝난 뒤 적용하세요.".into());
    }
    let base=campaign["baseCommit"].as_str().ok_or("기준 커밋 없음")?;
    git(&source,&["merge-base","--is-ancestor",base,expected]).await?;
    let commits=git(&source,&["rev-list","--reverse",&format!("{base}..{expected}")]).await?;
    let candidates=state.store.all("rsi_candidate");
    for commit in commits.lines() {
        let candidate=candidates.iter().find(|c|c["experimentAdoption"]["commit"]==commit
            && c["status"]=="verified" && c["evaluation"]["metrics"]["eligible"]==true
            && c["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees")
            .ok_or("검증되지 않은 커밋이 섞여 있어 적용하지 않았습니다.")?;
        rsi::verify_paired_receipt(state,candidate)?;
        let parent=git(&source,&["rev-parse",&format!("{commit}^")]).await?;
        if candidate["baseCommit"]!=parent {return Err("검증한 기준과 채택 계보가 다릅니다.".into());}
        let patch=candidate["patch"].as_str().ok_or("검증 패치 없음")?;
        if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {return Err("검증 패치 해시가 다릅니다.".into());}
        let temp=std::env::temp_dir().join(format!("dominion-rsi-publish-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).map_err(|e|e.to_string())?;
        let patch_path=temp.join("candidate.patch");
        std::fs::write(&patch_path,patch).map_err(|e|e.to_string())?;
        let checked=async {
            let mut tree=String::new();
            for args in [vec!["read-tree".to_string(),parent.clone()],
                vec!["apply".into(),"--cached".into(),patch_path.to_string_lossy().to_string()],vec!["write-tree".into()]] {
                let mut cmd=Command::new("git");
                cmd.current_dir(&source).args(args).env("GIT_INDEX_FILE",temp.join("index"));
                let out=run(cmd,GIT_LIMIT).await?;
                if !out.status.success(){return Err("검증 패치를 재구성하지 못했습니다.".to_string());}
                tree=String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
            if tree!=git(&source,&["rev-parse",&format!("{commit}^{{tree}}")]).await? {
                return Err("실제 채택 코드가 검증 패치와 다릅니다.".into());
            }
            Ok::<(),String>(())
        }.await;
        let _=std::fs::remove_dir_all(temp);checked?;
    }
    if commits.is_empty() {return Err("적용할 검증 변경이 없습니다.".into());}
    if git(&source,&["rev-parse","HEAD"]).await?!=base
        || git(&source,&["symbolic-ref","--quiet","HEAD"]).await?!=format!("refs/heads/{branch}")
        || !git(&source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty() {
        return Err("적용 직전에 작업 브랜치나 소스가 바뀌었습니다.".into());
    }
    git(&source,&["-c","core.hooksPath=","merge","--ff-only","--no-edit",expected]).await?;
    let application=json!({"commit":expected,"branch":branch,"sourceDir":source,
        "requiresRebuild":true,"productionDeployed":false,"appliedAt":chrono::Utc::now().to_rfc3339()});
    campaign["application"]=application.clone();
    state.store.put("rsi_campaign",&campaign).ok_or("소스 반영 후 기록 저장 실패")?;
    state.bus.record("rsi_campaign",&campaign);
    state.store.audit("rsi.campaign_applied_to_sub",json!({"campaignId":id,"commit":expected}));
    Ok(application)
}

/// Execute the existing paired-runner against the frozen baseline and the
/// exact model patch. Tests run on the operator's host: Git worktrees provide
/// source isolation, NOT an OS security sandbox. Never call this from a model.
pub async fn compare(context:&JobContext, job:&Value)->Result<Value,String>{
    let id=job["input"]["candidateId"].as_str().ok_or("비교할 후보가 없습니다.")?;
    let status=crate::engine::dev::status(&context.state).await;
    let branch=status["branch"].as_str().unwrap_or("");
    if status["git"]!=true || !is_work_branch(branch) {
        return Err("비교 실험은 dominion-sub 소스에서 시작하세요.".into());
    }
    let source=std::fs::canonicalize(status["config"]["sourceDir"].as_str().ok_or("소스 경로 없음")?).map_err(|e|e.to_string())?;
    compare_at(context,id,&source,&source).await
}

pub(crate) async fn compare_at(context:&JobContext,id:&str,source:&Path,_runner_source:&Path)->Result<Value,String>{
    let state=&context.state;
    let candidate=state.store.get("rsi_candidate",id)
        .ok_or("비교할 후보 기록이 없습니다.")?;
    if !matches!(candidate["status"].as_str(),Some("proposed"|"inconclusive")) {
        return Err("평가 대기 또는 근거 부족 후보만 재비교할 수 있습니다.".into());
    }
    if candidate["cases"].as_array().is_none_or(|c|c.len()<rsi::MIN_PAIRED_CASES) {
        return Err("고정된 평가 사례가 8개 미만이므로 승격 평가를 실행할 수 없습니다.".into());
    }
    let patch=candidate["patch"].as_str().ok_or("코드 패치가 없습니다.")?;
    if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {
        return Err("평가 전 코드 패치 SHA-256이 일치하지 않습니다.".into());
    }
    let prior=state.store.get("rsi_run",
        &format!("rsi-run-{}",candidate["rsiJobId"].as_str().unwrap_or("")))
        .ok_or("모델이 생성한 후보의 원본 실행 증거가 없습니다.")?;
    if prior["candidateId"]!=candidate["id"]||prior["checksPassed"]!=true {
        return Err("컴파일 검사를 통과한 현재 실행 후보만 A/B 실험을 실행합니다.".into());
    }
    if !git(source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty(){
        return Err("비교할 기준 작업공간에 추적 중인 변경이 있습니다.".into());
    }
    // Older frozen candidates remain evaluable after the checkout advances.
    // Both lab variants are created from candidate.baseCommit, never HEAD.
    git(source,&["cat-file","-e",&format!("{}^{{commit}}",candidate["baseCommit"].as_str().ok_or("기준 커밋 없음")?)]).await?;
    let output_dir=state.config.data_dir.join("rsi-paired")
        .join(format!("{}-{}",id,uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&output_dir).map_err(|e|e.to_string())?;
    let output_dir=std::fs::canonicalize(output_dir).map_err(|e|e.to_string())?;
    let patch_file=output_dir.join("candidate.patch");
    let plan_file=output_dir.join("plan.json");
    let report_file=output_dir.join("report.json");
    std::fs::write(&patch_file,patch).map_err(|e|e.to_string())?;
    let runner=output_dir.join("rsi-lab.sh");
    std::fs::write(&runner,include_str!("../../scripts/rsi-lab.sh")).map_err(|e|e.to_string())?;
    std::fs::write(output_dir.join("rsi-measure.mjs"),include_str!("../../scripts/rsi-measure.mjs")).map_err(|e|e.to_string())?;
    let mut plan=json!({"oracleVersion":candidate["oracleVersion"],"cases":candidate["cases"]});
    if candidate["cases"].as_array().into_iter().flatten().any(|c|c["filter"].as_str().is_some_and(|f|f.starts_with("rsi_frozen_oracle::"))){
        let oracle=include_str!("rsi_oracle_fixture.rs");
        let file=std::fs::canonicalize(&output_dir).map_err(|e|e.to_string())?.join("frozen-oracle.rs");
        std::fs::write(&file,oracle).map_err(|e|e.to_string())?;
        plan["oracleFixture"]=json!(file);plan["oracleSha256"]=json!(digest(oracle));
    }
    std::fs::write(&plan_file,plan.to_string()).map_err(|e|e.to_string())?;
    context.checkpoint(json!({
        "rsiCompareInFlight":true,"rsiPhase":"paired_experiment",
        "rsiCandidateId":id
    }))?;
    let mut path_parts = Vec::<PathBuf>::new();
    for tool in [cargo_executable(), npm_executable(), node_executable()] {
        if let Some(parent) = tool.parent() {
            path_parts.push(parent.to_path_buf());
        }
    }
    path_parts.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(path_parts).map_err(|e|e.to_string())?;
    let mut command=Command::new("bash");
    command.current_dir(&source).arg(&runner)
        .arg(candidate["baseCommit"].as_str().unwrap_or(""))
        .arg(&patch_file).arg(&plan_file).arg(&report_file)
        .env("PATH", path)
        .env("RSI_BUILD_CACHE",std::fs::canonicalize(&state.config.data_dir).map_err(|e|e.to_string())?.join("rsi-paired-build").join(&context.job_id));
    let outcome=crate::engine::rsi_evolve::command(context,command,Duration::from_secs(1200)).await?;
    if !outcome.status.success() {
        return Err(format!("독립 A/B 실험 실행 실패: {}",String::from_utf8_lossy(&outcome.stderr)
            .chars().rev().take(1200).collect::<String>().chars().rev().collect::<String>()));
    }
    let report_text=std::fs::read_to_string(&report_file).map_err(|e|e.to_string())?;
    let report:Value=serde_json::from_str(&report_text).map_err(|e|e.to_string())?;
    let evidence_path=output_dir.join("report.json.evidence.txt");
    let evidence_meta=std::fs::metadata(&evidence_path).map_err(|e|e.to_string())?;
    if evidence_meta.len()>24*1024*1024 {
        return Err("실험 출력이 증거 보관 상한(24MiB)을 초과했습니다.".into());
    }
    let evidence=std::fs::read(&evidence_path).map_err(|e|e.to_string())?;
    let evidence_sha=format!("{:x}",Sha256::digest(&evidence));
    if report["reportSha256"].as_str()!=Some(evidence_sha.as_str())
        || report["baseCommit"]!=candidate["baseCommit"]
        || report["patchSha256"]!=candidate["patchSha256"]
        || report["oracleVersion"]!=candidate["oracleVersion"] {
        return Err("원본·패치·평가 기준 또는 실행 증거의 해시가 일치하지 않습니다.".into());
    }
    let mut updated=rsi::evaluate(state,id,&report)?;
    let verdict=updated["evaluation"]["metrics"]["verdict"].clone();
    let metrics=updated["evaluation"]["metrics"].clone();
    // This evaluation came from a real local script invocation, not a
    // client-supplied verdict. It is still host execution, not a VM sandbox.
    updated["pairedComparisonComplete"]=json!(true);
    updated["evaluation"]["source"]=json!("server_invoked_local_paired_git_worktrees");
    updated["evaluation"]["evidenceSha256"]=json!(evidence_sha);
    state.store.put("rsi_candidate",&updated)
        .ok_or("A/B 평가 후보 갱신에 실패했습니다.")?;
    state.store.put("rsi_evaluation",&updated["evaluation"]);
    let receipt=json!({
        // A campaign can run multiple candidate comparisons. The old
        // job-scoped receipt ID overwrote earlier generations' original
        // paired results and made later lineage verification impossible.
        "id":format!("rsi-pair-{}-{}",id,updated["evaluation"]["id"].as_str().unwrap_or("")),
        "candidateId":id,"jobId":context.job_id,
        "evaluationId":updated["evaluation"]["id"],
        "report":report,"verdict":verdict,"metrics":metrics,
        "evidenceSha256":evidence_sha,
        "runner":"local_gitrepo_not_os_sandbox",
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    state.store.put("rsi_pair",&receipt);
    let origin_id=format!("rsi-run-{}",candidate["rsiJobId"].as_str().unwrap_or(""));
    if let Some(mut origin)=state.store.get("rsi_run",&origin_id) {
        origin["paired"]=json!({"verdict":verdict,"metrics":metrics,
            "reportSha256":evidence_sha,"comparedAt":receipt["createdAt"]});
        origin["pairedComparisonComplete"]=json!(true);
        state.store.put("rsi_run",&origin);
    }
    state.store.audit("rsi.paired_evaluation",
        json!({"id":id,"verdict":verdict,"evidenceSha256":evidence_sha}));
    context.checkpoint(json!({
        "rsiCompareInFlight":false,"rsiPhase":"paired_review"
    }))?;
    Ok(json!({
        "candidateId":id,"candidateStatus":updated["status"],
        "verdict":verdict,"metrics":metrics,
        "evidenceSha256":evidence_sha,
        "pairedCases":report["cases"].as_array().map(Vec::len).unwrap_or(0),
        "performanceImprovementClaimed":metrics["metricGains"].as_u64().unwrap_or(0)>0,
        "measurementScope":"frozen_native_cases_only"
    }))
}

pub async fn cleanup(worktree:&Worktree)->Result<(),String>{
    let name=worktree.path.to_str().ok_or("작업공간 경로 오류")?;
    git(&worktree.source,&["worktree","remove","--force",name]).await.map(|_|())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rsi_resolves_cargo_even_with_a_minimal_service_path() {
        let cargo = cargo_executable();
        assert!(
            cargo.is_file() || cargo == Path::new("cargo"),
            "unexpected cargo path: {cargo:?}"
        );
        assert!(!npm_executable().as_os_str().is_empty());
        let node = node_executable();
        assert!(
            node.is_file() || node == Path::new("node"),
            "unexpected node path: {node:?}"
        );
    }
    #[test]
    fn only_known_work_branches_can_receive_a_verified_campaign() {
        for branch in ["dominion-sub", "dominion-sub1", "dominion-sub2",
            "dominion-sub3", "dominion-sub4", "dominion-champion"] {
            assert!(is_work_branch(branch), "{branch}");
        }
        for branch in ["main", "master", "dominion-sub1-other",
            "dominion-sub5", "dominion-champion-old", "refs/heads/main", ""] {
            assert!(!is_work_branch(branch), "{branch}");
        }
    }

    #[test]
    fn later_generations_see_prior_compile_failures_without_fabricated_gains(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!("rsi-feedback-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("job",&json!({"id":"j1","input":{"rsiObjective":"correct Rust type failure"}}));
        state.store.put("rsi_run",&json!({"id":"rsi-run-j1","jobId":"j1",
            "candidateId":"rsi-one","createdAt":"2026-11-09T01:00:00Z",
            "checksPassed":false,"checks":[{"label":"cargo check --all-targets","passed":false}],
            "modelError":"","captureError":""}));
        let prompt=recent_feedback(&state);
        assert!(prompt.contains("correct Rust type failure"));
        assert!(prompt.contains("정적 검사 실패"));
        assert!(prompt.contains("cargo check --all-targets"));
        assert!(!prompt.contains("실제 성능 향상 검증"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn classification_covers_the_improver_harness_and_algorithm(){
        assert_eq!(classify("diff --git a/src/engine/rsi.rs b/src/engine/rsi.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/api/rsi.rs b/src/api/rsi.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/rsi_evolve.rs b/src/engine/rsi_evolve.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/room_state.rs b/src/engine/room_state.rs\n"),"algorithm");
        assert_eq!(classify("diff --git a/web/src/pages/Home.tsx b/web/src/pages/Home.tsx\n"),"harness");
    }
    #[test]
    fn failing_verification_never_completes_and_only_valid_failures_retry(){
        assert_eq!(run_status(true,false,false,true,"",""),("needs_review","build_or_typecheck_failed"));
        assert_eq!(run_status(true,false,true,true,"",""),("needs_review","unsafe_safety_failed"));
        assert_eq!(run_status(true,true,false,true,"",""),("completed","candidate_compiles_not_performance_verified"));
        assert_eq!(run_status(true,true,false,false,"","").0,"needs_review");
        assert_eq!(run_status(true,true,false,true,"provider timeout",""),
            ("completed","candidate_compiles_not_performance_verified"),
            "A verifiably compiled source patch must reach paired evaluation despite a late model stream error");
        assert_eq!(run_status(false,true,false,true,"","invalid diff").0,"needs_review");
        let failing=json!({"status":"needs_review","reason":"build_or_typecheck_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"cargo check --all-targets","passed":false,"stderr":"error[E0308]: wrong type"}]}});
        assert!(repair_feedback(&failing).unwrap().contains("error[E0308]"));
        let unsafe_failure=json!({"status":"needs_review","reason":"unsafe_safety_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"unsafe_safety","passed":false,"error":"unsafe 블록에 SAFETY 근거가 없습니다."}]}});
        assert!(repair_feedback(&unsafe_failure).unwrap().contains("SAFETY"));
        let mut late_transport_failure=failing;
        late_transport_failure["rsi"]["modelError"]=json!("provider stream closed after edit");
        assert!(repair_feedback(&late_transport_failure).is_some(),
            "A compiler-rejected real patch must still be repairable after a late stream error");
        assert_eq!(run_status(true,false,false,true,"stream closed",""),
            ("needs_review","build_or_typecheck_failed"));
    }
    #[test]
    fn sub1_compiler_errors_keep_first_location_after_long_cargo_output() {
        let raw=format!(
            "error[E0308]: mismatched types\n --> src/engine/decompose.rs:44:9\n{}\nerror: could not compile",
            "note: dependency warning\n".repeat(350));
        let reduced=compiler_diagnostic_excerpt(&raw);
        assert!(reduced.contains("src/engine/decompose.rs:44:9"));
        assert!(reduced.contains("error: could not compile"));
        assert!(reduced.len()<raw.len());
        assert_eq!(compiler_diagnostic_excerpt("short compiler failure"),"short compiler failure");
    }
    #[test]
    fn compiler_repair_rehydrates_current_source_from_ansi_rustc_location() {
        let root=std::env::temp_dir().join(format!("sub1-compiler-repair-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        let source="fn left() {\n    let x = 2;\n    println!(\"{}\", x);\n}\n";
        std::fs::write(root.join("src/engine/example.rs"),source).unwrap();
        let failure=json!({"rsi":{"checks":[{"passed":false,
            "stderr":"error: expected expression\n\u{1b}[1m  --> \u{1b}[0msrc/engine/example.rs:3:5\n"}]}});
        let repair=compiler_repair_context(&root,&failure).unwrap();
        let digest=format!("{:x}",Sha256::digest(source.as_bytes()));
        assert!(repair.contains("src/engine/example.rs"));
        assert!(repair.contains("    println!(\"{}\", x);"));
        assert!(!repair.contains("3 |     println!"),
            "Repair input must be raw source, never numbered source");
        assert!(repair.contains(&digest));
        assert!(repair.contains("workspace_replace_range"));
        let spoof=json!({"rsi":{"checks":[{"passed":false,
            "stderr":"  --> ../secret.rs:2:1\n"}]}});
        assert!(compiler_repair_context(&root,&spoof).is_none());
        let _=std::fs::remove_dir_all(root);
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
        proof["pairedComparisonComplete"]=json!(true);
        state.store.put("rsi_run",&proof);
        let mut compared=state.store.get("rsi_candidate",candidate_id).unwrap();
        compared["status"]=json!("inconclusive");
        compared["evaluation"]=json!({"metrics":{"regressions":0}});
        state.store.put("rsi_candidate",&compared);
        let applied=apply_to_sub(&state,candidate_id,&request).await.unwrap();
        assert_eq!(applied["status"],"applied_to_sub");
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 2\n");
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        assert!(!git_local(&source,&["status","--porcelain"]).is_empty());
        cleanup(&worktree).await.unwrap();
        assert!(!worktree.path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn independently_measured_plateau_restores_real_patch_into_descendant_only() {
        use crate::{config::Config, db::Store, state::AppState, vault::Vault};
        use std::process::Command as SyncCommand;
        fn git_result(path: &Path, args: &[&str]) -> String {
            let output=SyncCommand::new("git").current_dir(path).args(args)
                .output().unwrap();
            assert!(output.status.success(),
                "git {:?}: {}",args,String::from_utf8_lossy(&output.stderr));
            String::from_utf8(output.stdout).unwrap()
        }
        let root=std::env::temp_dir().join(format!("rsi-plateau-worktree-{}",uuid::Uuid::new_v4()));
        let source=root.join("source");
        let data=root.join("data");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(source.join("Cargo.toml"),
            "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
        std::fs::write(source.join("src/engine/rsi.rs"),"// system source\n").unwrap();
        let original="pub fn sample() -> u8 { 1 }\n";
        let changed="pub fn sample() -> u8 { 2 }\n";
        let path="src/engine/sample.rs";
        std::fs::write(source.join(path),original).unwrap();
        git_result(&source,&["init","-q"]);
        git_result(&source,&["checkout","-b","dominion-sub1"]);
        git_result(&source,&["add","."]);
        git_result(&source,&["-c","user.name=CI","-c","user.email=ci@example.com",
            "commit","-qm","baseline"]);
        let base=git_result(&source,&["rev-parse","HEAD"]).trim().to_string();
        std::fs::write(source.join(path),changed).unwrap();
        let patch=git_result(&source,&["diff","--",path]);
        std::fs::write(source.join(path),original).unwrap();
        assert!(!patch.trim().is_empty());
        let cfg=Config{port:0,data_dir:data.clone(),web_dir:data.join("web"),
            workspace:data.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let plan=(0..8).map(|i|json!({
            "id":format!("case-{i}"),
            "domain":if i==1 {"other"} else {"harness"},
            "holdout":i<3,
            "filter":format!("engine::rsi::tests::case_{i}")
        })).collect::<Vec<_>>();
        let mut candidate=crate::engine::rsi::propose(&state,&json!({
            "target":"harness","baseCommit":base,
            "patch":patch,"hypothesis":"Improve real source",
            "cases":plan,"oracleVersion":"frozen-v1",
            "generatorModel":"real-model"
        })).unwrap();
        let id=candidate["id"].as_str().unwrap().to_string();
        candidate["rsiJobId"]=json!("parent-gen");
        candidate["origin"]=json!("model_driven_detached_worktree");
        state.store.put("rsi_candidate",&candidate);
        let raw=json!({
            "baseCommit":base,"patchSha256":candidate["patchSha256"],
            "oracleVersion":"frozen-v1","runner":"local_gitrepo_not_os_sandbox",
            "reportSha256":"b".repeat(64),
            "baselineBuildOk":true,"candidateBuildOk":true,
            "cases":(0..8).map(|i|json!({
                "id":format!("case-{i}"),"baselinePass":true,"candidatePass":true
            })).collect::<Vec<_>>()
        });
        candidate=crate::engine::rsi::evaluate(&state,&id,&raw).unwrap();
        assert_eq!(candidate["status"],"inconclusive");
        candidate["pairedComparisonComplete"]=json!(true);
        candidate["evaluation"]["source"]=json!("server_invoked_local_paired_git_worktrees");
        candidate["evaluation"]["evidenceSha256"]=raw["reportSha256"].clone();
        state.store.put("rsi_candidate",&candidate);
        state.store.put("rsi_pair",&json!({
            "id":"measured-neutral-pair","candidateId":id,
            "evaluationId":candidate["evaluation"]["id"],
            "report":raw,"verdict":"inconclusive",
            "metrics":candidate["evaluation"]["metrics"],
            "evidenceSha256":candidate["evaluation"]["evidenceSha256"],
            "runner":"local_gitrepo_not_os_sandbox"
        }));
        state.store.put("rsi_run",&json!({
            "id":"rsi-run-parent-gen","candidateId":id,
            "baseCommit":base,"checksPassed":true,
            "checks":[{"label":"cargo check --all-targets","ran":true,"passed":true}]
        }));
        state.store.put("job",&json!({
            "id":"child-gen","kind":"rsi_generation","status":"running"
        }));
        let context=JobContext::new(state.clone(),"child-gen".into());
        let child_path=root.join("descendant");
        git_result(&source,&["worktree","add","--detach",
            child_path.to_str().unwrap(),&base]);
        let isolated=Worktree{source:source.clone(),path:child_path.clone(),base:base.clone()};
        let input=json!({"rsiRepairCandidateId":id,"rsiRepairParentJobId":"parent-gen"});
        let repaired=restore_failed_candidate(&context,&input,&isolated).await.unwrap().unwrap();
        assert_eq!(repaired["repairKind"],"quality_plateau");
        assert_eq!(repaired["qualityMetrics"]["improvedHoldouts"],0);
        assert_eq!(std::fs::read_to_string(child_path.join(path)).unwrap(),changed);
        assert_eq!(std::fs::read_to_string(source.join(path)).unwrap(),original);
        assert_eq!(git_result(&source,&["rev-parse","HEAD"]).trim(),base);
        assert!(git_result(&source,&["status","--porcelain"]).trim().is_empty());
        cleanup(&isolated).await.unwrap();
        assert!(!child_path.exists());
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

    #[test]
    fn unsafe_patch_is_audited_and_unrelated_patch_is_not(){
        let root=std::env::temp_dir().join(format!("rsi-unsafe-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"),"unsafe { call(); }\n").unwrap();
        std::fs::write(root.join("src/unrelated.rs"),"unsafe fn unrelated() {}\n").unwrap();
        let unsafe_patch="diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-unsafe { call(); }\n+unsafe { call_again(); }\n";
        let audit=unsafe_audit_for_patch(&root,unsafe_patch).expect("unsafe patch must be audited");
        assert_eq!(audit["passed"],false);
        let check=unsafe_audit_check(&audit);
        assert_eq!(check["label"],"unsafe_safety");
        assert_eq!(check["passed"],false);
        assert!(check["findings"].as_array().unwrap()[0].as_str().unwrap().ends_with("src/lib.rs:1"));
        assert!(check["findings"].as_array().unwrap().iter().all(|row|!row.as_str().unwrap().contains("unrelated.rs")),
            "unrelated pre-existing files must not block this patch");
        assert!(unsafe_audit_for_patch(&root,"diff --git a/src/lib.rs b/src/lib.rs\n+let value = 1;\n").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
