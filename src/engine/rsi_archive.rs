//! Measured stepping stones for recursive source search. Inspired by DGM's
//! archive/offspring selection and GEPA's per-case Pareto frontier; implemented
//! here against DOMINION's immutable paired receipts, without upstream code.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// A search parent is not an adopted generation. Its original base remains
/// the child's A/B baseline, including when the parent is a neutral patch.
pub(crate) fn parent_metrics(state: &AppState, candidate: &Value, base: &str) -> Option<Value> {
    let patch = candidate["patch"].as_str()?;
    let hash = format!("{:x}", Sha256::digest(patch.as_bytes()));
    let paths = candidate["paths"].as_array()?;
    if candidate["baseCommit"] != base
        || candidate["pairedComparisonComplete"] != true
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || !matches!(
            candidate["origin"].as_str(),
            Some("model_driven_detached_worktree" | "native_inductive_source_mutation")
        )
        || patch.is_empty()
        || candidate["patchSha256"] != hash
        || paths.is_empty()
        || paths
            .iter()
            .any(|path| !path.as_str().is_some_and(rsi::safe_path))
    {
        return None;
    }
    let metrics = rsi::replayed_pair_metrics(state, candidate)?;
    (metrics["verdict"] == "inconclusive"
        && metrics["regressions"] == 0
        && metrics["metricRegressions"] == 0
        && metrics["baselineSuccesses"] == metrics["pairedCases"]
        && metrics["candidateSuccesses"] == metrics["pairedCases"])
        .then_some(metrics)
}

fn fitness(metrics: &Value) -> Option<BTreeMap<String, f64>> {
    let mut result = BTreeMap::new();
    for case in metrics["measurements"].as_array()? {
        for metric in case["metrics"].as_array()? {
            let before = metric["baseline"].as_f64()?;
            let after = metric["candidate"].as_f64()?;
            let score = match metric["direction"].as_str()? {
                "higher" => after,
                "lower" if before > 0.0 => -after / before,
                "lower" if after == 0.0 => -1.0,
                _ => return None,
            };
            if !score.is_finite() {
                return None;
            }
            let key = format!("{}/{}", case["id"].as_str()?, metric["key"].as_str()?);
            if result.insert(key, score).is_some() {
                return None;
            }
        }
    }
    (!result.is_empty()).then_some(result)
}

fn dominates(left: &BTreeMap<String, f64>, right: &BTreeMap<String, f64>) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|(key, value)| right.get(key).is_some_and(|other| value >= other))
        && left
            .iter()
            .any(|(key, value)| right.get(key).is_some_and(|other| value > other))
}

fn plan_identity(base:&str, plan:&[Value], oracle:&str) -> String {
    let plan=serde_json::to_string(plan).unwrap_or_default();
    format!("{:x}", Sha256::digest(format!("{base}\n{oracle}\n{plan}").as_bytes()))
}

/// Candidate-only checks shared by the live history and the persisted archive.
/// A stored entry is never trusted; its paired receipt is replayed again.
fn candidate_scores(state:&AppState,candidate:&Value,base:&str,plan:&[Value],oracle:&str)
    -> Option<(String,BTreeMap<String,f64>)> {
    let id=candidate["id"].as_str()?.to_string();
    if candidate["cases"] != json!(plan) || candidate["oracleVersion"] != "rsi-worktree-v2" {
        return None;
    }
    let metrics=parent_metrics(state,candidate,base)?;
    let pair=state.store.get("rsi_pair", &format!("rsi-pair-{id}"))?;
    let hash=pair["report"]["oracleSha256"].as_str()
        .filter(|hash|hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit()))?;
    if hash != oracle {return None;}
    (candidate["evaluation"]["evidenceSha256"] == pair["evidenceSha256"])
        .then_some(())?;
    Some((id.clone(),fitness(&metrics)?))
}

