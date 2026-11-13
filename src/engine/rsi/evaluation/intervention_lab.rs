//! Source-grounded, successor-editable, resource-aware factorial experiment
//! designer for an OPEN RSI topology (including entirely invented roles).
//! The immutable evaluator owns paired measurements and promotions.
//! No observational contrast is reported as randomized causal proof.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

type Block=(String,String,bool);
type Arms=[Vec<f64>;3];
fn digest(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn hash_valid(s:&str)->bool{
    s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit())
}
fn grounded(row:&Value)->bool{
    row["role"].as_str().is_some_and(crate::engine::rsi_open_topology::valid_role)
        && row["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && row["sourceSha256"].as_str().is_some_and(hash_valid)
}
/// A source-level redesign of the experimental METHOD may pass on an
/// executable learning policy. Merely writing JSON is not enough: the actual
/// experiment planner module has to be in the separately adopted patch.
pub(crate) fn inherited_method(history:&[Value])->Value{
    for row in history.iter().rev(){
        let path="src/engine/rsi/evaluation/intervention_lab.rs";
        if row["accepted"]!=true || row["pairedRevalidated"]!=true
            || row["candidateOrigin"]!="model_driven_detached_worktree"
            || row["evaluation"]["metrics"]["eligible"]!=true
            || row["systemDesign"]["source"]!="model_authored_system_design"
            || row["systemDesign"]["generatorCommit"]!=row["generatorCommit"]
            || row["systemDesign"]["generationJobId"]!=row["jobId"]
            || row["acceptedSourceHashes"][path].as_str().is_none_or(|h|hash_valid(h)){
            continue;
        }
        let mapped=row["systemDesign"]["componentFiles"]["improvement_methods"]
            .as_array().is_some_and(|files|files.iter().any(|f|f==path));
        if !mapped{continue;}
        let mut method=row["systemDesign"]["experimentalPolicy"].clone();
        // A newer source edit explicitly supersedes prior experimental
        // algorithms. Omitting policy means resetting to the code default.
        if !method.is_object(){return Value::Null;}
        if method["source"]!="model_authored_causal_experiment_policy_v1"{
            return Value::Null;
        }
        method["independentlyAdoptedMethodCommit"]=row["acceptedCommit"].clone();
        method["sourceModule"]=json!(path);
        method["trialQualityProven"]=json!(false);
        return method;
    }
    Value::Null
}
fn count_variant(g:&Value,a:&str,b:&str)->Option<usize>{
    if g["factorialMeasured"]!=true
        || g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["factorialCompliance"]["eligibleForFactorial"]!=true {
        return None;
    }
    let edited=g["changedSourcePaths"].as_array()
        .filter(|paths|!paths.is_empty()&&paths.len()<=24)?;
    let previous=g["descendantPlan"]["architectureFrontier"].as_array()?;
    let declared=&g["systemDesign"]["componentFiles"];
    let mut roles=BTreeSet::<String>::new();
    for entry in edited{
        let path=entry.as_str().filter(|p|crate::engine::rsi::safe_path(p))?;
        let mut matching=previous.iter().filter(|row|row["path"]==path)
            .filter_map(|row|row["role"].as_str())
            .filter(|role|*role==a||*role==b)
            .collect::<BTreeSet<_>>();
        // A newly invented module may be absent from the PRE-EDIT frontier.
        // It can count only when the source-bound model design assigns it
        // to exactly one tested role, and no unrelated file was changed.
        for role in [a,b]{
            if declared[role].as_array().is_some_and(|files|
                files.iter().any(|f|f.as_str()==Some(path))){
                matching.insert(role);
            }
        }
        if matching.len()!=1{return None;}
        roles.insert((*matching.iter().next()?).to_string());
    }
    if roles.len()==1 && roles.contains(a){Some(0)}
    else if roles.len()==1 && roles.contains(b){Some(1)}
    else if roles.len()==2 && roles.contains(a)&&roles.contains(b){Some(2)}
    else{None}
}
fn mean(v:&[f64])->f64{
    v.iter().sum::<f64>()/v.len().max(1) as f64
}

/// Only independently measured candidates from the identical generator SHA,
/// frozen-case profile, case id and baseline outcome form comparable blocks.
/// Rejected candidates are legitimate NEGATIVE experiments, not adoptions.
fn observation(history:&[Value],a:&str,b:&str,commit:&str,profile:&str)->Value{
    let mut counts=[0usize;3];
    let mut blocks=BTreeMap::<Block,Arms>::new();
    let mut provenance=BTreeSet::<String>::new();
    let mut measured_ids=BTreeSet::<String>::new();
    for g in history.iter().filter(|g|
        g["generatorCommit"]==commit
        && g["comparisonProfileSha256"]==profile
        && g["factorialMeasured"]==true) {
        let Some(arm)=count_variant(g,a,b) else{continue};
        let Some(id)=g["candidateId"].as_str() else{continue};
        if provenance.len()>=32{break;}
        if !provenance.insert(id.to_string()){continue;}
        let mut contributed=false;
        for case in g["evaluation"]["metrics"]["measurements"].as_array()
            .into_iter().flatten(){
            if case["holdout"]!=true {continue;}
            let (Some(name),Some(before),Some(after))=(
                case["id"].as_str().filter(|s|!s.is_empty()),
                case["baselinePass"].as_bool(),case["candidatePass"].as_bool()
            )else{continue};
            blocks.entry((name.to_string(),profile.to_string(),before))
                .or_insert_with(||[Vec::new(),Vec::new(),Vec::new()])
                [arm].push((after as i32-before as i32) as f64);
            contributed=true;
        }
        if contributed{
            counts[arm]+=1;
            measured_ids.insert(id.to_string());
        }
    }
    let mut matched=Vec::new();
    for ((name,_,baseline),arms) in blocks.iter(){
        if arms.iter().any(Vec::is_empty){continue;}
        let a=mean(&arms[0]);let b=mean(&arms[1]);let both=mean(&arms[2]);
        matched.push(json!({"holdout":name,"baselinePass":baseline,
            "aOnlyDelta":a,"bOnlyDelta":b,"jointDelta":both,
            "interaction":both-a-b,
            "replicates":arms.iter().map(Vec::len).collect::<Vec<_>>() }));
    }
    let average=(!matched.is_empty()).then(||mean(&matched.iter()
        .filter_map(|r|r["interaction"].as_f64()).collect::<Vec<_>>()));
    let a_effect=(!matched.is_empty()).then(||mean(&matched.iter()
        .filter_map(|r|r["aOnlyDelta"].as_f64()).collect::<Vec<_>>()));
    let b_effect=(!matched.is_empty()).then(||mean(&matched.iter()
        .filter_map(|r|r["bOnlyDelta"].as_f64()).collect::<Vec<_>>()));
    let rows=matched.len();
    let mut evidence=json!({
        "source":"independent_matched_frozen_case_factorial_evidence_v1",
        "generatorCommit":commit,"profileSha256":profile,
        "roles":[a,b],
        "observedArms":{"aOnly":counts[0],"bOnly":counts[1],"joint":counts[2]},
        "completeFrozenCaseBlocks":rows,
        "meanInteractionContrast":average,
        "meanAOnlyEffect":a_effect,"meanBOnlyEffect":b_effect,
        "interactionStandardErrorBound":if rows==0{Value::Null}
            else{json!(2.0/(rows as f64).sqrt())},
        "contrasts":matched.into_iter().take(16).collect::<Vec<_>>(),
        "observationalOnly":true,"causalityProven":false,
        "candidateCount":counts.iter().sum::<usize>(),
        "pairedCandidateIds":measured_ids.into_iter().take(32).collect::<Vec<_>>()
    });
    evidence["sha256"]=json!(digest(&evidence));
    evidence
}

/// Different source revisions must never be pooled as one factorial result.
/// Transfer older independently measured interactions only as discounted
/// suggestions for NEW experiments after a source rewrite.
fn prior_interaction_hints(history:&[Value],a:&Value,b:&Value)->Value{
    let (Some(left),Some(right))=(a["role"].as_str(),b["role"].as_str())
        else{return json!([])};
    let mut hints=Vec::new();
    for g in history.iter().rev(){
        let old=&g["factorialExperiment"]["selected"];
        let e=&old["evidence"];
        if g["factorialLearning"]["originalPairedReceiptReplayed"]!=true
            || e["completeFrozenCaseBlocks"].as_u64().unwrap_or(0)<2
            || e["observationalOnly"]!=true {continue;}
        let direct=old["a"]["role"]==left && old["b"]["role"]==right;
        let reverse=old["a"]["role"]==right && old["b"]["role"]==left;
        if !direct&&!reverse{continue;}
        let Some(effect)=e["meanInteractionContrast"].as_f64()
            .filter(|x|x.is_finite()&&x.abs()<=2.0) else{continue};
        let sources_same=if direct {
            old["a"]["sourceSha256"]==a["sourceSha256"]
                && old["b"]["sourceSha256"]==b["sourceSha256"]
        }else {
            old["a"]["sourceSha256"]==b["sourceSha256"]
                && old["b"]["sourceSha256"]==a["sourceSha256"]
        };
        hints.push(json!({
            "source":"discounted_prior_source_interaction_v1",
            "earlierGeneratorCommit":g["generatorCommit"],
            "earlierProfileSha256":g["comparisonProfileSha256"],
            "sourceSha256Unchanged":sources_same,
            "observedInteractionContrast":effect,
            "relevanceWeight":if sources_same{0.5}else{0.12},
            "qualityProvenForCurrentRevision":false,
            "causalityProven":false
        }));
        if hints.len()>=3{break;}
    }
    json!(hints)
}

/// A general pair search over an open descendant source graph. Missing
/// counterfactuals have information value, not fake reward. The compiled
/// descendant controls this design and can rewrite this file next generation.
pub(crate) fn design(history:&[Value],frontier:&[Value],meta:&Value,
    symbiosis:&Value,allowed:&[&str],commit:&str,profile:Option<&str>,
    inherited_method:&Value)->Value{
    let profile=profile.filter(|p|hash_valid(p)).unwrap_or("");
    // No frozen profile means no independently comparable intervention.
    if profile.is_empty(){return Value::Null;}
    let primary=&meta["target"];
    let mut targets=BTreeMap::<String,Value>::new();
    for row in frontier.iter().filter(|row|grounded(row)
        && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))){
        let name=row["role"].as_str().unwrap_or("");
        targets.entry(name.to_string()).or_insert_with(||row.clone());
    }
    let mut pairs=Vec::<(Value,Value)>::new();
    if grounded(primary) && targets.contains_key(primary["role"].as_str().unwrap_or("")){
        let role=primary["role"].as_str().unwrap_or("");
        if let Some(partner)=symbiosis["complement"]["toRole"].as_str()
            .and_then(|r|targets.get(r)) {
            if role!=partner["role"]{pairs.push((primary.clone(),partner.clone()));}
        }
        for row in targets.values().filter(|row|row["role"]!=role){
            if !pairs.iter().any(|(_,b)|b["role"]==row["role"]){
                pairs.push((primary.clone(),row.clone()));
            }
        }
    }
    let mut ranked=Vec::<(f64,Value)>::new();
    let mode=inherited_method["mode"].as_str().unwrap_or("coverage_first");
    let exploration=inherited_method["explorationWeight"].as_f64()
        .filter(|v|v.is_finite()&&*v>=0.0&&*v<=3.0).unwrap_or(1.0);
    let interaction_priority=inherited_method["interactionWeight"].as_f64()
        .filter(|v|v.is_finite()&&*v>=0.0&&*v<=3.0).unwrap_or(1.0);
    let max_pairs=inherited_method["maxPairs"].as_u64()
        .filter(|v|(2..=40).contains(v)).unwrap_or(40) as usize;
    for (a,b) in pairs.into_iter().take(max_pairs){
        let roles=(a["role"].as_str().unwrap_or(""),
            b["role"].as_str().unwrap_or(""));
        let evidence=observation(history,roles.0,roles.1,commit,profile);
        let counts=&evidence["observedArms"];
        let trial=[counts["aOnly"].as_u64().unwrap_or(0),
            counts["bOnly"].as_u64().unwrap_or(0),
            counts["joint"].as_u64().unwrap_or(0)];
        // The most informative missing arm is selected automatically.
        // Joint code changes are attempted only after both isolated edits
        // have actual independently measured baselines.
        let missing=trial.iter().position(|n|*n==0);
        let measured_cases=evidence["completeFrozenCaseBlocks"].as_u64()
            .unwrap_or(0);
        let interaction=evidence["meanInteractionContrast"].as_f64().unwrap_or(0.0);
        let a_gain=evidence["meanAOnlyEffect"].as_f64().unwrap_or(0.0);
        let b_gain=evidence["meanBOnlyEffect"].as_f64().unwrap_or(0.0);
        let trustworthy=measured_cases>=3 && trial.iter().all(|n|*n>0);
        let arm=missing.unwrap_or_else(||{
            if trustworthy {
                if interaction>0.15 {2}
                else if a_gain>=b_gain {0} else{1}
            }else{
                trial.iter().enumerate().min_by_key(|(_,v)|*v)
                    .map(|(i,_)|i).unwrap_or(0)
            }
        });
        let phase=["isolate_a","isolate_b","test_joint"][arm];
        let target=if arm==1{b.clone()}else{a.clone()};
        let informative=trial.iter().filter(|n|**n>0).count() as f64;
        let balanced=1.0/(1.0+trial[arm] as f64);
        let coverage=evidence["completeFrozenCaseBlocks"].as_u64().unwrap_or(0);
        let hints=prior_interaction_hints(history,&a,&b);
        let transferred=hints.as_array().into_iter().flatten()
            .filter_map(|row|Some(
                row["observedInteractionContrast"].as_f64()?
                *row["relevanceWeight"].as_f64()?))
            .map(f64::abs).fold(0.0f64,f64::max).min(0.5);
        let confidence=coverage as f64/(coverage as f64+4.0);
        let synergy=confidence*interaction.abs().min(1.0);
        let score=0.7*informative+0.45*balanced*exploration
            +0.20*(coverage.min(8) as f64/8.0)
            +0.35*synergy*interaction_priority
            +0.16*transferred
            +if mode=="counterexample_repair" && interaction<0.0 {
                0.24
            }else if mode=="synergy_search" && interaction>0.0{
                0.24
            }else{0.0}
            +if roles.0=="improvement_methods"||roles.1=="improvement_methods"{
                0.15
            }else{0.0};
        ranked.push((score,json!({
            "a":a,"b":b,"target":target,"armIndex":arm,
            "phase":phase,"sourceIsolationRequired":arm!=2,
            "activeRoles":if arm==2{json!([roles.0,roles.1])}
                else if arm==0{json!([roles.0])}else{json!([roles.1])},
            "evidence":evidence,
            "expectedInformationGainPriority":score,
            "observedInteractionWeight":confidence,
            "crossRevisionHypotheses":hints,
            "hypothesis":"joint_delta_minus_isolated_effects",
            "qualityProven":false,"causalityProven":false
        })));
    }
    ranked.sort_by(|(a,l),(b,r)|b.total_cmp(a)
        .then_with(||l["a"]["role"].as_str().cmp(&r["a"]["role"].as_str()))
        .then_with(||l["b"]["role"].as_str().cmp(&r["b"]["role"].as_str())));
    let selected=ranked.first().map(|(_,row)|row.clone()).unwrap_or(Value::Null);
    let mut result=json!({
        "source":"compiled_open_topology_counterfactual_lab_v1",
        "generatorCommit":commit,"comparisonProfileSha256":profile,
        "historyCount":history.len(),
        "selected":selected,
        "inheritedExperimentalMethod":inherited_method,
        "pairAlternatives":ranked.into_iter().take(6)
            .map(|(_,v)|v).collect::<Vec<_>>(),
        "independentEvaluatorOnly":true,
        "claim":"matched_observational_factorial_not_randomized_causality",
        "qualityProven":false,
        "modelMayEvolveExperimentalMethod":true
    });
    result["sha256"]=json!(digest(&result));
    result
}

