//! Source-grounded meta-learning of *improvement principles*, not merely
//! the count of accepted source patches.
//!
//! Frozen paired evaluations are the reward source. A recompiled descendant
//! learns which model-authored mechanism improves actual held-out task cases,
//! which cases regress, and whether its accepted descendant improves a
//! DIFFERENT intelligence subsystem. It uses those observations to select
//! and rewrite future improvement programs. Nothing here edits the oracle,
//! claims parameter fine-tuning, or treats proposal text as measured gain.
//!
//! This learner is ordinary editable Rust. A later independently adopted
//! source revision may replace its feature extraction, credit assignment,
//! search algorithm, or synthesis policy.
use crate::engine::{rsi,rsi_mutual_program};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn sha_ok(v:&Value)->bool {
    v.as_str().is_some_and(|s|s.len()==64
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn independent_pair(g:&Value)->bool {
    (g["pairedMeasured"]==true||g["pairedRevalidated"]==true)
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && g["evaluation"]["metrics"]["measurements"].is_array()
        && g["evaluation"]["metrics"]["verdict"].as_str().is_some()
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["generatorCommit"].as_str().is_some_and(|s|s.len()==40)
        && sha_ok(&g["evaluationProfileSha256"])
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
}
/// An independently tested episode can derive its mechanism from EITHER
/// a compiled open-program node OR a real model-authored whole-system design.
/// This allows every earlier A->B->A, factorial and model/harness redesign
/// to teach the new learner; the graph is not a prerequisite for learning.
fn grounded_mechanism(g:&Value)->Option<(String,String,String)> {
    let changes=g["changedSourcePaths"].as_array()?;
    let plan=&g["openImprovementProgram"];
    let open_path=plan["target"]["path"].as_str();
    let open_executed=plan["source"]=="compiled_descendant_open_improvement_program_v1"
        && plan["revision"].as_str().is_some_and(|s|s.len()==64)
        && g["compiledPlanExecuted"]["openImprovementRevision"]==plan["revision"]
        && g["compiledPlanExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["metaPolicy"]["selectionMode"]=="authored_open_improvement_program"
        && g["metaPolicy"]["target"]==plan["target"]
        && g["metaPolicy"]["openImprovementRevision"]==plan["revision"];
    if open_executed {
        if let (Some(path),Some(role),Some(mechanism))=(
            open_path,plan["target"]["role"].as_str(),
            plan["node"]["mechanism"].as_str()) {
            if rsi::safe_path(path) && !mechanism.is_empty()
                && changes.iter().any(|p|p.as_str()==Some(path)) {
                return Some((mechanism.to_string(),role.to_string(),path.to_string()));
            }
        }
    }
    let design=&g["systemDesign"];
    let authentic=design["source"]=="model_authored_system_design"
        && design["generatorCommit"]==g["generatorCommit"]
        && design["generationJobId"]==g["jobId"]
        && g["compiledPlanExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["metaPolicy"]["revision"].as_str().is_some();
    if !authentic{return None;}
    let mechanism=design["hypothesis"].as_str()
        .filter(|s|(20..=1600).contains(&s.chars().count()))?;
    let path=design["targetFiles"].as_array()?.iter().filter_map(Value::as_str)
        .find(|path|rsi::safe_path(path)
            && changes.iter().any(|changed|changed.as_str()==Some(path)))?;
    // A coupled design is tracked as coupled, not falsely credited to
    // whichever component happened to be first in the source list.
    let role=if design["component"]=="system" {
        "coupled_system"
    }else {design["component"].as_str()?};
    if role.is_empty(){return None;}
    Some((mechanism.to_string(),role.to_string(),path.to_string()))
}
fn independent_trial(g:&Value)->bool {
    independent_pair(g) && grounded_mechanism(g).is_some()
}
/// Independent supervisor attestation of every case-level learning row.
/// The text stored in a generation is NOT an oracle: replay the original
/// candidate-scoped, frozen paired receipt from the protected state store.
/// Reject an otherwise convincing generated summary when the raw metrics,
/// base commit, changed path or adopted commit disagree.
pub(crate) fn verify_replay(state:&crate::state::AppState,history:&[Value])->bool {
    for g in history.iter().filter(|g|independent_trial(g)) {
        let Some(id)=g["candidateId"].as_str() else{return false};
        let Some(candidate)=state.store.get("rsi_candidate",id) else{return false};
        let Some(metrics)=rsi::replayed_pair_metrics(state,&candidate) else{return false};
        if candidate["id"]!=id
            || candidate["baseCommit"]!=g["generatorCommit"]
            || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
            || candidate["evaluation"]["metrics"]!=metrics
            || g["evaluation"]["metrics"]!=metrics {
            return false;
        }
        let Some((_,_,path))=grounded_mechanism(g) else{return false};
        if !candidate["paths"].as_array().is_some_and(|paths|
            paths.iter().any(|v|v.as_str()==Some(path.as_str()))) {
            return false;
        }
        if g["accepted"]==true
            && (candidate["status"]!="verified"
                || !rsi::verified_pair_receipt(state,&candidate)
                || candidate["experimentAdoption"]["commit"]!=g["acceptedCommit"]
                || candidate["experimentAdoption"]["generatorCommit"]!=g["generatorCommit"]) {
            return false;
        }
    }
    true
}
fn accepted_measured(g:&Value)->bool {
    independent_trial(g)
        && rsi_mutual_program::adopted_source(g)
        && g["evaluation"]["metrics"]["eligible"]==true
        && grounded_mechanism(g).is_some_and(|(_,_,path)|
            g["acceptedSourceHashes"][path.as_str()].as_str().is_some_and(|s|
                s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit())))
}
/// A downstream descendant may use a completely DIFFERENT improvement
/// method. Credit real outcome data by role even when no openProgram was
/// executed in that child. Never require a hand-written improvement path.
fn adopted_child(g:&Value)->bool {
    independent_pair(g)
        && rsi_mutual_program::adopted_source(g)
        && g["evaluation"]["metrics"]["eligible"]==true
        && !rsi_mutual_program::implemented_roles(g).is_empty()
}
fn changed_child_roles(g:&Value)->Vec<String> {
    if adopted_child(g) {
        return rsi_mutual_program::implemented_roles(g);
    }
    // A measured rejection still gives a loss/counterexample. Its target
    // role must come from an authentic model-authored design AND an actual
    // changed file, not from mere declared multi-role intent.
    let design=&g["systemDesign"];
    if !independent_pair(g)
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=g["generatorCommit"]
        || design["generationJobId"]!=g["jobId"] {return Vec::new();}
    let mut roles=BTreeSet::new();
    for (role,paths) in design["componentFiles"].as_object().into_iter().flat_map(|m|m.iter()) {
        if paths.as_array().is_some_and(|files|files.iter().any(|file|
            g["changedSourcePaths"].as_array().is_some_and(|changes|
                changes.iter().any(|change|change==file)))) {
            roles.insert(role.clone());
        }
    }
    roles.into_iter().collect()
}
fn case_delta(m:&Value)->Option<i8> {
    if m["holdout"]!=true {return None;}
    if let Some(qualities)=m["metrics"].as_array() {
        for metric in qualities.iter().filter(|metric|metric["key"]=="quality") {
            if metric["improved"]==true{return Some(1);}
            if metric["regressed"]==true{return Some(-1);}
            if metric["improved"]==false && metric["regressed"]==false{
                return Some(0);
            }
            let before=metric["baseline"].as_f64()?;
            let after=metric["candidate"].as_f64()?;
            return Some(if after>before{1}else if after<before{-1}else{0});
        }
    }
    match (m["baselinePass"].as_bool(),m["candidatePass"].as_bool()) {
        (Some(false),Some(true))=>Some(1),
        (Some(true),Some(false))=>Some(-1),
        (Some(_),Some(_))=>Some(0),
        _=>None
    }
}
fn cases(g:&Value)->Vec<(String,String,i8)> {
    let mut seen=BTreeSet::new();
    g["evaluation"]["metrics"]["measurements"].as_array()
        .into_iter().flatten().filter_map(|row|{
            let domain=row["domain"].as_str()
                .or_else(||g["domain"].as_str())
                .filter(|s|!s.is_empty())?;
            let id=row["id"].as_str().filter(|s|!s.is_empty())?;
            let change=case_delta(row)?;
            if !seen.insert((domain.to_owned(),id.to_owned())){return None;}
            Some((domain.to_owned(),id.to_owned(),change))
        }).collect()
}
/// Calibrate the *selection algorithm itself*. Only forecasts that were
/// chosen BEFORE independent case evaluation enter this error signal.
/// Model-authored confidence or source-adoption alone earns no credit.
fn predictive_calibration(history:&[Value])->Value {
    let mut results=Vec::<(f64,Value)>::new();
    let mut seen=BTreeSet::new();
    for g in history {
        if !independent_trial(g){continue;}
        let plan=&g["openImprovementProgram"];
        let Some(predicted)=plan["principleSelectionEvidence"]["caseQualityPosterior"]
            .as_f64().filter(|p|p.is_finite()&&(0.0..=1.0).contains(p))
            else{continue};
        if !sha_ok(&plan["principleLearningSha256"])
            || g["principleLearning"]["sha256"]!=plan["principleLearningSha256"] {
            continue;
        }
        let Some(candidate)=g["candidateId"].as_str() else{continue};
        for (domain,id,delta) in cases(g) {
            if !seen.insert((candidate.to_owned(),domain.clone(),id.clone())){continue;}
            let reward=if delta>0{1.0}else{0.0};
            let error=(predicted-reward).powi(2);
            results.push((error,json!({
                "generationId":g["id"],"candidateId":candidate,"domain":domain,
                "caseId":id,"forecast":predicted,"caseImproved":delta>0,
                "brierError":error,"source":"precommitted_compiled_principle_estimate"
            })));
        }
    }
    let trials=results.len();
    let mean=if trials>0 {
        results.iter().map(|x|x.0).sum::<f64>()/trials as f64
    }else{0.0};
    results.sort_by(|a,b|b.0.total_cmp(&a.0));
    json!({
        "source":"independent_case_level_meta_predictor_calibration_v1",
        "measuredCases":trials,"meanBrierError":mean,
        "highErrorCases":results.into_iter().take(6).map(|v|v.1).collect::<Vec<_>>(),
        "needsLearnerSourceRevision":trials>=3 && mean>0.28,
        "recommendsEditableSource":"src/engine/rsi_principle_learning.rs",
        "fixedOracleUnchanged":true,
        "qualityImprovedByRevision":false
    })
}
#[derive(Default)]
struct Aggregate {
    attempts:u64,
    independently_adopted:u64,
    cases:u64,
    improved:u64,
    regressed:u64,
    neutral:u64,
    successor_cases:u64,
    successor_gains:u64,
    successor_regressions:u64,
    cross_role_gains:u64,
    domains:BTreeSet<String>,
    case_keys:BTreeSet<String>,
    generation_ids:BTreeSet<String>,
    next_roles:BTreeSet<String>,
    roles:BTreeSet<String>,
    transferable_keys:BTreeSet<String>,
}
#[derive(Default)]
struct Transfer {
    mechanisms:BTreeSet<String>,
    domains:BTreeSet<String>,
    cases:u64,
    gains:u64,
    regressions:u64,
    successor_cases:u64,
    successor_gains:u64,
    successor_regressions:u64,
}
/// The descendant's learned table is deterministic given its durable record.
/// Cases are deduplicated per independent candidate and frozen profile.
/// Importantly, source adoption is NEVER counted as a task-quality success.
pub(crate) fn study(history:&[Value])->Value {
    let mut groups=BTreeMap::<String,Aggregate>::new();
    let mut seen_candidate=BTreeSet::new();
    for (i,g) in history.iter().enumerate() {
        if !independent_trial(g){continue;}
        let Some(candidate)=g["candidateId"].as_str() else{continue};
        let profile=g["evaluationProfileSha256"].as_str().unwrap_or("");
        if !seen_candidate.insert((profile.to_owned(),candidate.to_owned())) {
            continue;
        }
        let Some((mechanism,role,_path))=grounded_mechanism(g)
            else{continue};
        let mechanism=mechanism.trim().to_owned();
        if mechanism.is_empty()||mechanism.len()>1600{continue;}
        let group=groups.entry(mechanism.clone()).or_default();
        group.attempts+=1;
        group.roles.insert(role.clone());
        if g["openImprovementProgram"]["source"]
            =="compiled_descendant_open_improvement_program_v1"
            && g["metaPolicy"]["selectionMode"]=="authored_open_improvement_program"
            && g["compiledPlanExecuted"]["openImprovementRevision"]
                ==g["openImprovementProgram"]["revision"]
            && g["openImprovementProgram"]["node"]["mechanism"]==mechanism.as_str() {
            for factor in g["openImprovementProgram"]["node"]["principleKeys"]
                .as_array().into_iter().flatten().filter_map(Value::as_str) {
                if (3..=48).contains(&factor.len())
                    && factor.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_') {
                    group.transferable_keys.insert(factor.to_owned());
                }
            }
        }
        if let Some(id)=g["id"].as_str(){group.generation_ids.insert(id.to_string());}
        let adopted=accepted_measured(g);
        if adopted{group.independently_adopted+=1;}
        for (domain,case,delta) in cases(g){
            let id=format!("{profile}:{candidate}:{domain}:{case}");
            if !group.case_keys.insert(id){continue;}
            group.cases+=1;
            group.domains.insert(domain);
            match delta{1=>group.improved+=1,-1=>group.regressed+=1,_=>group.neutral+=1}
        }
        // Future offspring productivity: evidence must come from a NEW
        // independently evaluated child generated by the accepted source,
        // not the original candidate's own quality or a model's promise.
        if !adopted{continue;}
        let Some(commit)=g["acceptedCommit"].as_str() else{continue};
        let mut seen_children=BTreeSet::new();
        for child in history.iter().skip(i+1).filter(|child|
            child["generatorCommit"].as_str()==Some(commit)
            && independent_pair(child)) {
            let Some(child_candidate)=child["candidateId"].as_str() else{continue};
            if !seen_children.insert(child_candidate.to_string()){continue;}
            let other_roles=changed_child_roles(child).into_iter()
                .filter(|r|r!=&role).collect::<Vec<_>>();
            if other_roles.is_empty(){continue;}
            let child_adopted=adopted_child(child);
            for (domain,_case,delta) in cases(child) {
                group.successor_cases+=1;
                for other in &other_roles {group.next_roles.insert(other.clone());}
                group.domains.insert(domain);
                if delta>0 && child_adopted {
                    group.successor_gains+=1;
                    group.cross_role_gains+=1;
                }else if delta<0 {
                    group.successor_regressions+=1;
                }
            }
        }
    }
    let rows=groups.into_iter().map(|(mechanism,g)|{
        // Beta(1,1) shrinkage means two lucky cases cannot masquerade as
        // a universal law. Regression and neutral cases remain training data.
        let posterior=(g.improved as f64+1.0)/(g.cases as f64+2.0);
        let next_posterior=(g.successor_gains as f64+1.0)
            /(g.successor_cases as f64+2.0);
        json!({
            "mechanism":mechanism,"observedAttempts":g.attempts,
            "independentSourceAdoptions":g.independently_adopted,
            "measuredCases":g.cases,"improvedCases":g.improved,
            "regressedCases":g.regressed,"neutralCases":g.neutral,
            "nextGenerationCases":g.successor_cases,
            "nextGenerationImprovements":g.successor_gains,
            "nextGenerationRegressions":g.successor_regressions,
            "crossRoleImprovements":g.cross_role_gains,
            "posteriorCaseGain":posterior,
            "posteriorDescendantGain":next_posterior,
            "domains":g.domains.into_iter().collect::<Vec<_>>(),
            "changedRoles":g.roles.into_iter().collect::<Vec<_>>(),
            "descendantRoles":g.next_roles.into_iter().collect::<Vec<_>>(),
            "principleKeys":g.transferable_keys.into_iter().collect::<Vec<_>>(),
            "generationIds":g.generation_ids.into_iter().take(12).collect::<Vec<_>>(),
            "causalAttributionProven":false
        })
    }).collect::<Vec<_>>();
    // Independent, model-invented abstraction keys can identify shared
    // mechanisms even when programs use entirely different descriptions
    // and target subsystems. The keys carry observed task rewards, never
    // declarative credit merely for being present in an untested program.
    let mut factors=BTreeMap::<String,Transfer>::new();
    for row in &rows {
        for key in row["principleKeys"].as_array().into_iter().flatten()
            .filter_map(Value::as_str) {
            let f=factors.entry(key.to_string()).or_default();
            if let Some(mechanism)=row["mechanism"].as_str(){
                f.mechanisms.insert(mechanism.to_owned());
            }
            for domain in row["domains"].as_array().into_iter().flatten()
                .filter_map(Value::as_str) {
                f.domains.insert(domain.to_owned());
            }
            f.cases+=row["measuredCases"].as_u64().unwrap_or(0);
            f.gains+=row["improvedCases"].as_u64().unwrap_or(0);
            f.regressions+=row["regressedCases"].as_u64().unwrap_or(0);
            f.successor_cases+=row["nextGenerationCases"].as_u64().unwrap_or(0);
            f.successor_gains+=row["nextGenerationImprovements"].as_u64().unwrap_or(0);
            f.successor_regressions+=row["nextGenerationRegressions"].as_u64().unwrap_or(0);
        }
    }
    let features=factors.into_iter().map(|(name,f)|{
        let support=f.mechanisms.len();
        json!({
            "key":name,"independentMechanisms":support,
            "measuredCases":f.cases,"improvedCases":f.gains,
            "regressedCases":f.regressions,
            "nextGenerationCases":f.successor_cases,
            "nextGenerationImprovements":f.successor_gains,
            "nextGenerationRegressions":f.successor_regressions,
            "posteriorCaseGain":(f.gains as f64+1.0)/(f.cases as f64+2.0),
            "posteriorDescendantGain":(f.successor_gains as f64+1.0)
                /(f.successor_cases as f64+2.0),
            "domains":f.domains.into_iter().collect::<Vec<_>>(),
            "transferEligible":support>=2 && f.cases>=2,
            "causalFeatureProven":false
        })
    }).collect::<Vec<_>>();
    let mut ledger=json!({
        "source":"independently_measured_improvement_principle_learner_v1",
        "historyCount":history.len(),
        "principles":rows,
        "transferFeatures":features,
        "effectSource":"frozen_holdout_case_level_and_rebuilt_descendant",
        "sourceAdoptionIsNotTaskGain":true,
        "crossRoleEffectIsObservational":true,
        "predictiveCalibration":predictive_calibration(history),
        "modelWeightsTrained":false,
        "editableLearningSource":"src/engine/rsi_principle_learning.rs",
        "qualityProvenForNewProgram":false
    });
    ledger["sha256"]=json!(digest(&ledger));
    ledger
}
pub(crate) fn valid(ledger:&Value,history:&[Value])->bool {
    ledger==&study(history)
}
/// This function is the executable meta-learner's ranking algorithm. It
/// can itself be rewritten, together with this module, by a descendant.
/// Unmeasured ideas remain exploration hypotheses instead of fake gains.
pub(crate) fn estimate(ledger:&Value,node:&Value)->Value {
    let mechanism=node["mechanism"].as_str().unwrap_or("");
    let row=ledger["principles"].as_array().into_iter().flatten()
        .find(|row|row["mechanism"]==mechanism);
    // A previously unseen mechanism can inherit a predictive PRIOR
    // from a model-invented abstraction shared by at least two separately
    // evaluated mechanisms. Pick one supported factor to avoid counting
    // the same underlying task case multiple times through overlapping tags.
    let factor=if row.is_none() {
        ledger["transferFeatures"].as_array().into_iter().flatten()
            .filter(|feature|feature["transferEligible"]==true
                && node["principleKeys"].as_array().is_some_and(|keys|
                    keys.iter().any(|k|k==&feature["key"])))
            .max_by(|a,b|a["independentMechanisms"].as_u64()
                .cmp(&b["independentMechanisms"].as_u64())
                .then_with(||a["measuredCases"].as_u64()
                    .cmp(&b["measuredCases"].as_u64())))
    }else{None};
    let evidence=row.or(factor);
    let evidence_source=if row.is_some(){"exact_executed_mechanism"}
        else if factor.is_some(){"cross_mechanism_transferable_principle"}
        else{"unmeasured_prior"};
    let trials=evidence.and_then(|r|r["measuredCases"].as_u64()).unwrap_or(0);
    let descendant_cases=evidence.and_then(|r|r["nextGenerationCases"].as_u64())
        .unwrap_or(0);
    let quality=evidence.and_then(|r|r["posteriorCaseGain"].as_f64()).unwrap_or(0.5);
    let future=evidence.and_then(|r|r["posteriorDescendantGain"].as_f64()).unwrap_or(0.5);
    let regressions=evidence.and_then(|r|r["regressedCases"].as_u64()).unwrap_or(0);
    let child_regressions=evidence.and_then(|r|r["nextGenerationRegressions"].as_u64())
        .unwrap_or(0);
    let breadth=evidence.and_then(|r|r["domains"].as_array()).map(Vec::len)
        .unwrap_or(0);
    let novelty=0.16 / ((trials+1) as f64).sqrt();
    let risk=(regressions+child_regressions) as f64
        / ((trials+descendant_cases).max(1) as f64);
    let breadth_bonus=if breadth>1 {0.08*(breadth.min(4) as f64-1.0)/3.0}
        else{0.0};
    // Conservatively rate downstream ability separately from immediate wins.
    let score=0.50*quality+0.35*future+novelty+breadth_bonus-0.20*risk.min(1.0);
    json!({
        "mechanism":mechanism,"estimatedPriority":score,
        "caseQualityPosterior":quality,
        "futureDescendantPosterior":future,
        "measuredCases":trials,"nextGenerationCases":descendant_cases,
        "noveltyExploration":novelty,"crossTaskBreadth":breadth,
        "learnedEvidenceSource":evidence_source,
        "transferredPrincipleKey":factor.map(|r|r["key"].clone()),
        "crossMechanismGeneralizationIsHypothesis":factor.is_some(),
        "penaltyFromRegressions":risk,
        "predictionNotIndependentProof":true
    })
}
/// Short feedback targeted at WHAT future agent/model should learn, not
/// the old scripted graph transitions. A model may invent new hypotheses,
/// components, training procedures and meta-learners from these examples.
pub(crate) fn instruction(ledger:&Value)->String {
    let mut rows=ledger["principles"].as_array().cloned().unwrap_or_default();
    rows.sort_by(|a,b|{
        let ea=estimate(ledger,&json!({"mechanism":a["mechanism"]}));
        let eb=estimate(ledger,&json!({"mechanism":b["mechanism"]}));
        eb["estimatedPriority"].as_f64().unwrap_or(0.0)
            .total_cmp(&ea["estimatedPriority"].as_f64().unwrap_or(0.0))
    });
    let calibration=&ledger["predictiveCalibration"];
    let transfer=ledger["transferFeatures"].as_array().into_iter().flatten()
        .filter(|f|f["transferEligible"]==true)
        .take(5).map(|f|format!(
            "transferable principle={} independent-mechanisms={} measured-cases={} observed-domains={} task-gains={} regressions={}",
            f["key"],f["independentMechanisms"],f["measuredCases"],
            f["domains"],f["improvedCases"],f["regressedCases"]))
        .collect::<Vec<_>>().join("\n");
    let top=rows.iter().take(5).map(|row|format!(
        "mechanism={} measured={} gains={} regressions={} next-generation cases={} next-generation improvements={} across-domains={}",
        row["mechanism"],row["measuredCases"],row["improvedCases"],
        row["regressedCases"],row["nextGenerationCases"],
        row["nextGenerationImprovements"],row["domains"]
    )).collect::<Vec<_>>().join("\n");
    format!(
        "VERIFIED PRINCIPLE LEARNING (ledger={}):\n{}\n\
         CROSS-MECHANISM ABSTRACTIONS (independently supported):\n{}\n\
         These are independently evaluated task effects and observational \
         successor effects, not proof of a universal causal law. \
         Predictor calibration: {} cases; mean Brier error={}; \
         propose source-level revision of learner={}. \
         Diagnose which MECHANISM fails to transfer, and invent a new, \
         falsifiable improvement rule by rewriting the learning algorithm, \
         API model routing, memory, harness, tool behavior and learning algorithms (NOT external API weights). \
         Propose a new openProgram graph and implement the actual source. \
         Reuse good mechanisms across domains only when case-level evidence \
         supports transfer. Do not optimize raw patch-count or self-reported \
         acceptance. This learner's source can itself be redesigned.",
        ledger["sha256"],top,transfer,calibration["measuredCases"],
        calibration["meanBrierError"],calibration["needsLearnerSourceRevision"])
}
#[cfg(test)]
mod tests {
    use super::*;
    fn trial(id:&str,mechanism:&str,source:&str,target_role:&str,
        before:bool,after:bool,accepted:bool)->Value {
        let mut hashes=serde_json::Map::new();
        hashes.insert(source.into(),json!("b".repeat(64)));
        let generator="a".repeat(40);
        let mut assignments=serde_json::Map::new();
        assignments.insert(target_role.into(),json!([source]));
        json!({
            "id":id,"jobId":format!("job-{id}"),"candidateId":format!("candidate-{id}"),
            "generatorCommit":generator,"acceptedCommit":if accepted{json!("c".repeat(40))}else{Value::Null},
            "accepted":accepted,"pairedMeasured":true,"pairedRevalidated":accepted,
            "evaluationProfileSha256":"d".repeat(64),
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":accepted,"verdict":"paired_measured",
                    "measurements":[{"id":"case-a","domain":"memory","holdout":true,
                        "baselinePass":before,"candidatePass":after}]}},
            "changedSourcePaths":[source],
            "acceptedSourceHashes":Value::Object(hashes),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":generator,"metaRevision":"m",
                "openImprovementRevision":"r"},
            "metaPolicy":{"revision":"m","selectionMode":"authored_open_improvement_program",
                "openImprovementRevision":"r",
                "target":{"role":target_role,"path":source}},
            "openImprovementProgram":{"source":"compiled_descendant_open_improvement_program_v1",
                "revision":"r","target":{"role":target_role,"path":source},
                "node":{"mechanism":mechanism}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":generator,"generationJobId":format!("job-{id}"),
                "components":[target_role],
                "componentFiles":Value::Object(assignments),
                "targetFiles":[source]}
        })
    }
    #[test]
    fn independent_task_gains_train_priority_while_failed_ideas_lose_rank(){
        let good=trial("good","executable retrieval repair","src/engine/memory.rs",
            "memory",false,true,true);
        let bad=trial("bad","repeated no-op search","src/engine/memory.rs",
            "memory",true,false,false);
        let ledger=study(&[good,bad]);
        assert!(valid(&ledger,&[] )==false);
        let yes=estimate(&ledger,&json!({"mechanism":"executable retrieval repair"}));
        let no=estimate(&ledger,&json!({"mechanism":"repeated no-op search"}));
        assert!(yes["estimatedPriority"].as_f64()>no["estimatedPriority"].as_f64());
        assert_eq!(ledger["principles"].as_array().unwrap().len(),2);
    }
    #[test]
    fn a_different_child_method_can_credit_cross_role_task_improvement(){
        let parent=trial("parent","adapt retrieval by observing independent task failures",
            "src/engine/memory.rs","memory",false,true,true);
        let mut child=trial("child","child uses a different improvement program",
            "src/engine/models.rs","models",false,true,true);
        child["generatorCommit"]=parent["acceptedCommit"].clone();
        child["acceptedCommit"]=json!("e".repeat(40));
        child["compiledPlanExecuted"]["generatorCommit"]=child["generatorCommit"].clone();
        child["systemDesign"]["generatorCommit"]=child["generatorCommit"].clone();
        child["systemDesign"]["components"]=json!(["models"]);
        child["openImprovementProgram"]=Value::Null;
        child["metaPolicy"]=json!({"revision":"m"});
        child["evaluation"]["metrics"]["measurements"][0]["domain"]=json!("learning");
        let history=vec![parent,child];
        let report=study(&history);
        let row=&report["principles"][0];
        assert_eq!(row["mechanism"],
            "adapt retrieval by observing independent task failures");
        assert_eq!(row["nextGenerationCases"],1);
        assert_eq!(row["nextGenerationImprovements"],1);
        assert_eq!(row["descendantRoles"][0],"models");
        let mut altered=history;
        altered[1]["pairedRevalidated"]=json!(false);
        altered[1]["pairedMeasured"]=json!(false);
        assert_eq!(study(&altered)["principles"][0]["nextGenerationImprovements"],0);
    }
    #[test]
    fn accepted_source_without_quality_gain_is_not_a_learned_positive_example(){
        let g=trial("neutral","source edit without task improvement",
            "src/engine/learning.rs","learning",false,false,true);
        let row=study(&[g])["principles"][0].clone();
        assert_eq!(row["improvedCases"],0);
        assert_eq!(row["independentSourceAdoptions"],1);
        assert_eq!(row["posteriorCaseGain"].as_f64(),Some(1.0/3.0));
    }
    #[test]
    fn legacy_model_authored_system_change_teaches_the_next_open_learner(){
        let mut g=trial("legacy","placeholder","src/engine/models.rs",
            "models",false,true,true);
        g["openImprovementProgram"]=Value::Null;
        g["metaPolicy"]=json!({"revision":"m"});
        g["systemDesign"]["hypothesis"]=json!(
            "Replace the model selection and context assembly strategy so the frozen downstream tasks improve");
        g["systemDesign"]["component"]=json!("models");
        let ledger=study(&[g.clone()]);
        assert_eq!(ledger["principles"].as_array().unwrap().len(),1);
        assert_eq!(ledger["principles"][0]["improvedCases"],1);
        g["systemDesign"]["generatorCommit"]=json!("forged-parent");
        assert_eq!(study(&[g])["principles"].as_array().unwrap().len(),0);
    }
    #[test]
    fn two_different_mechanisms_train_a_transferable_factor_for_a_novel_third(){
        let mut first=trial("first-key","repair memory index using task traces",
            "src/engine/memory.rs","memory",false,true,true);
        let mut second=trial("second-key","adapt model context based on source outcomes",
            "src/engine/models.rs","models",false,true,true);
        first["openImprovementProgram"]["node"]["principleKeys"]
            =json!(["evidence_guided_context"]);
        second["openImprovementProgram"]["node"]["principleKeys"]
            =json!(["evidence_guided_context"]);
        second["evaluation"]["metrics"]["measurements"][0]["domain"]
            =json!("learning");
        let bank=study(&[first,second]);
        let factor=&bank["transferFeatures"][0];
        assert_eq!(factor["independentMechanisms"],2);
        assert_eq!(factor["transferEligible"],true);
        assert_eq!(factor["domains"].as_array().unwrap().len(),2);
        let novel=json!({"mechanism":"new adaptive learning algorithm combining two subsystems",
            "principleKeys":["evidence_guided_context"]});
        let general=estimate(&bank,&novel);
        let no_key=estimate(&bank,&json!({"mechanism":"unseen arbitrary change"}));
        assert_eq!(general["learnedEvidenceSource"],
            "cross_mechanism_transferable_principle");
        assert_eq!(general["caseQualityPosterior"].as_f64(),Some(0.75));
        assert!(general["estimatedPriority"].as_f64().unwrap()
            >no_key["estimatedPriority"].as_f64().unwrap());
        assert_eq!(no_key["caseQualityPosterior"].as_f64(),Some(0.5));
    }
    #[test]
    fn bad_heldout_forecasts_trigger_a_source_rewrite_of_the_meta_learner(){
        let samples=(0..3).map(|i|{
            let mut g=trial(&format!("badprediction-{i}"),
                "overconfident domain-specific technique",
                "src/engine/models.rs","models",false,false,false);
            g["openImprovementProgram"]["principleSelectionEvidence"]=json!({
                "caseQualityPosterior":0.95});
            g["openImprovementProgram"]["principleLearningSha256"]
                =json!("f".repeat(64));
            g["principleLearning"]=json!({"sha256":"f".repeat(64)});
            g
        }).collect::<Vec<_>>();
        let ledger=study(&samples);
        assert_eq!(ledger["predictiveCalibration"]["measuredCases"],3);
        assert_eq!(ledger["predictiveCalibration"]["needsLearnerSourceRevision"],true);
        assert!(ledger["predictiveCalibration"]["meanBrierError"]
            .as_f64().is_some_and(|e|e>0.8));
        let mut forged=samples;
        forged[0]["pairedMeasured"]=json!(false);
        assert_eq!(study(&forged)["predictiveCalibration"]["measuredCases"],2);
        assert_eq!(study(&forged)["predictiveCalibration"]["needsLearnerSourceRevision"],false);
    }
    #[test]
    fn model_prose_without_independent_paired_evidence_never_trains_principle(){
        let mut g=trial("false","invented gain","src/engine/memory.rs",
            "memory",false,true,true);
        g["evaluation"]["source"]=json!("model_self_report");
        assert_eq!(study(&[g])["principles"].as_array().unwrap().len(),0);
    }
}