/// The old coordinator accepts an archive choice produced by the newly
/// compiled descendant, but only if the ORIGINAL independent evaluator
/// can replay the candidate. This decouples policy evolution from evidence
/// authority: an evolved policy may rank parents differently, but cannot
/// invent a candidate, a baseline, or a frozen oracle.
pub(crate) fn validate_selected(
    state:&AppState,selection:&Value,base:&str,cases:&Value
)->Result<(),String>{
    if selection.is_null(){return Ok(());}
    if !selection.is_object(){return Err("후손 아카이브 선택 형식 오류".into());}
    let plan=rsi::case_plan(cases)
        .map_err(|e|format!("동결 평가 사례 오류: {e}"))?;
    let oracle=format!("{:x}",Sha256::digest(
        include_bytes!("rsi_oracle_fixture.rs")));
    let id=selection["candidateId"].as_str()
        .filter(|id|!id.is_empty()).ok_or("아카이브 후보 ID 없음")?;
    if selection["baseCommit"]!=base
        ||selection["oracleSha256"]!=oracle
        ||selection["adopted"]!=false
        ||selection["qualityProven"]!=false{
        return Err("후손 선택이 원본 기준 커밋·평가 오라클에 묶이지 않았습니다.".into());
    }
    let candidate=state.store.get("rsi_candidate",id)
        .ok_or("후손이 선택한 소스 후보가 없습니다.")?;
    let Some((verified_id,_scores))=candidate_scores(state,&candidate,base,&plan,&oracle)
        else{return Err("후손이 선택한 탐색 부모가 독립 평가 재검증을 통과하지 못했습니다.".into())};
    if verified_id!=id ||selection["patchSha256"]!=candidate["patchSha256"]{
        return Err("후손 탐색 부모의 후보 ID·패치 해시가 다릅니다.".into());
    }
    Ok(())
}

/// Every verified stepping stone is persisted under the exact evaluation
/// identity (base commit + case plan + oracle). Later campaigns can therefore
/// continue the same archive instead of rebuilding it from nothing.
fn remember(state:&AppState,identity:&str,candidate:&Value)->bool {
    state.store.put("rsi_archive",&json!({
        "id":format!("{identity}:{}",candidate["patchSha256"].as_str().unwrap_or("")),
        "identity":identity,
        "candidateId":candidate["id"],
        "patchSha256":candidate["patchSha256"],
        "caseCount":candidate["cases"].as_array().map(Vec::len),
        "createdAt":chrono::Utc::now().to_rfc3339(),
    })).is_some()
}

