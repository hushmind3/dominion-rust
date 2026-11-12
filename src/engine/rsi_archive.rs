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

/// Every verified stepping stone is persisted under the exact evaluation
/// identity (base commit + case plan + oracle). Later campaigns can therefore
/// continue the same archive instead of rebuilding it from nothing.
/// The selection also recovers sealed candidates from earlier persisted
/// generations when a process stopped before its next archive-selection step.
fn remember(state:&AppState,identity:&str,candidate:&Value) {
    let _=state.store.put("rsi_archive",&json!({
        "id":format!("{identity}:{}",candidate["patchSha256"].as_str().unwrap_or("")),
        "identity":identity,
        "candidateId":candidate["id"],
        "patchSha256":candidate["patchSha256"],
        "caseCount":candidate["cases"].as_array().map(Vec::len),
        "createdAt":chrono::Utc::now().to_rfc3339(),
    }));
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
    // Persisted generations are a recovery journal: a prior campaign may
    // stop after evaluation and before its next call to archive::select.
    // Deduplicate by patch digest across the caller's live history and DB.
    let prior_generations = state.store.all("rsi_generation");
    for generation in history.iter().chain(prior_generations.iter()) {
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
        remember(state,&identity,&candidate);
        entries.push((id, candidate["patchSha256"].clone(), scores));
    }
    for record in state.store.all("rsi_archive") {
        if record["identity"].as_str()!=Some(identity.as_str()) {continue;}
        let Some(candidate)=record["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else {continue};
        // The archive pointer is not a trusted authority. Reject a stale
        // or rewritten pointer before it can shadow a legitimate entry.
        let expected_id=format!("{identity}:{}",candidate["patchSha256"].as_str().unwrap_or(""));
        if record["id"]!=expected_id
            || record["patchSha256"]!=candidate["patchSha256"] {continue;}
        let Some((id,scores))=candidate_scores(state,&candidate,base,&plan,&oracle) else {continue};
        if !seen.insert(candidate["patchSha256"].to_string()) {continue;}
        entries.push((id,candidate["patchSha256"].clone(),scores));
    }
    let frontier = entries
        .iter()
        .filter(|entry| !entries.iter().any(|other| dominates(&other.2, &entry.2)))
        .collect::<Vec<_>>();
    // DGM's offspring penalty explores alternative stepping stones instead
    // of endlessly repairing the last neutral patch. Child attempts are
    // counted across every campaign so a restarted archive keeps its credit.
    let children = |id: &str| {
        let mut jobs=BTreeSet::new();
        for generation in history.iter().chain(prior_generations.iter()) {
            if generation["inheritedCandidateId"].as_str()==Some(id) {
                if let Some(job)=generation["jobId"].as_str() {jobs.insert(job.to_string());}
            }
        }
        jobs.len()
    };
    let selected = frontier.iter().min_by(|left, right| {
        children(&left.0)
            .cmp(&children(&right.0))
            .then_with(|| left.0.cmp(&right.0))
    });
    selected
        .map(|entry| {
            json!({
                "candidateId":entry.0,"patchSha256":entry.1,
                "policy":"paired-pareto-stepping-stones-v2","baseCommit":base,
                "oracleSha256":oracle,"archiveSize":entries.len(),"frontierSize":frontier.len(),
                "descendantAttempts":children(&entry.0),
                "persisted":true,"adopted":false,"qualityProven":false,
            })
        })
        .unwrap_or(Value::Null)
}

/// Verify a persisted archive inheritance without trusting the compiled
/// descendant's JSON summary. The source- and oracle-bound candidate receipt
/// must still replay, and the claimed archive pointer must exist.
pub(crate) fn valid_selection(
    state:&AppState, selection:&Value, base:&str, cases:&Value,
)->bool {
    if selection.is_null(){return true;}
    let Ok(plan)=rsi::case_plan(cases) else{return false};
    let oracle=format!("{:x}",Sha256::digest(include_bytes!("rsi_oracle_fixture.rs")));
    let identity=plan_identity(base,&plan,&oracle);
    let (Some(id),Some(patch))=(selection["candidateId"].as_str(),
        selection["patchSha256"].as_str()) else{return false};
    if selection["policy"]!="paired-pareto-stepping-stones-v2"
        || selection["baseCommit"]!=base
        || selection["oracleSha256"]!=oracle
        || selection["persisted"]!=true
        || selection["adopted"]!=false
        || selection["qualityProven"]!=false {return false;}
    let Some(candidate)=state.store.get("rsi_candidate",id) else{return false};
    let record_id=format!("{identity}:{patch}");
    let Some(record)=state.store.get("rsi_archive",&record_id) else{return false};
    record["identity"]==identity
        && record["candidateId"]==id
        && record["patchSha256"]==patch
        && candidate["patchSha256"]==patch
        && candidate_scores(state,&candidate,base,&plan,&oracle).is_some()
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
    fn recovers_previously_measured_candidate_after_campaign_restart_without_archive_snapshot() {
        let (state,root)=state();
        let previous=insert(&state,"crashed-before-archive-selection",[0.54,0.51]);
        let generation=json!({"id":"previous-campaign-1","campaignId":"previous",
            "candidateId":previous["candidateId"],"accepted":false,
            "verification":{"checksPassed":true},"evaluation":previous["evaluation"]});
        state.store.put("rsi_generation",&generation).unwrap();
        // No earlier select(): only the verified generation and its raw pair
        // receipt survived the process restart.
        drop(state);
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let reopened=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let selection=select(&reopened,&[],&"a".repeat(40),&plan());
        assert_eq!(selection["candidateId"],previous["candidateId"]);
        assert_eq!(selection["persisted"],true);
        assert!(reopened.store.all("rsi_archive").iter().any(|entry|
            entry["candidateId"]==previous["candidateId"]));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn cross_campaign_child_count_survives_restart_and_changes_selection() {
        let (state,root)=state();
        let left=insert(&state,"persist-left",[0.54,0.51]);
        let right=insert(&state,"persist-right",[0.51,0.54]);
        let first=select(&state,&[left,right],&"a".repeat(40),&plan());
        state.store.put("rsi_generation",&json!({
            "id":"prior-child-1","campaignId":"older-campaign",
            "jobId":"prior-child-worker","inheritedCandidateId":first["candidateId"]
        })).unwrap();
        drop(state);
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),
            workspace:root.clone()};
        let reopened=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let next=select(&reopened,&[],&"a".repeat(40),&plan());
        assert_ne!(next["candidateId"],first["candidateId"]);
        assert_eq!(next["descendantAttempts"],0);
        assert_eq!(next["frontierSize"],2);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn inherited_archive_selection_requires_replayed_candidate_and_exact_oracle() {
        let (state,root)=state();
        let generation=insert(&state,"sealed-child",[0.54,0.51]);
        let selection=select(&state,&[generation],&"a".repeat(40),&plan());
        assert!(valid_selection(&state,&selection,&"a".repeat(40),&plan()));
        let mut wrong_base=selection.clone();
        wrong_base["baseCommit"]=json!("b".repeat(40));
        assert!(!valid_selection(&state,&wrong_base,&"a".repeat(40),&plan()));
        let mut wrong_oracle=selection.clone();
        wrong_oracle["oracleSha256"]=json!("0".repeat(64));
        assert!(!valid_selection(&state,&wrong_oracle,&"a".repeat(40),&plan()));
        let mut wrong_patch=selection.clone();
        wrong_patch["patchSha256"]=json!("0".repeat(64));
        assert!(!valid_selection(&state,&wrong_patch,&"a".repeat(40),&plan()));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_rewritten_archive_pointer_on_restore() {
        let (state,root)=state();
        let candidate=insert(&state,"archive-pointer",[0.54,0.51]);
        let previous=select(&state,&[candidate],&"a".repeat(40),&plan());
        let mut records=state.store.all("rsi_archive");
        let mut record=records.remove(0);
        record["patchSha256"]=json!("not-the-original-patch");
        state.store.put("rsi_archive",&record).unwrap();
        assert!(select(&state,&[],&"a".repeat(40),&plan()).is_null());
        assert_eq!(previous["persisted"],true);
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
