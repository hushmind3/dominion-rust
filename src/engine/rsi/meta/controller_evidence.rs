//! Source-replayable meta-controller evidence and ranking features.
use super::*;

pub(super) fn planned_role(g:&Value)->Option<&str> {
    g["metaPolicy"]["target"]["role"].as_str()
        .or_else(||g["architectureTarget"]["role"].as_str())
}
pub(super) fn independent_outcome(g:&Value)->Option<bool> {
    // Two distinct facts: an experiment can have a replayable independent
    // measurement even if its patch was rejected, regressed or never adopted.
    // Successful offspring require an actual separate adoption receipt.
    // Source-writing method (native / local / API) is not a success criterion.
    if g["pairedMeasured"]!=true && g["pairedRevalidated"]!=true {return None;}
    if g["evaluation"]["metrics"]["eligible"].as_bool().is_none()
        || g["evaluation"]["metrics"]["verdict"].as_str().is_none() {
        return None;
    }
    let accepted=g["accepted"]==true
        && g["pairedRevalidated"]==true
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && g["acceptedSourceHashes"].as_object()
            .is_some_and(|hashes|!hashes.is_empty() && hashes.values().all(|hash|
                hash.as_str().is_some_and(|sha|sha.len()==64
                    && sha.bytes().all(|b|b.is_ascii_hexdigit()))));
    Some(accepted)
}

pub(super) fn subsequent_successes(history:&[Value],parent:&Value)->usize {
    if independent_outcome(parent)!=Some(true){return 0;}
    let Some(head)=parent["acceptedCommit"].as_str() else{return 0};
    history.iter().filter(|next|
        next["generatorCommit"].as_str()==Some(head)
            && next["controllerPlan"]["source"]=="compiled_descendant_system_controller_v2"
            && independent_outcome(next)==Some(true)).count()
}
pub(super) fn recent_calibration(history:&[Value])->f64 {
    let errors=history.iter().rev().filter_map(|g|
        g["metaLearning"]["brierError"].as_f64()
            .filter(|x|x.is_finite() && (0.0..=1.0).contains(x))
    ).take(16).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.65*mean).clamp(0.35,1.0)
}
pub(super) fn failure_class(g:&Value)->&'static str {
    if independent_outcome(g)==Some(true){"verified_gain"}
    else if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|v|v.contains("compile")||v.contains("rustc")) {
        "compiler_repair"
    }else if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0 {
        "measured_regression"
    }else if g["candidateId"].is_null() {"no_source_candidate"}
    else if independent_outcome(g)==Some(false) {"measured_no_gain"}
    else {"unmeasured_observation"}
}
/// Evidence about changes that generalize beyond the domain they targeted.
/// Count only frozen, paired, held-out cases attributed to ONE actually changed
/// source role and this exact file. Joint interventions remain confounded.
/// This affects the editable descendant's SEARCH, never the adoption oracle.
pub(super) fn generalization_evidence(
    history:&[Value],frontier:&[Value],role:&str,path:&str,target_domain:&str
)->Value {
    fn family(domain:&str)->&str {
        match domain {
            "recall"|"memory"=>"memory",
            "context"|"planning"|"plan"|"harness"=>"harness",
            "search"|"improver"|"algorithm"=>"improver",
            "training"|"learning"=>"learning",
            "tool-use"|"tools"=>"tools",
            "model"|"models"=>"models",
            "ui"|"frontend"=>"frontend",
            other=>other
        }
    }
    let mut domains=std::collections::BTreeMap::<String,(u64,u64,u64)>::new();
    for generation in history {
        if !crate::engine::rsi_principle_learning::independent_trial(generation)
            || generation["candidateId"].as_str().is_none()
            || !generation["changedSourcePaths"].as_array().is_some_and(|paths|
                paths.iter().any(|candidate|candidate.as_str()==Some(path))) {
            continue;
        }
        let actual=crate::engine::rsi_interaction::actual_roles(generation,frontier);
        if actual.len()!=1 || actual[0]!=role {continue;}
        for case in generation["evaluation"]["metrics"]["measurements"]
            .as_array().into_iter().flatten() {
            if case["holdout"]!=true {continue;}
            let Some(domain)=case["domain"].as_str() else{continue};
            if family(domain)==family(target_domain) {continue;}
            let (Some(before),Some(after))=
                (case["baselinePass"].as_bool(),case["candidatePass"].as_bool())
                else{continue};
            let mut improved=!before && after;
            let mut regressed=before && !after;
            for metric in case["metrics"].as_array().into_iter().flatten() {
                improved |= metric["improved"]==true;
                regressed |= metric["regressed"]==true;
            }
            // An explicitly measured regression dominates other gains in
            // the same case; it must not be diluted into an apparent win.
            if domains.len()>=16 && !domains.contains_key(family(domain)){continue;}
            let counts=domains.entry(family(domain).to_owned()).or_default();
            if regressed {counts.1+=1;}
            else if improved {counts.0+=1;}
            else {counts.2+=1;}
        }
    }
    let (mut wins,mut losses,mut neutral)=(0_u64,0_u64,0_u64);
    let cases=domains.into_iter().map(|(domain,(gains,regressions,stable))|{
        wins+=gains;losses+=regressions;neutral+=stable;
        json!({"domain":domain,"gains":gains,"regressions":regressions,
            "neutral":stable,"observations":gains+regressions+stable})
    }).collect::<Vec<_>>();
    let n=wins+losses+neutral;
    // Shrink sparse observations toward zero; losing a separate capability
    // carries more cost than a within-domain gain. No unmeasured optimism.
    let adjustment=if n==0 {0.0} else {
        (0.20*wins as f64-0.44*losses as f64)/(n as f64+4.0)
    };
    json!({"source":"isolated_paired_cross_domain_holdout_v1",
        "sourcePath":path,"sourceRole":role,"targetDomain":family(target_domain),
        "observedCases":n,"observedGains":wins,
        "observedRegressions":losses,"observedNeutral":neutral,
        "testedDomains":cases,"selectionAdjustment":adjustment,
        "causalityProven":false,"adoptionAuthority":"independent_supervisor"})
}