/// A neutral patch is a stepping stone, not an adopted improvement.
/// Credit a search-parent *hypothesis* only when its model-driven child
/// changed the inherited patch and the server independently verified and
/// adopted its new source. Replaying the original paired receipt is
/// mandatory. This is observational, not a causal ablation of the parent.
fn verified_productive_offspring(
    state:&AppState,g:&Value,base:&str,oracle:&str
)->bool{
    if g["accepted"]!=true ||g["pairedMeasured"]!=true
        ||g["pairedRevalidated"]!=true ||g["inheritedPatchChanged"]!=true
        ||g["candidateOrigin"]!="model_driven_detached_worktree"
        ||g["generatorCommit"]!=base
        ||g["archiveSelection"]["baseCommit"]!=base
        ||g["archiveSelection"]["oracleSha256"]!=oracle
        ||g["archiveSelection"]["candidateId"]!=g["inheritedCandidateId"]
        ||g["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees" {
        return false;
    }
    let Some(id)=g["candidateId"].as_str() else{return false};
    let Some(candidate)=state.store.get("rsi_candidate",id) else{return false};
    candidate["status"]=="verified"
        &&candidate["baseCommit"]==base
        &&candidate["origin"]=="model_driven_detached_worktree"
        &&candidate["evaluation"]["metrics"]["eligible"]==true
        &&candidate["evaluation"]["metrics"]==g["evaluation"]["metrics"]
        &&candidate["evaluation"]["evidenceSha256"]==g["evaluation"]["evidenceSha256"]
        &&candidate["experimentAdoption"]["commit"]==g["acceptedCommit"]
        &&candidate["experimentAdoption"]["generatorCommit"]==base
        &&candidate["experimentAdoption"]["productionDeployed"]==false
        &&rsi::verified_pair_receipt(state,&candidate)
}
fn offspring_priority(attempts:usize,measured_wins:usize)->f64{
    // Bayesian smoothing avoids treating 1/1 as conclusive. Every frontier
    // candidate first receives up to two exploration attempts.
    (measured_wins as f64+1.0)/(attempts as f64+2.0)
        +0.12/(attempts as f64+1.0).sqrt()
}

/// Only completed, independently evaluated candidates enter the archive. The
/// live campaign and the persisted archive share the verification gate; a
/// same-patch alias or an unverified claim cannot consume exploration credit.
pub(crate) fn select(state: &AppState, history: &[Value], base: &str, cases: &Value) -> Value {
    let Ok(plan) = rsi::case_plan(cases) else {
        return Value::Null;
    };
    let oracle = format!(
        "{:x}",
        Sha256::digest(include_bytes!("rsi_oracle_fixture.rs"))
    );
    let identity=plan_identity(base,&plan,&oracle);
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for generation in history {
        if generation["accepted"] == true || generation["verification"]["checksPassed"] != true {
            continue;
        }
        let Some(id) = generation["candidateId"].as_str() else {
            continue;
        };
        let Some(candidate) = state.store.get("rsi_candidate", id) else {
            continue;
        };
        if generation["evaluation"]["evidenceSha256"] != candidate["evaluation"]["evidenceSha256"] {
            continue;
        }
        let Some((id,scores)) = candidate_scores(state,&candidate,base,&plan,&oracle) else {
            continue;
        };
        if !seen.insert(candidate["patchSha256"].to_string()) {continue;}
        let persisted=remember(state,&identity,&candidate);
        entries.push((id, candidate["patchSha256"].clone(), scores,persisted));
    }
    for record in state.store.all("rsi_archive") {
        if record["identity"].as_str()!=Some(identity.as_str()) {continue;}
        let Some(candidate)=record["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else {continue};
        // A persisted pointer is not trusted just because its candidate
        // happens to be independently valid. Verify the exact candidate
        // patch and archive identity before giving it search-parent credit.
        let Some(patch)=candidate["patchSha256"].as_str() else {continue};
        if record["patchSha256"]!=patch
            ||record["id"]!=format!("{identity}:{patch}")
            ||record["caseCount"].as_u64()!=Some(plan.len() as u64) {
            continue;
        }
        let Some((id,scores))=candidate_scores(state,&candidate,base,&plan,&oracle) else {continue};
        if !seen.insert(patch.to_string()) {continue;}
        entries.push((id,candidate["patchSha256"].clone(),scores,true));
    }
    let frontier = entries
        .iter()
        .filter(|entry| !entries.iter().any(|other| dominates(&other.2, &entry.2)))
        .collect::<Vec<_>>();
    // Count durable descendant attempts across campaigns. A worker may
    // crash after its job was recorded but before its generation receipt
    // exists. Such attempts must not vanish on restart and cause repeated
    // selection of a failed stepping stone. The two records share one jobId.
    let mut child_jobs=BTreeMap::<String,BTreeSet<String>>::new();
    for generation in history.iter().chain(state.store.all("rsi_generation").iter()) {
        if let (Some(parent),Some(job))=(
            generation["inheritedCandidateId"].as_str(),
            generation["jobId"].as_str()) {
            if !job.is_empty() {
                child_jobs.entry(parent.to_string()).or_default().insert(job.to_string());
            }
        }
    }
    for job in state.store.all("job") {
        if job["kind"]!="rsi_generation" || job["input"]["rsiBaseCommit"]!=base
            || job["input"]["rsiArchiveSelection"]["baseCommit"]!=base
            || job["input"]["rsiArchiveSelection"]["oracleSha256"]!=oracle {
            continue;
        }
        if let (Some(id),Some(job_id))=(
            job["input"]["rsiInheritedCandidateId"].as_str(),
            job["id"].as_str()){
            if !job_id.is_empty()
                && job["input"]["rsiArchiveSelection"]["candidateId"]==id{
                child_jobs.entry(id.to_string()).or_default().insert(job_id.to_string());
            }
        }
    }
    let children=|id:&str|child_jobs.get(id).map_or(0,BTreeSet::len);
    // Measured offspring productivity survives a campaign reset as well:
    // historical unverified outcomes NEVER turn a neutral parent into
    // an adopted improvement. This signal only ranks which parent to try.
    let mut productive=BTreeMap::<String,BTreeSet<String>>::new();
    let persisted_generations=state.store.all("rsi_generation");
    for g in history.iter().chain(persisted_generations.iter()){
        let Some(parent)=g["inheritedCandidateId"].as_str() else{continue};
        if !child_jobs.contains_key(parent)
            ||!verified_productive_offspring(state,g,base,&oracle){
            continue;
        }
        if let Some(job)=g["jobId"].as_str(){
            productive.entry(parent.to_string()).or_default().insert(job.to_string());
        }
    }
    let wins=|id:&str|productive.get(id).map_or(0,BTreeSet::len);
    let selected = frontier.iter().min_by(|left,right|{
        let la=children(&left.0);
        let ra=children(&right.0);
        // Explore all verified non-dominated stepping stones before
        // exploiting a parent with real downstream source adoptions.
        if la<2 || ra<2 {
            la.cmp(&ra).then_with(||left.0.cmp(&right.0))
        }else{
            offspring_priority(ra,wins(&right.0))
                .total_cmp(&offspring_priority(la,wins(&left.0)))
                .then_with(||la.cmp(&ra))
                .then_with(||left.0.cmp(&right.0))
        }
    });
    selected
        .map(|entry| {
            json!({
                "candidateId":entry.0,"patchSha256":entry.1,
                "policy":"verified-offspring-pareto-stepping-stones-v3","baseCommit":base,
                "oracleSha256":oracle,"archiveSize":entries.len(),"frontierSize":frontier.len(),
                "descendantAttempts":children(&entry.0),
                "independentlyAdoptedDescendants":wins(&entry.0),
                "observedOffspringPriority":offspring_priority(
                    children(&entry.0),wins(&entry.0)),
                "offspringCausedByParentProven":false,
                "persisted":entry.3,"adopted":false,"qualityProven":false,
            })
        })
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("rsi-archive-{}", uuid::Uuid::new_v4()));
        let config = Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        (
            AppState::new(
                config.clone(),
                Store::open(&config.db_path()).unwrap(),
                Vault::open(&config.key_path()),
            ),
            root,
        )
    }

    fn plan() -> Value {
        json!((0..8)
            .map(|i| json!({"id":format!("case-{i}"),"domain":"memory",
                "holdout":i<2,"filter":format!("rsi_frozen_oracle::case_{i}"),
                "metrics":[{"key":"quality","direction":"higher","minRelativeGain":0.1}]
            }))
            .collect::<Vec<_>>())
    }

    fn insert(state: &AppState, name: &str, scores: [f64; 2]) -> Value {
        let patch = format!("diff --git a/src/engine/memory.rs b/src/engine/memory.rs\n--- a/src/engine/memory.rs\n+++ b/src/engine/memory.rs\n@@ -1 +1 @@\n-old\n+{name}\n");
        let mut candidate = rsi::propose(
            state,
            &json!({"target":"algorithm",
            "baseCommit":"a".repeat(40),"patch":patch,
            "hypothesis":"A measured stepping stone for source repair.",
            "cases":plan(),"oracleVersion":"rsi-worktree-v2","generatorModel":"test"}),
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap().to_string();
        let report = json!({"baseCommit":candidate["baseCommit"],"patchSha256":candidate["patchSha256"],
            "oracleVersion":candidate["oracleVersion"],"reportSha256":"b".repeat(64),"runner":"test",
            "baselineBuildOk":true,"candidateBuildOk":true,
            "oracleSha256":format!("{:x}", Sha256::digest(include_bytes!("rsi_oracle_fixture.rs"))),
            "cases":(0..8).map(|i|json!({"id":format!("case-{i}"),
                "baselinePass":true,"candidatePass":true,
                "metrics":{"quality":{"baseline":0.5,"candidate":scores[i%2]}}
            })).collect::<Vec<_>>()});
        candidate = rsi::evaluate(state, &id, &report).unwrap();
        candidate["origin"] = json!("model_driven_detached_worktree");
        candidate["pairedComparisonComplete"] = json!(true);
        candidate["evaluation"]["source"] = json!("server_invoked_local_paired_git_worktrees");
        candidate["evaluation"]["evidenceSha256"] = json!("b".repeat(64));
        state.store.put("rsi_candidate", &candidate).unwrap();
        state
            .store
            .put(
                "rsi_pair",
                &json!({"id":format!("rsi-pair-{id}"),"candidateId":id,
            "runner":"local_gitrepo_not_os_sandbox","report":report,
            "evidenceSha256":"b".repeat(64),"metrics":candidate["evaluation"]["metrics"],
            "verdict":candidate["evaluation"]["verdict"]}),
            )
            .unwrap();
        json!({"candidateId":id,"accepted":false,"verification":{"checksPassed":true},
            "evaluation":candidate["evaluation"]})
    }

    #[test]
    fn explores_distinct_pareto_parents_and_excludes_dominated_latest_patch() {
        let (state, root) = state();
        let left = insert(&state, "left", [0.54, 0.51]);
        let right = insert(&state, "right", [0.51, 0.54]);
        let dominated = insert(&state, "dominated", [0.50, 0.50]);
        let mut history = vec![left, right, dominated];
        let selected = select(&state, &history, &"a".repeat(40), &plan());
        assert_eq!(selected["archiveSize"], 3);
        assert_eq!(selected["frontierSize"], 2);
        assert_ne!(selected["candidateId"], history[2]["candidateId"]);
        history.push(json!({"jobId":"real-child","inheritedCandidateId":selected["candidateId"]}));
        let next = select(&state, &history, &"a".repeat(40), &plan());
        assert_ne!(next["candidateId"], selected["candidateId"]);
        assert_eq!(next["adopted"], false);
        assert_eq!(next["qualityProven"], false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persisted_archive_survives_store_reopen_and_counts_unfinished_child_job() {
        let (state, root) = state();
        let base="a".repeat(40);
        let generation=insert(&state,"restart-child",[0.53,0.51]);
        let selected=select(&state,&[generation],&base,&plan());
        let id=selected["candidateId"].as_str().unwrap().to_string();
        assert_eq!(selected["descendantAttempts"],0);
        // This durable child was created before an interrupted worker
        // could produce a finished rsi_generation entry.
        state.store.put("job",&json!({
            "id":"interrupted-descendant-1",
            "kind":"rsi_generation","status":"running",
            "input":{
                "rsiBaseCommit":base,
                "rsiInheritedCandidateId":id,
                "rsiArchiveSelection":selected
            }
        })).unwrap();
        // A superficially matching job with the wrong oracle is never
        // charged to this frozen comparison identity.
        state.store.put("job",&json!({
            "id":"unrelated-child",
            "kind":"rsi_generation","status":"completed",
            "input":{
                "rsiBaseCommit":base,
                "rsiInheritedCandidateId":id,
                "rsiArchiveSelection":{"baseCommit":base,
                    "candidateId":id,"oracleSha256":"wrong-oracle"}
            }
        })).unwrap();
        drop(state);
        let config=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let resumed=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let restored=select(&resumed,&[],&base,&plan());
        assert_eq!(restored["candidateId"],id);
        assert_eq!(restored["descendantAttempts"],1);
        assert_eq!(restored["persisted"],true);
        drop(resumed);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn archived_stepping_stone_survives_into_a_later_campaign() {
        let (state, root) = state();
        let generation = insert(&state, "persisted", [0.54, 0.51]);
        let first = select(&state, &[generation.clone()], &"a".repeat(40), &plan());
        assert_eq!(first["archiveSize"], 1);
        // A restarted campaign starts with no history: the persisted entry
        // under the same base, plan and oracle must still be selectable.
        let resumed = select(&state, &[], &"a".repeat(40), &plan());
        assert_eq!(resumed["candidateId"], first["candidateId"]);
        assert_eq!(resumed["persisted"], true);
        // A different evaluation identity must never reuse that entry.
        assert!(select(&state, &[], &"c".repeat(40), &plan()).is_null());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_parent_priority_requires_real_adopted_offspring() {
        assert!(offspring_priority(6,4)>offspring_priority(6,0));
        assert!(offspring_priority(4,2)>offspring_priority(4,0));
        assert!(offspring_priority(6,0)<offspring_priority(1,0));
        let fake=json!({"accepted":true,"pairedMeasured":true,
            "pairedRevalidated":true,"inheritedPatchChanged":true,
            "candidateOrigin":"native_inductive_source_mutation",
            "generatorCommit":"a".repeat(40),
            "inheritedCandidateId":"fake","candidateId":"fake",
            "archiveSelection":{"baseCommit":"a".repeat(40),
                "oracleSha256":"b".repeat(64),"candidateId":"fake"}});
        let (state,root)=state();
        assert!(!verified_productive_offspring(
            &state,&fake,&"a".repeat(40),&"b".repeat(64)));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn compiled_descendant_archive_selection_requires_replayed_original_oracle() {
        let (state,root)=state();
        let base="a".repeat(40);
        let g=insert(&state,"descendant-choice",[0.53,0.52]);
        let parent=select(&state,&[g],&base,&plan());
        assert!(validate_selected(&state,&parent,&base,&plan()).is_ok());
        let mut changed=parent.clone();
        changed["patchSha256"]=json!("f".repeat(64));
        assert!(validate_selected(&state,&changed,&base,&plan()).is_err());
        changed=parent.clone();
        changed["oracleSha256"]=json!("e".repeat(64));
        assert!(validate_selected(&state,&changed,&base,&plan()).is_err());
        changed=parent.clone();
        changed["adopted"]=json!(true);
        assert!(validate_selected(&state,&changed,&base,&plan()).is_err());
        assert!(validate_selected(&state,&parent,&"b".repeat(40),&plan()).is_err());
        assert!(validate_selected(&state,&Value::Null,&base,&plan()).is_ok());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn archive_record_requires_exact_stored_patch_and_identity() {
        let (state,root)=state();
        let base="a".repeat(40);
        let generation=insert(&state,"immutable-record",[0.53,0.52]);
        let original=select(&state,&[generation],&base,&plan());
        let key=format!("{}:{}",
            plan_identity(&base,&rsi::case_plan(&plan()).unwrap(),
                &format!("{:x}",Sha256::digest(include_bytes!("rsi_oracle_fixture.rs")))),
            original["patchSha256"].as_str().unwrap());
        let record=state.store.get("rsi_archive",&key).unwrap();
        let mut forged=record.clone();
        forged["patchSha256"]=json!("c".repeat(64));
        state.store.put("rsi_archive",&forged).unwrap();
        assert!(select(&state,&[],&base,&plan()).is_null(),
            "An archived pointer with a changed patch hash is ineligible");
        state.store.put("rsi_archive",&record).unwrap();
        let restored=select(&state,&[],&base,&plan());
        assert_eq!(restored["candidateId"],original["candidateId"]);
        assert_eq!(restored["persisted"],true);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn persisted_archive_entry_is_reverified_not_trusted() {
        let (state, root) = state();
        let generation = insert(&state, "reverify", [0.52, 0.51]);
        let selected = select(&state, &[generation.clone()], &"a".repeat(40), &plan());
        let id = selected["candidateId"].as_str().unwrap().to_string();
        let key = format!("rsi-pair-{id}");
        let receipt = state.store.get("rsi_pair", &key).unwrap();
        let mut forged = receipt.clone();
        forged["report"]["cases"][0]["metrics"]["quality"]["candidate"] = json!(5.0);
        state.store.put("rsi_pair", &forged).unwrap();
        assert!(select(&state, &[], &"a".repeat(40), &plan()).is_null(),
            "a tampered receipt must invalidate the persisted entry");
        state.store.put("rsi_pair", &receipt).unwrap();
        assert!(select(&state, &[], &"a".repeat(40), &plan()).is_object());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_forged_pair_patch_wrong_base_plan_and_oracle() {
        let (state, root) = state();
        let generation = insert(&state, "original", [0.51, 0.51]);
        let id = generation["candidateId"].as_str().unwrap();
        let original = state.store.get("rsi_candidate", id).unwrap();
        let history = vec![generation.clone()];
        assert!(select(&state, &history, &"a".repeat(40), &plan()).is_object());
        assert!(select(&state, &history, &"c".repeat(40), &plan()).is_null());
        let mut changed_plan = plan();
        changed_plan[0]["holdout"] = json!(false);
        assert!(select(&state, &history, &"a".repeat(40), &changed_plan).is_null());
        for field in ["patch", "patchSha256"] {
            let mut forged = original.clone();
            forged[field] = json!("forged");
            state.store.put("rsi_candidate", &forged).unwrap();
            assert!(select(&state, &history, &"a".repeat(40), &plan()).is_null());
        }
        state.store.put("rsi_candidate", &original).unwrap();
        let key = format!("rsi-pair-{id}");
        let receipt = state.store.get("rsi_pair", &key).unwrap();
        let mut forged = receipt.clone();
        forged["report"]["cases"][0]["metrics"]["quality"]["candidate"] = json!(1.0);
        state.store.put("rsi_pair", &forged).unwrap();
        assert!(select(&state, &history, &"a".repeat(40), &plan()).is_null());
        let mut stale = receipt;
        stale["report"]["oracleSha256"] = json!("d".repeat(64));
        state.store.put("rsi_pair", &stale).unwrap();
        assert!(select(&state, &history, &"a".repeat(40), &plan()).is_null());
        let _ = std::fs::remove_dir_all(root);
    }
}
