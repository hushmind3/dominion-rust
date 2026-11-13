//! Evidence-producing, isolated source worktree for model-driven DOMINION revisions.
//! A worktree protects the live checkout from ordinary workspace writes; it is
//! NOT a process/OS security sandbox. No candidate is automatically promoted.
use crate::engine::{jobs::JobContext, rsi, tools};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::{Path,PathBuf},time::Duration};
use tokio::process::Command;

#[path = "rsi_worktree/adoption.rs"]
mod adoption;
pub use adoption::{apply_to_sub,apply_campaign_to_sub};
#[path = "rsi_worktree/comparison.rs"]
mod comparison;
pub use comparison::compare;
pub(crate) use comparison::compare_at;

const GIT_LIMIT: Duration = Duration::from_secs(30);
const CHECK_LIMIT: Duration = Duration::from_secs(360);

/// An RSI source checkout must be an explicitly owned development branch,
/// never main or a competing agent's branch. Legacy experiments are supported
/// only when their source provenance is bound to that exact checkout.
pub(crate) fn rsi_work_branch(branch: &str) -> bool {
    matches!(branch, "dominion-champion" | "dominion-sub")
}

/// Legacy experiments did not record a source branch. They may only be
/// applied to the original legacy work branch. Current campaigns must match
/// the exact branch they were evaluated on, including when the checkout's
/// source path is identical.
fn owns_application_branch(current: &str, recorded: Option<&str>) -> bool {
    rsi_work_branch(current) && match recorded.filter(|v| !v.is_empty()) {
        Some(branch) => branch == current,
        None => current == "dominion-sub",
    }
}

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
        ("planning-development","planning","planning_development_quality",false),
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
/// Preload an independently measured, rejected patch into the next
/// generation's disposable checkout. This patch does NOT update the accepted
/// source revision: its child must change actual code and beat the ORIGINAL
/// baseline in the frozen A/B test before any adoption is possible.
async fn inherit_neutral_patch(
    context:&JobContext, path:&Path, base:&str, id:&str,
)->Result<(),String>{
    let candidate=context.state.store.get("rsi_candidate",id)
        .ok_or("계승할 실제 개선 후보를 찾을 수 없습니다.")?;
    let patch=candidate["patch"].as_str().ok_or("계승 후보의 소스 패치 없음")?;
    if crate::engine::rsi_archive::parent_metrics(&context.state, &candidate, base).is_none() {
        return Err("독립 검증되지 않았거나 다른 기준 커밋의 패치는 계승하지 않습니다.".into());
    }
    let temporary=std::env::temp_dir().join(format!(
        "dominion-rsi-neutral-{}.patch",uuid::Uuid::new_v4()));
    std::fs::write(&temporary,patch).map_err(|e|e.to_string())?;
    let name=temporary.to_str().ok_or("계승 패치 경로 오류")?;
    let applied=async {
        git(path,&["apply","--check",name]).await?;
        git(path,&["apply",name]).await?;
        if git(path,&["status","--porcelain","--untracked-files=normal"]).await?.is_empty(){
            return Err("계승 패치가 실제 소스 차이를 남기지 않았습니다.".into());
        }
        Ok::<(),String>(())
    }.await;
    let _=std::fs::remove_file(&temporary);
    applied?;
    context.state.store.audit("rsi.neutral_patch_inherited",json!({
        "generationJobId":context.job_id,"candidateId":id,
        "baseCommit":base,"patchSha256":candidate["patchSha256"],
        "sourceDir":path.to_string_lossy(),
    }));
    Ok(())
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
    if !campaign {
        let actual_branch=git(&source,&["rev-parse","--abbrev-ref","HEAD"]).await?;
        if !owns_application_branch(
            actual_branch.trim(),input["rsiSourceBranch"].as_str()) {
            return Err("원본 RSI 브랜치와 실험 시작 시 기록된 원본 브랜치가 일치하지 않습니다.".into());
        }
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
    if let Some(id)=input["rsiInheritedCandidateId"].as_str().filter(|id|!id.is_empty()) {
        if let Err(error)=inherit_neutral_patch(context,&path,base,id).await {
            let _=git(&source,&["worktree","remove","--force",name]).await;
            return Err(error);
        }
    }
    if let Err(error)=context.checkpoint(json!({
        "rsiWorkspace":name,"rsiBaseCommit":base,
        "rsiPhase":"model_editing","rsiIsolation":"detached_git_worktree_not_os_sandbox"
    })) {
        let _=git(&source,&["worktree","remove","--force",name]).await;
        return Err(error);
    }
    Ok(Worktree{source,path,base:base.to_string()})
}
#[path="capture.rs"]
mod capture;
use capture::classify;
pub use capture::finish;

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
        return ("needs_review","no_valid_candidate");
    }
    // The model can exhaust its output budget AFTER a real source write.
    // Preserve the actual compiler outcome instead of misclassifying a
    // noncompiling patch as merely a model error. This distinction is what
    // drives the next generation's source-repair method.
    if audit_failed{return ("needs_review","unsafe_safety_failed");}
    if !checks_passed{return ("needs_review","build_or_typecheck_failed");}
    // A real patch that passed an independent compiler verification can be
    // compared even if the provider failed during its final prose turn.
    // A user-initiated cancellation remains a hard stop.
    if model_error.contains("중단") || model_error.contains("cancelled")
        || model_error.contains("canceled") {
        return ("needs_review","model_error");
    }
    ("completed","candidate_compiles_not_performance_verified")
}