/// The non-editable supervisor verifies source identity and the supplied
/// decision's receipt without rerunning an old copy of the child's planner.
pub(crate) fn validate(plan:&Value,frontier:&[Value],commit:&str,
    history_count:usize,profile:Option<&str>)->Result<(),String>{
    if plan.is_null(){return Ok(());}
    let mut clean=plan.clone();
    let map=clean.as_object_mut().ok_or("요인별 실험 계획의 형식 오류")?;
    map.remove("sha256");
    let checksum=digest(&clean);
    if plan["source"]!="compiled_open_topology_counterfactual_lab_v1"
        || plan["sha256"].as_str()!=Some(checksum.as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history_count as u64)
        || plan["comparisonProfileSha256"]!=profile.unwrap_or("")
        || plan["independentEvaluatorOnly"]!=true
        || plan["qualityProven"]!=false
        || plan["pairAlternatives"].as_array().is_none_or(|rows|rows.len()>6){
        return Err("후손 실험 정책의 소스·평가 조건이 올바르지 않습니다.".into());
    }
    let check=|trial:&Value|{
        let a=&trial["a"];let b=&trial["b"];
        let target=&trial["target"];
        let arm=trial["armIndex"].as_u64();
        grounded(a)&&grounded(b)&&a["role"]!=b["role"]
            && frontier.iter().any(|r|r==a) && frontier.iter().any(|r|r==b)
            && matches!(arm,Some(0|1|2))
            && target==(if arm==Some(1){b}else{a})
            && trial["sourceIsolationRequired"]==(arm!=Some(2))
            && trial["qualityProven"]==false && trial["causalityProven"]==false
            && trial["evidence"]["source"]=="independent_matched_frozen_case_factorial_evidence_v1"
            && trial["evidence"]["generatorCommit"]==commit
            && trial["evidence"]["profileSha256"]==profile.unwrap_or("")
            && {
                let mut evidence=trial["evidence"].clone();
                let ev=evidence.as_object_mut();
                if let Some(ev)=ev{ev.remove("sha256");}
                trial["evidence"]["sha256"]==digest(&evidence)
            }
    };
    if !plan["pairAlternatives"].as_array().is_some_and(|rows|
        rows.iter().all(|row|check(row))
        && (plan["selected"].is_null()
            || (check(&plan["selected"])&&rows.contains(&plan["selected"])))) {
        return Err("후손의 요인별 실험 대상이 실제 소스 구조와 다릅니다.".into());
    }
    Ok(())
}

