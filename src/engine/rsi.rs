//! Manually supervised recursive self-improvement for code-level changes.
//!
//! Existing policy/algorithm optimizers handle live parameters. This ledger
//! handles *source code* revisions to algorithms, harness and the improver
//! itself. A proposal can never write source, execute a patch or deploy.
//! Only separately run, frozen, paired experiments can authorize an operator
//! to merge a reviewed patch. The next generation records its generator.
use crate::{engine::algorithms, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MIN_PAIRED_CASES: usize = 8;
const MAX_PATCH_BYTES: usize = 256 * 1024;

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn safe_path(path: &str) -> bool {
    !path.contains('\\')
        && !path.starts_with('/')
        && !path.split('/').any(|part|part.is_empty() || part.starts_with('.'))
        && (path.starts_with("src/")
            || path.starts_with("tests/")
            || path.starts_with("web/src/")
            || path.starts_with("src-tauri/src/"))
}

/// Parse and constrain every changed path, not only the declared target.
/// Never let a proposed code patch quietly replace CI, secrets or dependencies.
fn patch_paths(patch: &str) -> Result<Vec<String>, String> {
    if patch.trim().is_empty() || patch.len() > MAX_PATCH_BYTES {
        return Err("patch는 비어 있지 않은 256KiB 이하의 unified git diff여야 합니다.".into());
    }
    if patch.contains("GIT binary patch")
        || patch.contains("Binary files ")
        || patch.lines().any(|line| {
            line.starts_with("rename from ")
                || line.starts_with("rename to ")
                || line.starts_with("copy from ")
                || line.starts_with("copy to ")
                || line.starts_with("new file mode 120000")
                || line.starts_with("new mode 120000")
                || line.starts_with("old mode ")
                || line.starts_with("new mode ")
        })
    {
        return Err("바이너리·링크·파일 이동·권한 변경 패치는 허용하지 않습니다.".into());
    }
    let mut paths = BTreeSet::new();
    for line in patch.lines().filter(|line| line.starts_with("diff --git ")) {
        let Some(value) = line.strip_prefix("diff --git a/") else {
            return Err("git diff 경로 형식이 올바르지 않습니다.".into());
        };
        let Some((left, right)) = value.split_once(" b/") else {
            return Err("git diff 경로 형식이 올바르지 않습니다.".into());
        };
        if left != right || !safe_path(left) {
            return Err(format!("패치 대상 경로가 허용되지 않습니다: {left}"));
        }
        paths.insert(left.to_string());
    }
    if paths.is_empty() || paths.len() > 24 {
        return Err("패치에는 1~24개의 허용된 코드 경로가 필요합니다.".into());
    }
    Ok(paths.into_iter().collect())
}

fn case_plan(value: &Value) -> Result<Vec<Value>, String> {
    let rows = value.as_array().ok_or("평가 계획 cases 배열이 필요합니다.")?;
    if rows.len() < 2 || rows.len() > 128 {
        return Err("평가 계획은 2~128개의 독립 사례를 포함해야 합니다.".into());
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for row in rows {
        let id = row["id"].as_str().filter(|v| !v.is_empty() && v.len() <= 120)
            .ok_or("각 사례에는 id가 필요합니다.")?;
        let domain = row["domain"].as_str().filter(|v| !v.is_empty() && v.len() <= 80)
            .ok_or("각 사례에는 domain이 필요합니다.")?;
        let filter = row["filter"].as_str()
            .filter(|v| !v.is_empty() && v.len() <= 240
                && v.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'))
            .ok_or("각 사례는 동결된 Rust 테스트 함수 filter를 지정해야 합니다.")?;
        if !seen.insert(id.to_string()) {
            return Err(format!("중복 평가 사례: {id}"));
        }
        let metrics = crate::engine::rsi_metrics::plan(&row["metrics"])?;
        result.push(json!({"id":id,"domain":domain,"holdout":row["holdout"] == true,"filter":filter,"metrics":metrics}));
    }
    Ok(result)
}

pub fn propose(state: &AppState, body: &Value) -> Result<Value, String> {
    let target = body["target"].as_str().ok_or("target이 필요합니다.")?;
    if !matches!(target, "algorithm" | "harness" | "improver") {
        return Err("target은 algorithm, harness, improver 중 하나입니다.".into());
    }
    let base = body["baseCommit"].as_str().filter(|v| sha(v))
        .ok_or("baseCommit은 40자리 git SHA여야 합니다.")?;
    let patch = body["patch"].as_str().ok_or("patch가 필요합니다.")?;
    let paths = patch_paths(patch)?;
    if target == "improver" && !paths.iter().any(|path| {
        path == "src/engine/rsi.rs" || path == "src/engine/rsi_worktree.rs"
            || path == "src/engine/rsi_evolve.rs" || path == "src/engine/rsi_metrics.rs"
            || path == "src/api/rsi.rs" || path == "src/engine/algorithms.rs"
            || path == "src/engine/algorithm_eval.rs"
            || path == "src/engine/policy_patch.rs"
    }) {
        return Err("improver 후보는 실제 개선기 소스를 수정해야 합니다.".into());
    }
    let hypothesis = body["hypothesis"].as_str()
        .filter(|v| v.trim().chars().count() >= 15 && v.chars().count() <= 3000)
        .ok_or("재현 가능한 개선 가설(15~3000자)이 필요합니다.")?;
    let cases = case_plan(&body["cases"])?;
    let oracle_version = body["oracleVersion"].as_str()
        .filter(|v| !v.is_empty() && v.len() <= 120)
        .ok_or("후보 등록 전에 고정된 oracleVersion이 필요합니다.")?;
    let patch_hash = digest(patch);
    let plan_hash=digest(&json!({"cases":cases,"oracleVersion":oracle_version}).to_string());
    let id = format!("rsi-{}", digest(&format!("{base}:{target}:{patch_hash}:{plan_hash}")));
    if let Some(existing) = state.store.get("rsi_candidate", &id) {
        return Ok(existing);
    }
    let parent = body["parentCandidateId"].as_str().filter(|s| !s.is_empty());
    if let Some(parent) = parent {
        let previous = state.store.get("rsi_candidate", parent)
            .ok_or("parentCandidateId가 존재하지 않습니다.")?;
        if previous["target"] != "improver"
            || previous["status"] != "reported_deployed"
        {
            return Err("실제 배포를 기록한 개선기만 다음 세대 개선기의 부모가 될 수 있습니다.".into());
        }
        if body["generatorSourceCommit"] != previous["deployment"]["commitSha"] {
            return Err("다음 개선기를 실행한 소스 커밋을 부모의 배포 revision과 연결하세요.".into());
        }
    }
    let generator_revision = algorithms::active(state, "selfimprove.search")
        .and_then(|row| row["revision"].as_str().map(str::to_string));
    let now = chrono::Utc::now().to_rfc3339();
    let record = json!({
        "id":id, "target":target, "baseCommit":base, "patchSha256":patch_hash,
        "patch":patch, "paths":paths, "hypothesis":hypothesis,
        "cases":cases, "oracleVersion":oracle_version,
        "status":"proposed", "createdAt":now,
        "generatorAlgorithmRevision":generator_revision,
        "generatorModel":body["generatorModel"], "generatorEffort":body["generatorEffort"],
        "generatorSourceCommit":body["generatorSourceCommit"],
        "parentCandidateId":parent, "lineageGeneration": parent
            .and_then(|id| state.store.get("rsi_candidate", id))
            .and_then(|v| v["lineageGeneration"].as_u64())
            .unwrap_or(0) + 1,
        "evaluation":null, "approval":null,
        "note":"승인 전에는 소스 변경·코드 실행·자동 배포 없음"
    });
    let saved = state.store.put("rsi_candidate", &record)
        .ok_or("RSI 후보 저장 실패")?;
    state.store.audit("rsi.proposed", json!({"id":id,"target":target,"baseCommit":base}));
    state.bus.record("rsi_candidate", &saved);
    Ok(saved)
}

fn evaluate_cases(plan: &[Value], measured: &[Value], target: &str) -> Result<Value, String> {
    if plan.len() != measured.len() {
        return Err("실험 사례 수가 고정 계획과 일치하지 않습니다.".into());
    }
    let mut used = BTreeSet::new();
    let mut gains = 0usize;
    let mut losses = 0usize;
    let mut baseline_successes = 0usize;
    let mut candidate_successes = 0usize;
    let mut holdouts = 0usize;
    let mut improved_holdouts = 0usize;
    let mut holdout_domains = BTreeSet::new();
    let mut holdout_evidence = Vec::new();
    let mut improved_holdout_domains = BTreeSet::new();
    let mut holdout_gains = 0usize;
    let mut meta_holdout_gains = 0usize;
    let mut metric_gains = 0;
    let mut metric_losses = 0;
    let mut measurements = Vec::new();
    let mut meta_gains = 0;
    for measurement in measured {
        let id = measurement["id"].as_str().ok_or("측정 사례 id가 없습니다.")?;
        let Some(planned) = plan.iter().find(|row| row["id"] == id) else {
            return Err(format!("계획에 없는 측정 사례: {id}"));
        };
        if !used.insert(id.to_string()) {
            return Err(format!("중복 측정 사례: {id}"));
        }
        let before = measurement["baselinePass"].as_bool()
            .ok_or("baselinePass는 실제 실행 결과 boolean이어야 합니다.")?;
        let after = measurement["candidatePass"].as_bool()
            .ok_or("candidatePass는 실제 실행 결과 boolean이어야 합니다.")?;
        if before { baseline_successes += 1; }
        if after { candidate_successes += 1; }
        if !before && after { gains += 1; }
        if before && !after { losses += 1; }
        let specs = planned["metrics"].as_array().cloned().unwrap_or_default();
        let (mg, ml, detail) = if before && after {
            crate::engine::rsi_metrics::compare(&specs,&measurement["metrics"])?
        } else { (0,0,Vec::new()) };
        // Failed cases cannot earn metric gains by returning early or doing less work.
        let mg = if before && after { mg } else { 0 };
        metric_gains += mg;
        metric_losses += ml;
        if planned["domain"] == "improver" { meta_gains += mg; }
        measurements.push(json!({"id":id,"domain":planned["domain"],
            "holdout":planned["holdout"],"baselinePass":before,"candidatePass":after,
            "metrics":detail}));
        if planned["holdout"] == true {
            let domain = planned["domain"].as_str()
                .filter(|domain| !domain.trim().is_empty())
                .ok_or("동결 holdout 사례의 도메인이 없습니다.")?;
            holdouts += 1;
            holdout_domains.insert(domain.to_string());
            let improved = (!before && after) || mg > 0;
            if improved {
                improved_holdouts += 1;
                improved_holdout_domains.insert(domain.to_string());
                if !before && after { holdout_gains += 1; }
                if planned["domain"]=="improver" { meta_holdout_gains += mg; }
            }
            // Derived from the frozen plan and individual local measurements.
            // The RSI coordinator must not infer transfers from domain counts.
            holdout_evidence.push(json!({
                "id":id,"domain":domain,
                "baselinePass":before,"candidatePass":after,
                "metricGains":mg,"metricRegressions":ml,
                "improved":improved
            }));
        }
    }
    let ready = plan.len() >= MIN_PAIRED_CASES
        && holdouts >= 2
        && gains + metric_gains > 0
        // A training-only win is never sufficient to adopt changed code.
        && improved_holdouts > 0
        && losses + metric_losses == 0
        && (target != "improver"
            || (holdout_domains.len() >= 2 && improved_holdouts > 0
                && (!plan.iter().any(|p|p["domain"]=="improver" && p["metrics"].as_array().is_some_and(|m|!m.is_empty())) || meta_holdout_gains>0)));
    Ok(json!({
        "pairedCases":plan.len(),"holdoutCases":holdouts,
        "holdoutDomains":holdout_domains.into_iter().collect::<Vec<_>>(),
        "improvedHoldoutDomains":improved_holdout_domains.into_iter().collect::<Vec<_>>(),
        "holdoutEvidence":holdout_evidence,"holdoutGains":holdout_gains,
        "baselineSuccesses":baseline_successes,"candidateSuccesses":candidate_successes,
        "gains":gains,"regressions":losses+metric_losses,"improvedHoldouts":improved_holdouts,
        "metricGains":metric_gains,"metricRegressions":metric_losses,
        "metaGains":meta_gains,"metaHoldoutGains":meta_holdout_gains,"measurements":measurements,
        "eligible":ready,
        "verdict":if losses+metric_losses>0 {"regressed"} else if ready {"improved"} else {"inconclusive"}
    }))
}

pub fn evaluate(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    let mut record = state.store.get("rsi_candidate", id).ok_or("후보를 찾을 수 없습니다.")?;
    if record["status"] != "proposed" && record["status"] != "inconclusive" {
        return Err("proposed 또는 inconclusive 상태만 재평가할 수 있습니다.".into());
    }
    if body["baseCommit"] != record["baseCommit"]
        || body["patchSha256"] != record["patchSha256"]
    {
        return Err("평가 대상 소스 revision 또는 패치 해시가 바뀌었습니다.".into());
    }
    let runner = body["runner"].as_str().filter(|s| !s.trim().is_empty() && s.len() <= 100)
        .ok_or("독립 실행 환경 식별자 runner가 필요합니다.")?;
    let oracle = body["oracleVersion"].as_str().filter(|s| !s.is_empty() && s.len() <= 120)
        .ok_or("고정된 oracleVersion이 필요합니다.")?;
    if record["oracleVersion"] != oracle {
        return Err("후보 등록 후 평가 oracleVersion을 변경할 수 없습니다.".into());
    }
    let proof = body["reportSha256"].as_str().filter(|s| s.len()==64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("실행 보고서의 SHA-256 해시가 필요합니다.")?;
    let plan = record["cases"].as_array().ok_or("계획이 손상되었습니다.")?;
    let measured = body["cases"].as_array().ok_or("측정 사례 배열이 필요합니다.")?;
    let mut outcome = evaluate_cases(plan, measured, record["target"].as_str().unwrap_or(""))?;
    if body["baselineBuildOk"] != true || body["candidateBuildOk"] != true {
        outcome["eligible"] = json!(false);
        outcome["verdict"] = json!("build_failed");
    }
    let status = if outcome["verdict"] == "improved" { "verified" }
        else if outcome["verdict"] == "regressed" || outcome["verdict"] == "build_failed" {
            "rejected"
        } else { "inconclusive" };
    let evaluation = json!({
        "id":format!("rsi-eval-{}",uuid::Uuid::new_v4()),"status":status,
        "verdict":outcome["verdict"],"metrics":outcome,
        "baselineCommit":record["baseCommit"],"patchSha256":record["patchSha256"],
        "runner":runner,"oracleVersion":oracle,"reportSha256":proof,
        "source":"operator-submitted paired results; report provenance is not independently authenticated",
        "evaluatedAt":chrono::Utc::now().to_rfc3339()
    });
    record["status"] = json!(status);
    record["evaluation"] = evaluation.clone();
    let saved = state.store.put("rsi_candidate", &record).ok_or("평가 저장 실패")?;
    state.store.put("rsi_evaluation", &evaluation);
    state.store.audit("rsi.evaluated",json!({"id":id,"status":status,"report":proof}));
    state.bus.record("rsi_candidate",&saved);
    Ok(saved)
}

/// An operator accepts a proven candidate for manual Git merge. No source
/// edits, process restarts, policy activation or branch ref changes occur here.
pub fn decide(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    let mut record = state.store.get("rsi_candidate", id).ok_or("후보를 찾을 수 없습니다.")?;
    let action = body["action"].as_str().ok_or("action이 필요합니다.")?;
    if action != "approve" && action != "reject" {
        return Err("action은 approve 또는 reject여야 합니다.".into());
    }
    if action == "approve" {
        if record["status"] != "verified" {
            return Err("고정된 독립 사례에서 검증된 후보만 승인할 수 있습니다.".into());
        }
        if body["confirmation"] != "APPROVE"
            || body["baseCommit"] != record["baseCommit"]
            || body["patchSha256"] != record["patchSha256"]
        {
            return Err("승인 전 baseCommit, patchSha256과 명시적 APPROVE를 확인하세요.".into());
        }
        record["status"] = json!("approved_for_merge");
    } else {
        if record["status"] == "approved_for_merge" {
            return Err("이미 승인된 후보는 새 검토 대상으로 되돌려야 합니다.".into());
        }
        record["status"] = json!("rejected");
    }
    record["approval"] = json!({
        "action":action,"operator":body["operator"],"reviewNote":body["reviewNote"],
        "at":chrono::Utc::now().to_rfc3339()
    });
    let saved = state.store.put("rsi_candidate",&record).ok_or("승인 저장 실패")?;
    state.store.audit("rsi.decision",json!({"id":id,"action":action}));
    state.bus.record("rsi_candidate",&saved);
    Ok(saved)
}

/// Records an operator-attested deployment of an approved improver.
/// This is deliberately NOT a Git mutation or an independently verified
/// deployment. The commit and CI evidence are shown for separate review.
pub fn confirm_deployment(
    state: &AppState,
    id: &str,
    body: &Value,
) -> Result<Value, String> {
    let mut record = state.store.get("rsi_candidate", id)
        .ok_or("후보를 찾을 수 없습니다.")?;
    if record["status"] != "approved_for_merge" {
        return Err("수동 병합 승인된 후보만 배포 기록을 만들 수 있습니다.".into());
    }
    let commit = body["commitSha"].as_str().filter(|sha_value| sha(sha_value))
        .ok_or("배포 commitSha는 40자리 Git SHA여야 합니다.")?;
    if commit == record["baseCommit"].as_str().unwrap_or("") {
        return Err("배포 커밋이 원본과 같습니다.".into());
    }
    let operator = body["operator"].as_str().filter(|s| !s.trim().is_empty())
        .ok_or("배포 확인자 operator가 필요합니다.")?;
    if body["confirmation"] != "DEPLOYED" {
        return Err("실제 배포 후 DEPLOYED를 명시해야 합니다.".into());
    }
    record["deployment"] = json!({
        "commitSha":commit,
        "operator":operator,
        "ciEvidence":body["ciEvidence"],
        "at":chrono::Utc::now().to_rfc3339(),
        "verification":"operator_reported_not_automatically_verified"
    });
    record["status"] = json!("reported_deployed");
    let saved = state.store.put("rsi_candidate", &record).ok_or("배포 기록 저장 실패")?;
    state.store.audit("rsi.deployment_reported",json!({"id":id,"commitSha":commit}));
    state.bus.record("rsi_candidate",&saved);
    Ok(saved)
}

pub fn list(state: &AppState) -> Vec<Value> {
    let mut items = state.store.all("rsi_candidate");
    items.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    items
}

pub fn one(state: &AppState, id: &str) -> Option<Value> {
    state.store.get("rsi_candidate", id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    fn state() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-rsi-{}", uuid::Uuid::new_v4()));
        let config = Config { port:0, data_dir:root.clone(), web_dir:root.join("web"), workspace:root.clone() };
        let state = AppState::new(config.clone(), Store::open(&config.db_path()).unwrap(), Vault::open(&config.key_path()));
        (state,root)
    }
    fn base() -> String { "a".repeat(40) }
    fn patch() -> String {
        "diff --git a/src/engine/rsi.rs b/src/engine/rsi.rs\n--- a/src/engine/rsi.rs\n+++ b/src/engine/rsi.rs\n@@ -1 +1 @@\n-old\n+new\n".into()
    }
    fn cases(domain: &str) -> Vec<Value> {
        (0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i==1 {"other"} else {domain},"holdout":i<3,
            "filter":format!("engine::rsi::tests::case_{i}")})).collect()
    }
    fn proposal(target: &str) -> Value {
        json!({"target":target,"baseCommit":base(),"patch":patch(),
            "hypothesis":"반복되는 개선 후보를 보다 정확하게 평가하도록 변경한다.",
            "cases":cases("memory"),"oracleVersion":"frozen-v1","generatorModel":"manual"})
    }
    fn report(record: &Value) -> Value {
        json!({"baseCommit":record["baseCommit"],"patchSha256":record["patchSha256"],
            "oracleVersion":"frozen-v1","runner":"local-worktree","reportSha256":"b".repeat(64),
            "baselineBuildOk":true,"candidateBuildOk":true,
            "cases":(0..8).map(|i|json!({"id":format!("case-{i}"),"baselinePass":i!=0,"candidatePass":true})).collect::<Vec<_>>()})
    }
    #[test]
    fn paired_results_cannot_be_approved_without_evidence_and_frozen_revision() {
        let (state,root)=state();
        let record=propose(&state,&proposal("improver")).unwrap();
        let id=record["id"].as_str().unwrap();
        assert_eq!(propose(&state,&proposal("improver")).unwrap()["id"],id);
        assert!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE"})).is_err());
        let mut result=report(&record);
        result["patchSha256"]=json!("changed");
        assert!(evaluate(&state,id,&result).is_err());
        let evaluated=evaluate(&state,id,&report(&record)).unwrap();
        assert_eq!(evaluated["status"],"verified");
        let evidence = evaluated["evaluation"]["metrics"]["holdoutEvidence"].as_array().unwrap();
        assert_eq!(evidence.len(),3);
        assert_eq!(evidence[0]["domain"],"memory");
        assert_eq!(evidence[0]["baselinePass"],false);
        assert_eq!(evidence[0]["candidatePass"],true);
        assert_eq!(evidence[0]["improved"],true);
        assert_eq!(evidence[1]["domain"],"other");
        assert_eq!(evidence[1]["improved"],false);
        assert!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE","baseCommit":base(),"patchSha256":"wrong"})).is_err());
        assert_eq!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE",
            "baseCommit":base(),"patchSha256":record["patchSha256"],"operator":"owner"})).unwrap()["status"],"approved_for_merge");
        assert_eq!(state.store.all("rsi_evaluation").len(),1);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn evaluation_cannot_switch_its_frozen_oracle_or_test_names() {
        let (state,root)=state();
        let mut bad=proposal("algorithm");
        bad["cases"][0]["filter"]=json!("not-a-full-test-name%");
        assert!(propose(&state,&bad).is_err());
        let record=propose(&state,&proposal("algorithm")).unwrap();
        let mut result=report(&record);
        result["oracleVersion"]=json!("a-different-test");
        assert!(evaluate(&state,record["id"].as_str().unwrap(),&result).is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn training_only_improvement_never_promotes_a_candidate() {
        let (state,root)=state();
        let candidate=propose(&state,&proposal("harness")).unwrap();
        let id=candidate["id"].as_str().unwrap();
        let mut measured=report(&candidate);
        // The only change is outside the three frozen holdout cases.
        for (i,row) in measured["cases"].as_array_mut().unwrap().iter_mut().enumerate() {
            row["baselinePass"]=json!(i!=3);
            row["candidatePass"]=json!(true);
        }
        let evaluated=evaluate(&state,id,&measured).unwrap();
        let metrics=&evaluated["evaluation"]["metrics"];
        assert_eq!(metrics["gains"],1);
        assert_eq!(metrics["improvedHoldouts"],0);
        assert_eq!(metrics["eligible"],false);
        assert_eq!(evaluated["status"],"inconclusive");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn improver_metric_gain_only_on_training_does_not_promote() {
        let (state,root)=state();
        let mut request=proposal("improver");
        request["cases"][3]["domain"]=json!("improver");
        request["cases"][3]["metrics"]=json!([{
            "key":"tokens","direction":"lower",
            "minRelativeGain":0.05,"maxRelativeRegression":0.0
        }]);
        let candidate=propose(&state,&request).unwrap();
        let mut measured=report(&candidate);
        for row in measured["cases"].as_array_mut().unwrap() {
            row["baselinePass"]=json!(true);
            row["candidatePass"]=json!(true);
        }
        measured["cases"][3]["metrics"]=json!({"tokens":{"baseline":100.0,"candidate":50.0}});
        let tested=evaluate(&state,candidate["id"].as_str().unwrap(),&measured).unwrap();
        assert_eq!(tested["status"],"inconclusive");
        assert_eq!(tested["evaluation"]["metrics"]["metaGains"],1);
        assert_eq!(tested["evaluation"]["metrics"]["metaHoldoutGains"],0);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn holdout_regressions_and_unmatched_cases_never_promote() {
        let (state,root)=state();
        let mut p=proposal("harness");
        p["patch"]=json!(patch().replace("new","alternate"));
        let record=propose(&state,&p).unwrap();
        let mut result=report(&record);
        result["cases"][1]["candidatePass"]=json!(false);
        let id=record["id"].as_str().unwrap();
        assert_eq!(evaluate(&state,id,&result).unwrap()["status"],"rejected");
        assert!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE"})).is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn proposal_rejects_unsafe_paths_and_incomplete_improver_evidence() {
        let (state,root)=state();
        let mut p=proposal("algorithm");
        p["patch"]=json!("diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml\n@@ -1 +1 @@\n-old\n+new\n");
        assert!(propose(&state,&p).is_err());
        let mut p=proposal("algorithm");
        p["patch"]=json!(patch().replace("src/engine/rsi.rs","src/../secrets.env"));
        assert!(propose(&state,&p).is_err());
        let mut p=proposal("improver");
        p["cases"]=json!((0..8).map(|i|json!({"id":format!("case-{i}"),"domain":"only-one","holdout":i<3,"filter":format!("engine::rsi::tests::case_{i}")})).collect::<Vec<_>>());
        let candidate=propose(&state,&p).unwrap();
        assert_eq!(evaluate(&state,candidate["id"].as_str().unwrap(),&report(&candidate)).unwrap()["status"],"inconclusive");
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn only_a_deployed_improver_can_seed_the_next_generation() {
        let (state,root)=state();
        let parent=propose(&state,&proposal("improver")).unwrap();
        let id=parent["id"].as_str().unwrap();
        evaluate(&state,id,&report(&parent)).unwrap();
        decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE",
            "baseCommit":base(),"patchSha256":parent["patchSha256"]})).unwrap();
        let mut child=proposal("improver");
        child["patch"]=json!(patch().replace("+new","+newer"));
        child["parentCandidateId"]=parent["id"].clone();
        assert!(propose(&state,&child).is_err(), "approval is not deployment");
        let deployed = "b".repeat(40);
        assert_eq!(confirm_deployment(&state,id,&json!({
            "commitSha":deployed,"operator":"owner","confirmation":"DEPLOYED"
        })).unwrap()["status"],"reported_deployed");
        child["generatorSourceCommit"]=json!(deployed);
        assert_eq!(propose(&state,&child).unwrap()["lineageGeneration"],2);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn all_passed_guards_can_promote_a_real_metric_gain_and_freeze_its_plan(){
        let(state,root)=state();let mut p=proposal("harness");
        p["cases"][0]["metrics"]=json!([{"key":"quality","direction":"higher","minRelativeGain":0.1}]);
        let candidate=propose(&state,&p).unwrap();
        let mut measured=report(&candidate);
        for row in measured["cases"].as_array_mut().unwrap(){row["baselinePass"]=json!(true);}
        measured["cases"][0]["metrics"]=json!({"quality":{"baseline":0.7,"candidate":0.9}});
        let result=evaluate(&state,candidate["id"].as_str().unwrap(),&measured).unwrap();
        assert_eq!(result["status"],"verified");assert_eq!(result["evaluation"]["metrics"]["gains"],0);
        assert_eq!(result["evaluation"]["metrics"]["metricGains"],1);
        p["cases"][0]["metrics"][0]["minRelativeGain"]=json!(0.01);
        assert_ne!(propose(&state,&p).unwrap()["id"],candidate["id"],"a different oracle cannot reuse a verified candidate identity");
        let _=std::fs::remove_dir_all(root);
    }
}