/// Extract the FIRST actual compiler location from the failed verifier.
/// The next model turn must read this precise file/line in the same detached
/// worktree, not spend another generation searching the entire repository.
pub fn repair_target(result:&Value)->Option<(String,u64)> {
    if result["reason"]!="build_or_typecheck_failed" {return None;}
    for check in result["rsi"]["checks"].as_array()? {
        if check["passed"]==true {continue;}
        // Native validation stores focused cargo output in "detail".
        // Looking only for stderr/error discards the actual file:line
        // diagnostic and turns repair into another blind model attempt.
        let diagnostic=check["detail"].as_str()
            .or_else(||check["stderr"].as_str())
            .or_else(||check["error"].as_str()).unwrap_or("");
        for text in diagnostic.lines() {
            // cargo often emits ANSI-colored diagnostics even when its
            // stderr is captured: "\\x1b[94m--> \\x1b[0msrc/file.rs:75:13".
            // Remove CSI escapes before parsing the real compiler location.
            let mut plain=String::new();
            let mut chars=text.chars().peekable();
            while let Some(ch)=chars.next() {
                if ch=='\u{1b}' && chars.peek()==Some(&'[') {
                    chars.next();
                    for code in chars.by_ref() {
                        if ('@'..='~').contains(&code) {break;}
                    }
                } else {plain.push(ch);}
            }
            let Some(location)=plain.trim().strip_prefix("-->") else {continue;};
            let mut fields=location.trim().rsplitn(3, ':');
            let _column=fields.next();
            let Some(line)=fields.next().and_then(|s|s.parse::<u64>().ok()) else {continue;};
            let Some(path)=fields.next() else {continue;};
            if line==0 || line>100_000 || !path.ends_with(".rs")
                || !crate::engine::rsi::safe_path(path) {continue;}
            return Some((path.to_string(),line));
        }
    }
    None
}

/// Return a short, grounded repair instruction. Do not repeat blind model
/// calls for provider errors, missing diffs or checkout integrity failures.
pub fn repair_feedback(result:&Value)->Option<String>{
    if result["status"]!="needs_review"
        || result["rsi"]["sourceClean"]!=true {
        return None;
    }
    // A model/provider error that occurred AFTER a verified source edit
    // must not suppress the independently observed compiler diagnostics.
    // Only retry such errors when the verifier actually found a broken patch.
    if !result["rsi"]["modelError"].as_str().unwrap_or("").is_empty()
        && result["reason"]!="build_or_typecheck_failed" {
        return None;
    }
    // A model tried to edit, yet left zero source bytes changed. Give it a
    // bounded, evidence-driven correction rather than abandoning the whole
    // generation as research-only. Never retry an ordinary no-edit answer.
    if result["reason"]=="no_valid_candidate"
        && result["rsi"]["captureError"]=="모델이 실제 소스 파일을 변경하지 않았습니다."
        && result["rsi"]["editEvidence"]["attempts"].as_u64().unwrap_or(0)>0 {
        let problems=result["rsi"]["editEvidence"]["recentFailures"].as_array()
            .into_iter().flatten().filter_map(Value::as_str)
            .take(3).collect::<Vec<_>>().join("; ");
        return Some(format!(
            "이전 편집은 실제 Git 변경을 남기지 못했습니다. 편집 도구 호출은 완료된 코드 수정이 아닙니다. 최신 workspace_read_range의 sourceSha256과 줄 범위를 확인하고 원본과 다른 새 구현을 workspace_replace_range로 작성하세요. 앞서 나온 오류: {}. 성공했다고 설명만 하지 말고 파일 변경을 실행하세요.",
            if problems.is_empty() {"실제 변경 없음"}else{&problems}
        ));
    }
    if !matches!(result["reason"].as_str(),
            Some("build_or_typecheck_failed"|"unsafe_safety_failed"))
        ||!result["rsi"]["captureError"].as_str().unwrap_or("").is_empty(){
        return None;
    }
    let checks=result["rsi"]["checks"].as_array()?;
    let failures=checks.iter().filter(|check|check["passed"]!=true)
        .map(|check|{
            let label=check["label"].as_str().unwrap_or("verification");
            let detail=check["detail"].as_str()
                .or_else(||check["stderr"].as_str())
                .or_else(||check["error"].as_str())
                .unwrap_or("The verifier reported failure without further detail.");
            format!("{label}: {}",detail.chars().take(1100).collect::<String>())
        }).collect::<Vec<_>>();
    if failures.is_empty(){return None;}
    let target=repair_target(result).map(|(path,line)|format!(
        "우선 고칠 오류 위치: {path}:{line}. workspace_read_range를 path={path}, startLine={}, endLine={}로 호출해 현재 원문과 sourceSha256을 확보한 뒤, 그 파일의 실제 오류를 workspace_replace_range로 수정하세요. 다른 파일 탐색으로 돌아가지 마세요.\n\n",
        line.saturating_sub(12).max(1),line.saturating_add(12)
    )).unwrap_or_default();
    Some(format!(
        "이전 소스 변경 후 별도의 컴파일/타입 검사에서 오류가 발견됐습니다. 기존 변경을 읽고 실제 오류를 고쳐 다시 저장하세요. 검사 자체를 삭제하거나 우회하지 마세요. 수정 전과 무관한 파일은 건드리지 마세요. 테스트는 엔진이 다시 실행합니다.\n\n{}{}",
        target,failures.join("\n\n")
    ))
}
pub async fn cleanup(worktree:&Worktree)->Result<(),String>{
    let name=worktree.path.to_str().ok_or("작업공간 경로 오류")?;
    git(&worktree.source,&["worktree","remove","--force",name]).await.map(|_|())
}

#[cfg(test)]
#[path = "rsi_worktree/tests.rs"]
mod tests;