/// A frozen supervisor checks the *original* paired-worktree measurement
/// receipts. It must not recalculate the descendant's editable experiment
/// selection formula, but can reject fabricated or cross-revision evidence.
pub(crate) fn validate_receipts(
    state:&crate::state::AppState,history:&[Value],plan:&Value
)->Result<(),String>{
    if plan.is_null(){return Ok(());}
    let Some(pairs)=plan["pairAlternatives"].as_array()
        .filter(|v|v.len()<=6) else{
        return Err("요인 실험 후보의 원본 평가 출처가 없습니다.".into());
    };
    let commit=plan["generatorCommit"].as_str()
        .ok_or("실험 원본 생성기 SHA 없음")?;
    let profile=plan["comparisonProfileSha256"].as_str()
        .ok_or("동결 평가 사례 SHA 없음")?;
    for pair in pairs {
        let evidence=&pair["evidence"];
        let ids=evidence["pairedCandidateIds"].as_array()
            .filter(|ids|ids.len()<=32)
            .ok_or("요인 실험은 원본 측정 후보의 ID를 보존해야 합니다.")?;
        if evidence["candidateCount"].as_u64()!=Some(ids.len() as u64) {
            return Err("요인 실험의 원본 측정 횟수와 후보 목록이 다릅니다.".into());
        }
        let mut seen=BTreeSet::new();
        for id in ids {
            let id=id.as_str().filter(|s|!s.is_empty())
                .ok_or("요인 실험 후보의 ID가 유효하지 않습니다.")?;
            if !seen.insert(id){
                return Err("중복된 요인 실험 후보를 계산했습니다.".into());
            }
            let row=history.iter().find(|g|g["candidateId"]==id
                && g["generatorCommit"]==commit
                && g["comparisonProfileSha256"]==profile
                && g["factorialMeasured"]==true
                && g["factorialCompliance"]["eligibleForFactorial"]==true)
                .ok_or("요인 실험 후보의 동결 평가 조건이 다릅니다.")?;
            let candidate=state.store.get("rsi_candidate",id)
                .ok_or("실험 원본 후보가 저장되어 있지 않습니다.")?;
            let replayed=crate::engine::rsi::replayed_pair_metrics(state,&candidate)
                .ok_or("원본 짝 비교 평가 영수증을 독립 재계산하지 못했습니다.")?;
            if candidate["baseCommit"]!=commit
                || row["changedSourcePaths"]!=candidate["paths"]
                || row["evaluation"]["metrics"]!=replayed
                || row["pairedMeasured"]!=true {
                return Err("실험의 변경 소스와 독립 평가 원본이 일치하지 않습니다.".into());
            }
        }
    }
    Ok(())
}

