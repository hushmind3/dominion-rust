//! Detached source candidate lifecycle, static checks and compiler feedback.
//! Rust submodule of rsi_worktree; executable via root re-exports.
use super::*;

pub struct Worktree {
    pub source: PathBuf,
    pub path: PathBuf,
    pub base: String,
}
pub(super) fn digest(value:&str)->String {
    format!("{:x}",Sha256::digest(value.as_bytes()))
}
pub(super) async fn run(mut command: Command, limit: Duration) -> Result<std::process::Output,String> {
    tools::run_command(&mut command, limit).await
}
pub(super) async fn git(dir:&Path, args:&[&str])->Result<String,String>{
    let mut command=Command::new("git");
    command.current_dir(dir).args(args);
    let output=run(command,GIT_LIMIT).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).chars().take(900).collect());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
pub(super) fn valid_sha(value:&str)->bool {
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

/// Classify actual changed source paths, not arbitrary diff-header substrings.
/// A compiled meta-controller rewrite must be identifiable as improvement
/// machinery in the next independently measured generation.
pub(super) fn classify(patch:&str)->&'static str {
    let mut changed=patch.lines().filter_map(|line|{
        let header=line.strip_prefix("diff --git ")?;
        let mut fields=header.split_whitespace();
        let before=fields.next()?.strip_prefix("a/")?;
        let after=fields.next()?.strip_prefix("b/")?;
        (before==after && fields.next().is_none()).then_some(after)
    }).peekable();
    if changed.peek().is_none(){return "harness";}
    let mut only_algorithms=true;
    for path in changed{
        if path=="src/api/rsi.rs"
            || path.starts_with("src/engine/rsi_")
            || path.starts_with("src/engine/rsi/")
            || path=="src/engine/rsi.rs"
        {
            return "improver";
        }
        only_algorithms &= matches!(path,
            "src/engine/memory.rs"|"src/engine/algorithms.rs"
            |"src/engine/room_state.rs")
            || path.starts_with("src/engine/memory/");
    }
    if only_algorithms {"algorithm"} else {"harness"}
}
/// Retain the FIRST actionable compiler diagnostic as well as its final
/// summary. Cargo often prints several thousand bytes after the first
/// " --> path:line:column" location; tail-only truncation hid the source
/// location from the model's actual compiler-repair turn.
pub(super) fn compiler_diagnostic_excerpt(value:&str)->String {
    const HEAD:usize=2600;
    const TAIL:usize=1800;
    let total=value.chars().count();
    if total<=HEAD+TAIL {return value.to_string();}
    let beginning=value.chars().take(HEAD).collect::<String>();
    let end=value.chars().skip(total-TAIL).collect::<String>();
    format!("{beginning}\n[{} diagnostic characters omitted]\n{end}",total-HEAD-TAIL)
}
pub(super) fn check_result(label:&str, result:Result<std::process::Output,String>)->Value {
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
pub(super) async fn validate(worktree:&Worktree,patch:&str)->Vec<Value> {
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
    // Native provenance must bind the exact worker, source, campaign and
    // final bytes. A model write is never inferred from native induction.
    let native_receipt=context.state.store.get("rsi_mutation",&context.job_id)
        .filter(|receipt|input["rsiNativeInduction"]==true
            && receipt["origin"]=="native_inductive_source_mutation"
            && receipt["campaignId"]==input["rsiCampaignId"]
            && receipt["sourceDir"].as_str()==Some(worktree.source.to_string_lossy().as_ref())
            && receipt["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
            && receipt["path"].as_str().is_some_and(|p|
                patch.contains(&format!("diff --git a/{p} b/{p}"))));
    let native_origin=native_receipt.as_ref().is_some_and(|receipt|{
        let Some(path)=receipt["path"].as_str() else {return false;};
        std::fs::read(worktree.path.join(path)).ok().is_some_and(|bytes|
            format!("{:x}",Sha256::digest(&bytes))==receipt["afterSha256"])
    });
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
            "generatorModel":if native_origin {json!(format!("dominion-native:{}",
                native_receipt.as_ref().and_then(|row|row["operator"].as_str()).unwrap_or("unknown")))}else{input["model"].clone()},
            "generatorEffort":if native_origin {Value::Null}else{input["reasoningEffort"].clone()},
            "generatorSourceCommit":worktree.base
        });
        match rsi::propose(&context.state,&proposal) {
            Ok(mut candidate)=>{
                candidate["rsiJobId"]=json!(context.job_id);
                candidate["origin"]=json!(if native_origin {
                    "native_inductive_source_mutation"
                }else{"model_driven_detached_worktree"});
                if native_origin {
                    candidate["nativeMutation"]=json!(native_receipt);
                }
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
        "candidateOrigin":if native_origin {"native_inductive_source_mutation"}else{"model_driven_detached_worktree"},
        "nativeMutation":native_receipt,
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
pub(super) fn run_status(
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