/// Separate prequential target: a source patch can be accepted without
/// improving a held-out TASK. Measure those events independently of adoption.
/// The compiled source and frozen pair are evaluated before they can teach the
/// next generation; no model-reported outcome is accepted as training data.
pub(super) fn heldout_quality(g:&Value)->Vec<(bool,bool)> {
    if !crate::engine::rsi_principle_learning::independent_trial(g)
        || g["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || g["candidateId"].as_str().is_none()
        || g["generatorCommit"].as_str().is_none_or(|sha|sha.len()!=40)
        || g["evaluationProfileSha256"].as_str().is_none_or(|sha|sha.len()!=64)
        || g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["compiledPlanExecuted"]["generatorCommit"]!=g["generatorCommit"] {
        return Vec::new();
    }
    let mut seen=BTreeSet::new();
    let mut observations=Vec::new();
    for case in g["evaluation"]["metrics"]["measurements"].as_array().into_iter().flatten() {
        if case["holdout"]!=true {continue;}
        let (Some(id),Some(domain))=(case["id"].as_str(),case["domain"].as_str())
            else{continue};
        if !seen.insert((id.to_string(),domain.to_string())) {continue;}
        let (Some(before),Some(after))=
            (case["baselinePass"].as_bool(),case["candidatePass"].as_bool())
            else{continue};
        let mut improved=!before&&after;
        let mut regressed=before&&!after;
        for metric in case["metrics"].as_array().into_iter().flatten() {
            improved|=metric["improved"]==true;
            regressed|=metric["regressed"]==true;
        }
        observations.push((improved&&!regressed,regressed));
    }
    observations
}
pub(super) fn task_quality_prior(history:&[Value],role:&str)->Value {
    let mut candidates=BTreeSet::new();
    let (mut n,mut gain,mut loss)=(0_u64,0_u64,0_u64);
    for generation in history.iter().filter(|g|planned_role(g)==Some(role)) {
        let Some(candidate)=generation["candidateId"].as_str() else{continue};
        let key=(generation["evaluationProfileSha256"].as_str().unwrap_or("").to_owned(),
            candidate.to_string());
        if !candidates.insert(key){continue;}
        for (better,worse) in heldout_quality(generation) {
            n+=1;gain+=u64::from(better);loss+=u64::from(worse);
        }
    }
    json!({"source":"independently_paired_heldout_task_quality_prior_v1",
        "observedCases":n,"observedGains":gain,"observedRegressions":loss,
        "probability":(gain as f64+1.0)/(n as f64+2.0),
        "notAcceptanceProbability":true})
}
/// Compute prequential forecast error from the OLD policy sealed before
/// editing each candidate. A high error means "rewrite this predictor" is
/// an exploration target, not evidence that the revision is better.
pub(super) fn quality_forecast_calibration(history:&[Value])->Value {
    let mut seen=BTreeSet::new();
    let mut errors=Vec::<f64>::new();
    let mut regressions=0_u64;
    let mut measured_candidates=0_u64;
    for generation in history.iter().rev() {
        let Some(id)=generation["candidateId"].as_str() else{continue};
        let key=(generation["evaluationProfileSha256"].as_str().unwrap_or("").to_owned(),
            id.to_owned());
        if !seen.insert(key) {continue;}
        let forecast=&generation["metaPolicy"]["forecast"];
        let Some(p)=forecast["taskCaseGainProbability"].as_f64()
            .filter(|v|v.is_finite()&&(0.0..=1.0).contains(v)) else{continue};
        if generation["metaPolicy"]["revision"].as_str()
            .is_none_or(|sha|sha.len()!=64)
            || generation["compiledPlanExecuted"]["metaRevision"]!=generation["metaPolicy"]["revision"] {
            continue;
        }
        let cases=heldout_quality(generation);
        if !cases.is_empty(){measured_candidates+=1;}
        for (better,worse) in cases {
            errors.push((p-f64::from(u8::from(better))).powi(2));
            regressions+=u64::from(worse);
        }
        // The repair trigger needs several independently measured candidate
        // decisions, not four correlated cases from one candidate.
        if measured_candidates>=8 {break;}
    }
    let n=errors.len();
    let mean=if n>0 {errors.iter().sum::<f64>()/n as f64}else{0.0};
    // Four independent case outcomes in the last observed window trigger
    // hypothesis generation, never self-acceptance or oracle edits.
    json!({"source":"prequential_sealed_task_gain_calibration_v1",
        "observations":n,"measuredCandidates":measured_candidates,
        "meanBrierError":mean,"observedRegressions":regressions,
        "needsPredictorSourceRevision":measured_candidates>=4
            && n>=4 && mean>0.35,
        "editableSource":"src/engine/rsi/meta/controller_selection.rs",
        "qualityImproved":false,"independentMetaABProven":false})
}

