//! Source-grounded, independently replayed NEGATIVE transfer between
//! intelligence capabilities. A candidate changing A may damage capability B.
//! This is an experimental hypothesis, never a claim that the accepted
//! intelligence regressed; rejected candidates are still useful counterexamples.
//! The compiled descendant may rewrite this learner in ordinary Rust source.
use crate::{engine::{rsi,rsi_intervention_lab},state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn hash(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn sha(s:&str,n:usize)->bool{
    s.len()==n && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn measured_role(domain:&str)->&str{
    match domain{
        "model"|"models"|"model_architecture"=>"model_architecture",
        "recall"|"memory"=>"memory",
        "training"|"learning"=>"learning",
        "tool-use"|"tools"=>"tools",
        "context"|"planning"|"harness"=>"harness",
        "improver"|"algorithm"|"search"|"scientific_method"
            |"improvement_methods"=>"improvement_methods",
        "ui"|"frontend"=>"frontend",
        other=>other
    }
}
fn regressed(m:&Value)->bool {
    if m["holdout"]!=true || m["baselinePass"]!=true {return false;}
    if m["candidatePass"]==false {return true;}
    m["metrics"].as_array().is_some_and(|rows|
        rows.iter().any(|row|row["regressed"]==true))
}
type CohortKey=(String,String,String,String,String);
#[derive(Default)]
struct Cases{
    candidate_ids:BTreeSet<String>,
    case_ids:BTreeSet<String>,
}
fn record(cohorts:&mut BTreeMap<CohortKey,Cases>,key:CohortKey,
    candidate:&str,case:&str) {
    let row=cohorts.entry(key).or_default();
    row.candidate_ids.insert(candidate.to_string());
    row.case_ids.insert(case.to_string());
}
/// Confirm frozen paired receipts *again*, not the generation's editable
/// claimed metrics. Repeated single-source failures on the SAME generator
/// revision/profile expose a source-victim experiment worth running next.
pub(crate) fn plan(state:&AppState,history:&[Value],frontier:&[Value],
    commit:&str)->Value {
    let mut cohorts=BTreeMap::<CohortKey,Cases>::new();
    for g in history {
        let (Some(id),Some(profile),Some(generator))=(
            g["candidateId"].as_str(),
            g["comparisonProfileSha256"].as_str().filter(|s|sha(s,64)),
            g["generatorCommit"].as_str().filter(|s|sha(s,40)))
        else{continue};
        if g["pairedMeasured"]!=true {continue;}
        // Compare independent failures under a shared, ACTUALLY used source
        // writer. Different API model actors are never a causal control.
        let Some(actor)=rsi_intervention_lab::source_actor(g)else{continue;};
        let Some(candidate)=state.store.get("rsi_candidate",id)else{continue};
        if candidate["baseCommit"]!=generator
            || candidate["evaluation"]["source"]!=
                "server_invoked_local_paired_git_worktrees"
            || candidate["paths"]!=g["changedSourcePaths"]{continue;}
        let Some(metrics)=rsi::replayed_pair_metrics(state,&candidate)
            else{continue};
        if metrics!=g["evaluation"]["metrics"]{continue;}
        let Some(changed)=candidate["paths"].as_array()
            .filter(|rows|rows.len()==1)
            .and_then(|rows|rows[0].as_str())
            .filter(|path|rsi::safe_path(path))else{continue};
        let sources=frontier.iter().filter(|row|
            row["path"]==changed
            && row["sourceSha256"].as_str().is_some_and(|s|sha(s,64)))
            .collect::<Vec<_>>();
        // If the changed source is classified as multiple roles, no unique
        // candidate source can be blamed. Reject this confounded attribution.
        if sources.len()!=1 {continue;}
        let Some(from)=sources[0]["role"].as_str()else{continue};
        for m in metrics["measurements"].as_array().into_iter().flatten(){
            if !regressed(m){continue;}
            let (Some(domain),Some(case))=(m["domain"].as_str(),
                m["id"].as_str().filter(|s|!s.is_empty()))
                else{continue};
            let to=measured_role(domain);
            if to==from || !frontier.iter().any(|r|
                r["role"]==to && r["path"]!=changed
                && r["sourceSha256"].as_str().is_some_and(|s|sha(s,64))) {
                continue;
            }
            record(&mut cohorts,(generator.to_string(),
                profile.to_string(),actor.clone(),from.to_string(),to.to_string()),id,case);
        }
    }
    let mut evidence=Vec::<Value>::new();
    for ((generator,profile,actor,from,to),case) in cohorts {
        let Some(source)=frontier.iter().find(|r|
            r["role"]==from && r["path"].as_str()
                .is_some_and(rsi::safe_path))else{continue};
        let Some(victim)=frontier.iter().find(|r|
            r["role"]==to && r["path"]!=source["path"]
                && r["sourceSha256"].as_str().is_some_and(|s|sha(s,64)))
                else{continue};
        let supported=case.candidate_ids.len()>=2 && case.case_ids.len()>=2;
        evidence.push(json!({
            "sourceRole":from,"victimRole":to,
            "sourceWritingModelStratum":actor,
            "source":source,"victim":victim,
            "generatorSourceCommit":generator,"frozenProfileSha256":profile,
            "independentCandidateCounterexamples":case.candidate_ids.len().min(8),
            "distinctDamagedHoldoutCases":case.case_ids.len().min(12),
            "candidateIds":case.candidate_ids.iter().take(8).collect::<Vec<_>>(),
            "caseIds":case.case_ids.iter().take(12).collect::<Vec<_>>(),
            "supportedForPairedInvestigation":supported,
            "candidateDamageNotInheritedRegression":true,
            "causalAttributionProven":false
        }));
    }
    evidence.sort_by(|a,b|{
        b["independentCandidateCounterexamples"].as_u64().cmp(
            &a["independentCandidateCounterexamples"].as_u64())
            .then_with(||b["distinctDamagedHoldoutCases"].as_u64().cmp(
                &a["distinctDamagedHoldoutCases"].as_u64()))
            .then_with(||a["sourceRole"].as_str().cmp(&b["sourceRole"].as_str()))
            .then_with(||a["victimRole"].as_str().cmp(&b["victimRole"].as_str()))
    });
    evidence.truncate(32);
    // Historical failures remain visible to learning. They cannot force a
    // new edit on a DIFFERENT, already rewritten source revision.
    let selected=evidence.iter().find(|row|
        row["supportedForPairedInvestigation"]==true
        && row["generatorSourceCommit"]==commit)
        .cloned().unwrap_or(Value::Null);
    let mut plan=json!({
        "source":"compiled_replayed_negative_transfer_repair_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "selected":selected,"crossCapabilityCounterexamples":evidence,
        "requiredIndependentCandidates":2,
        "requiredDistinctHoldoutCases":2,
        "selectedRequiresCurrentUnmodifiedSourceRevision":true,
        "experiment":"source_only_vs_victim_only_vs_joint",
        "noClaimOfAcceptedIntelligenceRegression":true,
        "causalityProven":false,"qualityProven":false
    });
    plan["sha256"]=json!(hash(&plan));
    plan
}
pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,len:usize)->bool{
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut()else{return false};
    obj.remove("sha256");
    if plan["source"]!="compiled_replayed_negative_transfer_repair_v1"
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(len as u64)
        || plan["sha256"].as_str()!=Some(hash(&unsigned).as_str())
        || plan["selectedRequiresCurrentUnmodifiedSourceRevision"]!=true
        || plan["noClaimOfAcceptedIntelligenceRegression"]!=true
        || plan["causalityProven"]!=false || plan["qualityProven"]!=false {
        return false;
    }
    let good=|row:&Value| row["supportedForPairedInvestigation"]==true
        && row["generatorSourceCommit"]==commit
        && row["independentCandidateCounterexamples"].as_u64().unwrap_or(0)>=2
        && row["distinctDamagedHoldoutCases"].as_u64().unwrap_or(0)>=2
        && row["candidateIds"].as_array().is_some_and(|ids|
            ids.len()>=2 && ids.len()<=8
                && row["independentCandidateCounterexamples"].as_u64()==Some(ids.len() as u64)
                && ids.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>().len()==ids.len())
        && row["caseIds"].as_array().is_some_and(|ids|
            ids.len()>=2 && ids.len()<=12
                && row["distinctDamagedHoldoutCases"].as_u64()==Some(ids.len() as u64)
                && ids.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>().len()==ids.len())
        && row["sourceWritingModelStratum"].as_str().is_some_and(|s|!s.is_empty())
        && frontier.contains(&row["source"])
        && frontier.contains(&row["victim"])
        && row["source"]["role"]!=row["victim"]["role"]
        && row["source"]["path"]!=row["victim"]["path"];
    let Some(all)=plan["crossCapabilityCounterexamples"].as_array()
        .filter(|rows|rows.len()<=32)else{return false};
    if all.iter().any(|row|
        row["source"]["sourceSha256"].as_str()
            .is_none_or(|s|!sha(s,64))
        || row["victim"]["sourceSha256"].as_str()
            .is_none_or(|s|!sha(s,64))
        || !frontier.contains(&row["source"])
        || !frontier.contains(&row["victim"])) {
        return false;
    }
    plan["selected"].is_null()
        || (good(&plan["selected"])
            && all.iter().any(|row|row==&plan["selected"]))
}
/// Frozen supervisor only replays immutable candidate receipts for selected
/// source/victim pairs; the descendant keeps ownership of its ranking
/// algorithm. Non-selected hints never grant edit or adoption authority.
pub(crate) fn validate_receipts(
    state:&AppState,history:&[Value],plan:&Value
)->Result<(),String>{
    let selected=&plan["selected"];
    if selected.is_null(){return Ok(());}
    let candidates=selected["candidateIds"].as_array()
        .ok_or("실패 후보 영수증 목록 없음")?;
    let damaged=selected["caseIds"].as_array()
        .ok_or("손상 홀드아웃 기록 없음")?;
    let commit=selected["generatorSourceCommit"].as_str()
        .ok_or("소스 세대 없음")?;
    let profile=selected["frozenProfileSha256"].as_str()
        .ok_or("동결 프로필 없음")?;
    let actor=selected["sourceWritingModelStratum"].as_str()
        .ok_or("모델 실행 계층 없음")?;
    let source=selected["source"]["path"].as_str()
        .ok_or("변경된 소스 경로 없음")?;
    let victim=selected["victimRole"].as_str()
        .ok_or("피해 역할 없음")?;
    let mut independently_damaged=BTreeSet::<String>::new();
    let mut seen_candidates=BTreeSet::<String>::new();
    for id in candidates {
        let id=id.as_str().filter(|s|!s.is_empty())
            .ok_or("실패 후보 ID 없음")?;
        if !seen_candidates.insert(id.to_string()){
            return Err("독립 실패 후보 ID를 중복 계산했습니다.".into());
        }
        let row=history.iter().find(|g|
            g["candidateId"]==id
            && g["generatorCommit"]==commit
            && g["comparisonProfileSha256"]==profile
            && g["pairedMeasured"]==true
            && g["changedSourcePaths"]==json!([source])
            && rsi_intervention_lab::source_actor(g).as_deref()==Some(actor))
            .ok_or("실패 영수증이 원래 세대·평가 조건·모델에서 생성되지 않았습니다.")?;
        let candidate=state.store.get("rsi_candidate",id)
            .ok_or("원본 실패 후보가 없음")?;
        if candidate["baseCommit"]!=commit || candidate["paths"]!=json!([source])
            || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"{
            return Err("실패 후보의 출처 또는 편집 범위 불일치".into());
        }
        let metrics=rsi::replayed_pair_metrics(state,&candidate)
            .ok_or("동결 평가 영수증을 독립 재계산하지 못했습니다.")?;
        if metrics!=row["evaluation"]["metrics"]{
            return Err("과거 세대가 보고한 평가와 원본이 다릅니다.".into());
        }
        for m in metrics["measurements"].as_array().into_iter().flatten(){
            if regressed(m) && m["domain"].as_str().map(measured_role)==Some(victim) {
                if let Some(id)=m["id"].as_str(){independently_damaged.insert(id.to_string());}
            }
        }
    }
    if damaged.iter().any(|v|v.as_str().is_none_or(|id|
        !independently_damaged.contains(id))){
        return Err("서로 다른 동결 홀드아웃 손상 증거가 부족합니다.".into());
    }
    Ok(())
}
pub(crate) fn instruction(plan:&Value)->String{
    let pair=&plan["selected"];
    if pair.is_null(){return String::new()}
    format!(
        "INDEPENDENT WHOLE-INTELLIGENCE NEGATIVE TRANSFER: a source-only \
         change role={} file={} repeatedly damaged another previously \
         passing capability role={} file={} under SAME frozen source/profile; \
         distinct candidates={}, damaged holdout cases={}. \
         Rewrite or experimentally contrast BOTH source files. \
         Test source-only, victim-only, and joint repairs on independently \
         frozen heldouts; never treat the rejected candidate's damage as \
         a regression of the adopted intelligence. Experimental cause is NOT \
         proven. This learning algorithm is descendant-editable.",
         pair["sourceRole"],pair["source"]["path"],pair["victimRole"],
         pair["victim"]["path"],pair["independentCandidateCounterexamples"],
         pair["distinctDamagedHoldoutCases"])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counterexamples_require_distinct_trials_and_cases_on_one_cohort(){
        let mut counts=BTreeMap::new();
        let k=("a".repeat(40),"b".repeat(64),"declared:actor".into(),
            "model_architecture".to_string(),"memory".to_string());
        record(&mut counts,k.clone(),"candidate-1","holdout-1");
        record(&mut counts,k.clone(),"candidate-1","holdout-1");
        assert_eq!(counts[&k].candidate_ids.len(),1);
        assert_eq!(counts[&k].case_ids.len(),1);
        record(&mut counts,k.clone(),"candidate-2","holdout-2");
        assert_eq!(counts[&k].candidate_ids.len(),2);
        assert_eq!(counts[&k].case_ids.len(),2);
        let other=("a".repeat(40),"c".repeat(64),"declared:actor".into(),
            "model_architecture".to_string(),"memory".to_string());
        record(&mut counts,other.clone(),"candidate-3","holdout-3");
        assert_eq!(counts[&other].candidate_ids.len(),1,
            "a changed frozen profile may not inflate the original cohort");
    }
    #[test]
    fn original_paired_receipts_select_two_real_intelligence_sources(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(
            format!("rsi-negative-transfer-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let source="src/engine/models.rs";
        let frontier=vec![
            json!({"role":"model_architecture","path":source,"domain":"models",
                "sourceSha256":"a".repeat(64)}),
            json!({"role":"memory","path":"src/engine/memory.rs",
                "domain":"memory","sourceSha256":"b".repeat(64)})];
        let head="c".repeat(40);
        let profile="d".repeat(64);
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),
            "domain":"memory","holdout":i<4,"metrics":[]}))
            .collect::<Vec<_>>();
        let mut history=Vec::new();
        for trial in 0..2 {
            let id=format!("negative-candidate-{trial}");
            let raw=(0..8).map(|i|json!({"id":format!("case-{i}"),
                "baselinePass":true,
                "candidatePass":i!=trial,"metrics":{}}))
                .collect::<Vec<_>>();
            let metrics=rsi::evaluate_cases(&cases,&raw,"models").unwrap();
            assert_eq!(metrics["verdict"],"regressed");
            let receipt=format!("{:0>64}",trial+1);
            let patch=format!("{:0>64}",trial+5);
            state.store.put("rsi_candidate",&json!({
                "id":id,"baseCommit":head,"target":"models",
                "patchSha256":patch,"oracleVersion":"rsi-worktree-v2",
                "cases":cases,"paths":[source],
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                    "metrics":metrics,"verdict":"regressed",
                    "evidenceSha256":receipt}
            })).unwrap();
            state.store.put("rsi_pair",&json!({
                "id":format!("rsi-pair-{id}"),"candidateId":id,
                "runner":"local_gitrepo_not_os_sandbox",
                "evidenceSha256":receipt,
                "metrics":metrics,"verdict":"regressed",
                "report":{"reportSha256":receipt,"baseCommit":head,
                    "patchSha256":patch,"oracleVersion":"rsi-worktree-v2",
                    "baselineBuildOk":true,"candidateBuildOk":true,
                    "cases":raw}
            })).unwrap();
            history.push(json!({"candidateId":id,"generatorCommit":head,
                "generatorProviderId":"provider","generatorModel":"source-writer",
                "comparisonProfileSha256":profile,"pairedMeasured":true,
                "changedSourcePaths":[source],"evaluation":{"metrics":metrics}}));
        }
        let result=plan(&state,&history,&frontier,&head);
        assert_eq!(result["selected"]["sourceRole"],"model_architecture");
        assert_eq!(result["selected"]["victimRole"],"memory");
        assert_eq!(result["selected"]["independentCandidateCounterexamples"],2);
        assert_eq!(result["selected"]["distinctDamagedHoldoutCases"],2);
        assert!(valid(&result,&frontier,&head,2));
        assert!(validate_receipts(&state,&history,&result).is_ok());
        // Distinct source-writing actors cannot be pooled as a single
        // repeated regression, even on an identical frozen case profile.
        history[1]["generatorModel"]=json!("another-writer");
        let mixed=plan(&state,&history,&frontier,&head);
        assert!(mixed["selected"].is_null());
        history[1]["generatorModel"]=json!("source-writer");
        let mut duplicate=result.clone();
        duplicate["selected"]["candidateIds"]=json!([
            "negative-candidate-0","negative-candidate-0"]);
        assert!(!valid(&duplicate,&frontier,&head,2));
        assert!(validate_receipts(&state,&history,&duplicate).is_err());
        let mut forged_plan=result.clone();
        forged_plan["selected"]["victim"]["path"]=json!("src/engine/other.rs");
        let mut unsigned=forged_plan.clone();
        unsigned.as_object_mut().unwrap().remove("sha256");
        forged_plan["sha256"]=json!(hash(&unsigned));
        assert!(!valid(&forged_plan,&frontier,&head,2),
            "even a rehashed supervisor proposal cannot invent source ownership");
        let next_revision=plan(&state,&history,&frontier,&"e".repeat(40));
        assert!(next_revision["selected"].is_null(),
            "old failures cannot force a joint edit after source code changed");
        // One untrusted self-reported damaged case cannot unlock a new
        // two-source edit objective; only the replayed original pair does.
        // Trial 1 damaged holdout 1, not holdout 0. Forge the real
        // failure and ensure revalidation rejects the changed claim.
        history[1]["evaluation"]["metrics"]["measurements"][1]["candidatePass"]
            =json!(true);
        let forged=plan(&state,&history,&frontier,&head);
        assert!(forged["selected"].is_null());
        assert!(valid(&forged,&frontier,&head,2));
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn negative_transfer_requires_a_previously_passing_holdout(){
        assert!(!regressed(&json!({"holdout":true,"baselinePass":false,
            "candidatePass":false})));
        assert!(!regressed(&json!({"holdout":false,"baselinePass":true,
            "candidatePass":false})));
        assert!(regressed(&json!({"holdout":true,"baselinePass":true,
            "candidatePass":false})));
        assert!(regressed(&json!({"holdout":true,"baselinePass":true,
            "candidatePass":true,"metrics":[{"regressed":true}]})));
    }
}