/// The active edit policy does not claim successful isolation; changed
/// source paths are compared AFTER execution by the independent coordinator.
/// Model-authored source edits remain its decision; the compiled descendant
/// promotes information-gain experiments into its live meta-target choice.
pub(crate) fn steer(mut meta:Value,plan:&Value)->Value{
    let selected=&plan["selected"];
    if !selected.is_object(){return meta;}
    let target=&selected["target"];
    if !grounded(target){return meta;}
    let chosen=meta["alternatives"].as_array().into_iter().flatten()
        .find(|item|item["target"]==*target).cloned();
    let previous=meta["revision"].clone();
    meta["target"]=target.clone();
    if let Some(winner)=chosen {
        meta["selectionEvidence"]=winner.clone();
        meta["forecast"]["acceptanceProbability"]=winner["predictedAdoptionProbability"].clone();
        meta["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    }
    meta["mode"]=json!("independently_matched_factorial_experiment_design");
    meta["factorialExperimentSha256"]=plan["sha256"].clone();
    meta["revision"]=json!(digest(&json!([
        previous,plan["sha256"],target,selected["phase"]
    ])));
    meta
}

pub(crate) fn compliance(plan:&Value,changed:&[Value])->Value{
    let row=&plan["selected"];
    let actual=changed.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let a=row["a"]["path"].as_str().unwrap_or("");
    let b=row["b"]["path"].as_str().unwrap_or("");
    let phase=row["phase"].as_str().unwrap_or("");
    let success=match phase {
        "isolate_a"=>!actual.is_empty()&&actual.iter().all(|p|*p==a),
        "isolate_b"=>!actual.is_empty()&&actual.iter().all(|p|*p==b),
        "test_joint"=>actual.contains(a)&&actual.contains(b)
            &&actual.iter().all(|p|*p==a||*p==b),
        _=>false
    };
    json!({"source":"observed_changed_source_factorial_classification_v1",
        "phase":phase,"eligibleForFactorial":success,
        "changedSourcePaths":changed,
        "candidateOnly":true,"qualityProven":false})
}
pub(crate) fn instruction(plan:&Value)->String{
    let s=&plan["selected"];
    if !s.is_object(){return String::new();}
    let phase=s["phase"].as_str().unwrap_or("");
    let directive=match phase{
        "isolate_a"|"isolate_b"=>
            "Edit ONLY selected target file in this trial. Companion is a read-only counterfactual reference. If other files are changed, the experiment is not an isolated observation.",
        "test_joint"=>
            "Edit BOTH the A and B source files. The independent experiment ledger will inspect actual changed files rather than your claimed roles.",
        _=>"Inspect actual source and design an independently testable modification."
    };
    format!("[SELF-EVOLVING COUNTERFACTUAL LAB] phase={phase} score={}. \
        A={} at {}, B={} at {}. Matching frozen observations={}; \
        exact original generator={} and frozen profile={}. {} \
        The compiled next-generation planner can rewrite this lab. \
        No results are causal proof without matched independent evaluation.",
        s["expectedInformationGainPriority"],s["a"]["role"],s["a"]["path"],
        s["b"]["role"],s["b"]["path"],
        s["evidence"]["completeFrozenCaseBlocks"],
        plan["generatorCommit"],plan["comparisonProfileSha256"],directive)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn dynamic_roles_can_generate_isolated_and_joint_experiment_steps(){
        let a=json!({"role":"world_model_planner","domain":"learning",
            "path":"src/engine/world.rs","sourceSha256":"a".repeat(64)});
        let b=json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_meta_controller.rs","sourceSha256":"b".repeat(64)});
        let x=design(&[],&[a.clone(),b.clone()],&json!({"target":a}),
            &Value::Null,&["learning","improver"],&"f".repeat(40),
            Some(&"1".repeat(64)),&Value::Null);
        assert_eq!(x["selected"]["phase"],"isolate_a");
        assert_eq!(x["selected"]["b"]["role"],"improvement_methods");
        assert_eq!(compliance(&x,&[json!("src/engine/world.rs")])["eligibleForFactorial"],true);
        assert_eq!(compliance(&x,&[json!("src/engine/world.rs"),
            json!("src/engine/rsi_meta_controller.rs")])["eligibleForFactorial"],false);
    }
    #[test]
    fn factorial_sha_attestation_requires_exact_source_and_profile(){
        let a=json!({"role":"algorithms","domain":"improver",
            "path":"src/engine/algorithms.rs","sourceSha256":"a".repeat(64)});
        let b=json!({"role":"tools","domain":"tools",
            "path":"src/engine/tools.rs","sourceSha256":"b".repeat(64)});
        let commit="f".repeat(40);let profile="1".repeat(64);
        let plan=design(&[],&[a.clone(),b.clone()],&json!({"target":a.clone()}),
            &Value::Null,&["improver","tools"],&commit,Some(&profile),&Value::Null);
        assert!(validate(&plan,&[a.clone(),b.clone()],&commit,0,Some(&profile)).is_ok());
        let mut corrupted=plan.clone();
        corrupted["selected"]["a"]["sourceSha256"]=json!("c".repeat(64));
        assert!(validate(&corrupted,&[a,b],&commit,0,Some(&profile)).is_err());
        assert!(validate(&plan,&[],&commit,0,Some(&profile)).is_err());
    }
    #[test]
    fn matched_evaluation_cannot_mix_different_generator_commits(){
        let a="algorithms";let b="tools";let profile="a".repeat(64);
        let row=json!({"id":"trial-1","candidateId":"c1","generatorCommit":"b".repeat(40),
            "comparisonProfileSha256":profile,"factorialMeasured":true,
            "factorialCompliance":{"eligibleForFactorial":true},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "changedSourcePaths":["src/engine/algorithms.rs"],
            "descendantPlan":{"architectureFrontier":[{"role":a,"path":"src/engine/algorithms.rs"}]},
            "evaluation":{"metrics":{"measurements":[{"id":"h1","holdout":true,
                "baselinePass":false,"candidatePass":true}]}}});
        let mismatched=observation(&[row],a,b,&"c".repeat(40),&profile);
        assert_eq!(mismatched["observedArms"]["aOnly"],0);
    }
}
