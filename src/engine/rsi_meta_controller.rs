//! Editable metacognitive controller compiled into each accepted descendant.
//! Inspired by HyperAgents, predictive improvement dynamics, and comparative
//! cross-trajectory evolution. It proposes search experiments; frozen independent
//! evaluation is the sole authority for adoption. Every forecast is calibrated
//! against the *next* observed, separately verified outcome.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn model_design_roles(g:&Value)->Vec<String>{
    let design=&g["systemDesign"];
    if design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=g["generatorCommit"]
        || design["generationJobId"]!=g["jobId"] {return Vec::new();}
    let names=if design["component"]=="system"{
        design["components"].as_array().cloned().unwrap_or_default()
    }else{vec![design["component"].clone()]};
    names.iter().filter_map(Value::as_str)
        .filter(|name|!name.is_empty() && name.len()<=64
            && name.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-')
            && *name!="system")
        .map(str::to_string).collect()
}
fn independent_adoption(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && !model_design_roles(g).is_empty()
        && g["systemDesign"]["targetFiles"].as_array()
            .is_some_and(|files|!files.is_empty() && files.iter().all(|path|
                path.as_str().is_some_and(|p|
                    g["acceptedSourceHashes"][p].as_str().is_some_and(|sha|
                        sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
}

/// Direct evidence that the accepted child rewrote the implementation of
/// improvement itself, not just a task module or a claimed meta design.
pub(crate) fn meta_source_changed(g:&Value)->bool{
    if !independent_adoption(g){return false;}
    let Some(files)=g["acceptedSourceHashes"].as_object() else{return false};
    files.keys().any(|path|matches!(path.as_str(),
        "src/engine/rsi_meta_controller.rs"|"src/engine/rsi_system.rs"
        |"src/engine/rsi_evolve.rs"|"src/engine/rsi_mutation.rs"
        |"src/engine/procedure.rs"|"src/engine/learning.rs"))
}

fn role_touched(g:&Value,role:&str)->bool{
    model_design_roles(g).iter().any(|name|name==role)
        || g["selfControllerVerified"]==true
            && g["systemAgenda"]["target"]["role"]==role
}
fn failure_kind(g:&Value)->&'static str{
    if g["accepted"]==true{return "adopted";}
    if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|error|
            error.contains("compile")||error.contains("compiler")) {
        return "compiler_failure";
    }
    if g["candidateId"].is_null() {
        return "no_verified_code_candidate";
    }
    if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0{
        return "behavior_regression";
    }
    if g["evaluation"]["verdict"].is_null(){return "evaluation_unavailable";}
    "no_independent_gain"
}
fn next_generation_productivity(history:&[Value],generation:&Value)->usize{
    if !independent_adoption(generation){return 0;}
    let Some(child_commit)=generation["acceptedCommit"].as_str() else{return 0};
    history.iter().filter(|child|
        child["generatorCommit"]==child_commit
            && child["selfControllerVerified"]==true
            && independent_adoption(child)
    ).count()
}

/// Learn an explicit categorical distribution over causal failure modes.
/// A scalar benchmark score cannot tell an agent whether to improve its
/// hypothesis, compiler repair procedure, code writer or predictor.
/// Dirichlet smoothing avoids certainty from one sparse experiment.
const OUTCOME_CLASSES:[&str;5]=[
    "adopted","compiler_failure","no_verified_code_candidate",
    "behavior_regression","no_independent_gain"
];
fn observed_class(g:&Value)->Option<&'static str>{
    if independent_adoption(g){return Some("adopted");}
    if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|e|
            e.contains("compiler")||e.contains("compile")){
        return Some("compiler_failure");
    }
    if g["candidateId"].is_null(){
        if g["result"].is_object()||g["attemptedCandidateId"].is_string()
            || g["error"].is_string(){return Some("no_verified_code_candidate");}
        return None;
    }
    if g["evaluation"]["metrics"]["regressions"]
        .as_u64().is_some_and(|count|count>0){
        return Some("behavior_regression");
    }
    if g["evaluation"]["verdict"].as_str().is_some(){
        return Some("no_independent_gain");
    }
    None
}
fn failure_distribution(rows:&[&Value])->Value{
    let mut counts=[0u64;5];
    for row in rows{
        if let Some(class)=observed_class(row){
            if let Some(index)=OUTCOME_CLASSES.iter().position(|item|*item==class){
                counts[index]+=1;
            }
        }
    }
    let total=counts.iter().sum::<u64>();
    let mut posterior=serde_json::Map::new();
    // One pseudocount per category: no zero-probability failure mode.
    for (index,class) in OUTCOME_CLASSES.iter().enumerate(){
        posterior.insert((*class).into(),
            json!((counts[index] as f64+1.0)/(total as f64+5.0)));
    }
    Value::Object(posterior)
}

fn confidence_adjustment(history:&[Value])->f64{
    let errors=history.iter().filter_map(|row|
        row["metaLearning"]["categoricalBrier"].as_f64()
            .or_else(||row["metaLearning"]["brierError"].as_f64())
            .filter(|value|value.is_finite() && *value>=0.0 && *value<=1.0))
        .rev().take(12).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.6*mean).clamp(0.4,1.0)
}


/// Frozen paired holdouts provide *observed* direction-of-effect data.
/// A multi-module patch cannot establish which module caused an improvement.
/// The next compiled descendant owns this editable causal-hypothesis model.
fn measured_interaction_edges(history:&[Value])
    ->BTreeMap<(String,String),(u64,u64,u64,u64)>{
    let mut edges=BTreeMap::<(String,String),(u64,u64,u64,u64)>::new();
    for generation in history.iter().filter(|g|independent_adoption(g)){
        let roles=model_design_roles(generation);
        let targets=&generation["systemDesign"]["targetFiles"];
        let implemented=roles.into_iter().filter(|role|{
            let mapped=generation["systemDesign"]["componentFiles"][role.as_str()]
                .as_array().filter(|rows|!rows.is_empty())
                .or_else(||(generation["systemDesign"]["component"]==role.as_str()
                    && generation["systemDesign"]["componentFiles"].is_null())
                    .then(||targets.as_array()).flatten());
            mapped.is_some_and(|files|files.iter().all(|file|{
                let Some(path)=file.as_str() else{return false};
                targets.as_array().is_some_and(|all|all.iter().any(|r|r==path))
                && generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|sha|sha.len()==64
                        && sha.bytes().all(|b|b.is_ascii_hexdigit()))
            }))
        }).collect::<Vec<_>>();
        if implemented.is_empty(){continue;}
        for case in generation["evaluation"]["metrics"]["holdoutEvidence"]
            .as_array().into_iter().flatten(){
            let Some(domain)=case["domain"].as_str() else{continue};
            let target_role=match domain{
                "context"|"planning"=>"harness",
                "improver"|"search"=>"improvement_methods",
                "ui"=>"frontend",
                "model"=>"models",
                "memory"|"tools"|"learning"=>domain,
                _=>continue
            };
            let (Some(before),Some(after))=(case["baselinePass"].as_bool(),
                case["candidatePass"].as_bool()) else{continue};
            let loss=before&&!after
                ||case["metricRegressions"].as_u64().unwrap_or(0)>0;
            let gain=!before&&after
                ||case["metricGains"].as_u64().unwrap_or(0)>0;
            for source in &implemented{
                if source==target_role || !gain&&!loss {continue;}
                let evidence=edges.entry((source.clone(),target_role.to_string()))
                    .or_default();
                let joint=implemented.len()>1;
                if loss {
                    if joint{evidence.3+=1}else{evidence.1+=1;}
                } else if joint {evidence.2+=1;} else {evidence.0+=1;}
            }
        }
    }
    edges
}

/// Construct a *two-way* source-edit experiment from independently adopted
/// parent -> rebuilt-child role transitions. A joint edit is not automatically
/// a causal gain: both directions remain hypotheses until separately tested.
/// The role graph, traversal, and scoring are part of the editable descendant,
/// never part of the frozen external evaluator.
fn reciprocal_evolution_contract(
    history:&[Value],primary:&Value,frontier:&[Value]
)->Value {
    let Some(primary_role)=primary["role"].as_str() else{return Value::Null};
    let Some(primary_path)=primary["path"].as_str() else{return Value::Null};
    let adopted=history.iter().filter(|g|independent_adoption(g)).collect::<Vec<_>>();
    let observed=measured_interaction_edges(history);
    let mut transitions=BTreeMap::<(String,String),u64>::new();
    let mut joint=BTreeMap::<(String,String),u64>::new();
    for ancestor in &adopted {
        let ancestor_roles=model_design_roles(ancestor);
        for (index,left) in ancestor_roles.iter().enumerate() {
            for right in ancestor_roles.iter().skip(index+1) {
                if left==right {continue;}
                let pair=if left<right {(left.clone(),right.clone())}
                    else{(right.clone(),left.clone())};
                *joint.entry(pair).or_default()+=1;
            }
        }
        for descendant in adopted.iter().filter(|child|
            child["generatorCommit"]==ancestor["acceptedCommit"]
                && child["selfControllerVerified"]==true) {
            for left in &ancestor_roles {
                for right in model_design_roles(descendant) {
                    if left!=&right {
                        *transitions.entry((left.clone(),right)).or_default()+=1;
                    }
                }
            }
        }
    }
    let counterpart=frontier.iter().filter_map(|candidate| {
        let role=candidate["role"].as_str()?;
        if role==primary_role || candidate["path"].as_str()==Some(primary_path)
            || candidate["sourceSha256"].as_str().is_none_or(|sha|sha.len()!=64) {
            return None;
        }
        let forward=transitions.get(&(primary_role.to_string(),role.to_string()))
            .copied().unwrap_or(0);
        let reverse=transitions.get(&(role.to_string(),primary_role.to_string()))
            .copied().unwrap_or(0);
        let pair=if primary_role<role {(primary_role.to_string(),role.to_string())}
            else{(role.to_string(),primary_role.to_string())};
        let observed_joint=joint.get(&pair).copied().unwrap_or(0);
        let (gain_ab,regression_ab,joint_ab,_)=observed
            .get(&(primary_role.to_string(),role.to_string()))
            .copied().unwrap_or_default();
        let (gain_ba,regression_ba,joint_ba,_)=observed
            .get(&(role.to_string(),primary_role.to_string()))
            .copied().unwrap_or_default();
        // Reciprocal observed transitions dominate one-way successes; an
        // editable meta-method paired with a task capability is the cold-start
        // prior, not an immutable constraint on future invented roles.
        let meta_prior=if role=="improvement_methods"
            || primary_role=="improvement_methods" {1.0}else{0.0};
        let attempts=candidate["attempts"].as_u64().unwrap_or(0) as f64;
        let unexplored=if candidate["verifiedRedesigns"].as_u64().unwrap_or(0)==0
            {0.3}else{0.0};
        let reversal_prior=adopted_reciprocal_reversal(history,frontier)
            .is_some_and(|(next,previous)|next["role"]==primary_role
                && previous==role);
        let score=12.0*(forward.min(reverse) as f64)
            +4.0*((forward+reverse) as f64)
            +2.0*(observed_joint as f64)
            +8.0*(gain_ab.min(gain_ba) as f64)
            +3.0*((gain_ab+gain_ba) as f64)
            +0.5*((joint_ab+joint_ba) as f64)
            -6.0*((regression_ab+regression_ba) as f64)
            +if reversal_prior{24.0}else{0.0}
            +meta_prior+unexplored+1.0/(1.0+attempts);
        Some((candidate.clone(),score,forward,reverse,observed_joint,
            gain_ab,gain_ba,regression_ab,regression_ba))
    }).max_by(|a,b|a.1.total_cmp(&b.1)
        .then_with(||b.0["path"].as_str().cmp(&a.0["path"].as_str())));
    let Some((partner,_,forward,reverse,joint_adoptions,
        gain_ab,gain_ba,regression_ab,regression_ba))=counterpart
        else{return Value::Null};
    let mode=if forward>0 && reverse>0 {"observed_bidirectional_lineage_transfer"}
        else if forward+reverse+joint_adoptions>0 {"partial_interaction_evidence"}
        else {"new_bidirectional_experiment"};
    json!({
        "source":"compiled_descendant_reciprocal_evolution_v1",
        "mode":mode,
        "primary":primary,"partner":partner,
        "roles":[primary["role"],partner["role"]],
        "sourceFiles":[primary["path"],partner["path"]],
        "sourceHashes":[primary["sourceSha256"],partner["sourceSha256"]],
        "observedForwardTransfers":forward,
        "observedReverseTransfers":reverse,
        "observedJointAdoptions":joint_adoptions,
        "measuredForwardHoldoutGains":gain_ab,
        "measuredReverseHoldoutGains":gain_ba,
        "measuredForwardRegressions":regression_ab,
        "measuredReverseRegressions":regression_ba,
        "twoWayMeasuredInteraction":gain_ab>0&&gain_ba>0,
        "twoWayTemporalEvidence":forward>0 && reverse>0,
        "hypothesis":"A source change to the first capability should improve the second capability's future improvement procedure, and a change to the second should improve subsequent redesigns of the first.",
        "requiredNewWork":["inspect_both_exact_current_sources",
            "write_a_coupled_task_and_improvement_method_change",
            "compare_frozen_independent_holdouts",
            "rebuild_and_observe_next_descendant_role_transfer"],
        "causalImprovementProven":false,
        "qualityProven":false
    })
}

/// Only a truly adopted two-role patch can ask the next compiled child to
/// reverse direction. Source SHA receipts must cover BOTH role mappings;
/// merely mentioning two roles in the model's design is never sufficient.
fn adopted_reciprocal_reversal(history:&[Value],frontier:&[Value])
    ->Option<(Value,String)>{
    let ancestor=history.last()?;
    if !independent_adoption(ancestor)
        || ancestor["selfControllerVerified"]!=true{return None;}
    let pair=&ancestor["systemAgenda"]["reciprocalExperiment"];
    if pair["source"]!="compiled_descendant_reciprocal_evolution_v1"{
        return None;
    }
    let old_primary=pair["primary"]["role"].as_str()?;
    let old_partner=pair["partner"]["role"].as_str()?;
    if old_primary==old_partner{return None;}
    let targets=ancestor["systemDesign"]["targetFiles"].as_array()?;
    for component in [&pair["primary"],&pair["partner"]]{
        let role=component["role"].as_str()?;
        let path=component["path"].as_str()?;
        let role_files=ancestor["systemDesign"]["componentFiles"][role].as_array()?;
        if !role_files.iter().any(|item|item==path)
            || !targets.iter().any(|item|item==path)
            || ancestor["acceptedSourceHashes"][path].as_str().is_none_or(|sha|
                sha.len()!=64||!sha.bytes().all(|b|b.is_ascii_hexdigit())){
            return None;
        }
    }
    let target=frontier.iter().find(|row|
        row["role"]==old_partner && row["domain"].is_string()
            && row["path"].is_string()
            && row["sourceSha256"].as_str()
                .is_some_and(|sha|sha.len()==64))?;
    Some((target.clone(),old_primary.to_string()))
}

/// Attach the new descendant's counterpart to ACTUAL automatically selected
/// edit focus. Explicit owner focus and model-authored inherited stages are
/// not overwritten; the immutable parent still checks both source hashes.
pub(crate) fn attach_reciprocal_focus(agenda:&mut Value,policy:&Value) {
    if agenda["inheritedStage"].is_object()
        || agenda["target"]!=policy["target"] {return;}
    let partner=&policy["reciprocalExperiment"]["partner"];
    if !partner.is_object() || agenda["frontier"].as_array()
        .is_none_or(|rows|!rows.contains(partner)) {return;}
    let Some(focus)=agenda["focus"].as_array_mut() else{return};
    if focus.len()!=1 || focus[0]!=policy["target"] || focus.contains(partner) {
        return;
    }
    focus.push(partner.clone());
    agenda["reciprocalExperiment"]=policy["reciprocalExperiment"].clone();
}

/// Compare independent outcomes across generations, rather than choosing
/// an arbitrary first unmodified module. Success counts only when a
/// model-authored system design changed its declared source and the original
/// paired evaluator adopted that change. Future-generation productivity is a
/// distinct observation from immediate benchmark gain.
pub(crate) fn synthesize(
    history:&[Value],frontier:&[Value],allowed_domains:&[&str],
    inherited_domain:Option<&str>
)->Value{
    let calibration=confidence_adjustment(history);
    let mut ranked=Vec::<(Value,f64)>::new();
    let trials=history.len() as f64;
    for target in frontier.iter().filter(|row|
        row["domain"].as_str().is_some_and(|domain|allowed_domains.contains(&domain))) {
        let Some(role)=target["role"].as_str() else{continue};
        let relevant=history.iter().filter(|g|role_touched(g,role)).collect::<Vec<_>>();
        let attempts=relevant.len();
        let adopted=relevant.iter().filter(|g|independent_adoption(g)).count();
        let regressions=relevant.iter().filter(|g|
            g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0).count();
        let fruitless=relevant.iter().filter(|g|
            g["candidateId"].is_null() && g["accepted"]!=true).count();
        let productivity=relevant.iter()
            .map(|g|next_generation_productivity(history,g)).sum::<usize>();
        let meta_rewrites=relevant.iter().filter(|g|meta_source_changed(g)).count();
        let productive_meta_rewrites=relevant.iter().filter(|g|
            meta_source_changed(g) && next_generation_productivity(history,g)>0)
            .count();
        let failure_posterior=failure_distribution(&relevant);
        let probability=(adopted as f64+1.0)/(attempts as f64+2.0);
        let epistemic=(probability*(1.0-probability)/(attempts as f64+3.0)).sqrt();
        let coverage=if target["verifiedRedesigns"].as_u64().unwrap_or(0)>0{0.0}else{0.22};
        let inherited_bonus=if inherited_domain==target["domain"].as_str(){0.12}else{0.0};
        let size=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(size/12_000.0).min(1.0);
        let penalty=regressions as f64/(attempts.max(1) as f64);
        let stall=fruitless as f64/(attempts.max(1) as f64);
        let compiler_risk=failure_posterior["compiler_failure"].as_f64().unwrap_or(0.2);
        let regression_risk=failure_posterior["behavior_regression"].as_f64().unwrap_or(0.2);
        let edit_risk=failure_posterior["no_verified_code_candidate"].as_f64().unwrap_or(0.2);
        let clade=(productivity as f64/(adopted.max(1) as f64)).min(3.0);
        // UCB-style uncertainty, Bayesian shrinkage, and descendant
        // productivity provide a bounded, auditable choice. A failed edit is
        // evidence about execution feasibility, not proof the design is bad.
        let score=0.55*probability+0.48*epistemic*calibration
            +0.14*((trials+2.0).ln()/(attempts as f64+1.0)).sqrt()
            +coverage+inherited_bonus+0.14*clade
            +0.08*(productive_meta_rewrites as f64).min(3.0)
            -0.12*penalty-0.07*stall-0.035*cost
            -0.09*compiler_risk-0.06*regression_risk-0.06*edit_risk;
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "sourceSha256":target["sourceSha256"],
            "score":score,"predictedAdoptionProbability":probability,
            "uncertainty":epistemic*calibration,
            "failurePosterior":failure_posterior,
            "attempts":attempts,
            "verifiedModelAdoptions":adopted,"regressions":regressions,
            "editsWithoutCandidate":fruitless,
            "productiveDescendantAdoptions":productivity,
            "verifiedMetaSourceRewrites":meta_rewrites,
            "productiveMetaSourceRewrites":productive_meta_rewrites,
            "newComponent":target["verifiedRedesigns"].as_u64().unwrap_or(0)==0
        }),score));
    }
    ranked.sort_by(|(left,a),(right,b)|
        b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||left["role"].as_str().cmp(&right["role"].as_str())));
    let reverse_step=adopted_reciprocal_reversal(history,frontier);
    let mut winner=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    if let Some((ref reverse_target,_))=reverse_step {
        if let Some((row,_))=ranked.iter().find(|(row,_)|
            row["target"]==*reverse_target){
            winner=row.clone();
        }
    }
    let target=winner["target"].clone();
    let reciprocal=reciprocal_evolution_contract(history,&target,frontier);
    let reference=history.iter().rev().filter(|g|independent_adoption(g))
        .take(2).map(|g|json!({
            "generationJobId":g["jobId"],"acceptedCommit":g["acceptedCommit"],
            "roles":model_design_roles(g),
            "hypothesis":g["systemDesign"]["hypothesis"].as_str().unwrap_or("")
                .chars().take(400).collect::<String>(),
            "design":g["systemDesign"]["design"].as_str().unwrap_or("")
                .chars().take(700).collect::<String>(),
            "targetFiles":g["systemDesign"]["targetFiles"],
            "metaGain":g["evaluation"]["metrics"]["metaHoldoutGains"],
            "productiveDescendants":next_generation_productivity(history,g)
        })).collect::<Vec<_>>();
    let counterexamples=history.iter().rev().filter(|g|
        g["accepted"]!=true && !model_design_roles(g).is_empty())
        .take(3).map(|g|json!({
            "generationJobId":g["jobId"],"roles":model_design_roles(g),
            "failureKind":failure_kind(g),
            "sourceTarget":g["systemDesign"]["targetFiles"],
            "hypothesis":g["systemDesign"]["hypothesis"].as_str().unwrap_or("")
                .chars().take(250).collect::<String>(),
            "error":g["error"].as_str().unwrap_or("")
                .chars().take(500).collect::<String>()
        })).collect::<Vec<_>>();
    let mode=if counterexamples.iter().any(|row|
        row["failureKind"]=="compiler_failure"){"causal_repair_and_new_design"}
        else if !reference.is_empty(){"comparative_hybridization"}
        else{"open_ended_mechanism_discovery"};
    let alternatives=ranked.iter().take(6)
        .map(|(row,_)|row.clone()).collect::<Vec<_>>();
    let serialized=json!([target,alternatives,reference,counterexamples,mode,
        inherited_domain,history.len(),reciprocal,reverse_step]);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&serialized).unwrap_or_default()));
    json!({
        "source":"compiled_editable_meta_controller_v1",
        "revision":revision,"historyCount":history.len(),
        "mode":mode,"target":target,"selectionEvidence":winner,
        "reciprocalExperiment":reciprocal,
        "reciprocalContinuation":reverse_step.as_ref().map(|(target,from)|
            json!({"source":"verified_descendant_reverse_redesign_v1",
                "nextRole":target["role"],"priorRole":from,
                "sourceSha256":target["sourceSha256"],
                "priorChangeWasAdopted":true,"qualityProven":false}))
            .unwrap_or(Value::Null),
        "alternatives":alternatives,
        "referenceDesigns":reference,"counterexamples":counterexamples,
        "forecast":{"acceptanceProbability":winner["predictedAdoptionProbability"],
            "uncertainty":winner["uncertainty"],
            "failurePosterior":winner["failurePosterior"],
            "calibrationFactor":calibration,
            "qualityProven":false},
        "metaMechanismSource":"src/engine/rsi_meta_controller.rs",
        "successorCanRewriteMetaMechanism":true,
        "frozenEvaluatorUnchanged":true
    })
}

/// A frozen-case digest distinguishes compatible comparisons from coincidental
/// success labels. An absent suite never admits peers into the archive.
pub(crate) fn comparison_profile(input:&Value)->Option<String>{
    let cases=input["rsiFrozenCases"].as_array().filter(|cases|!cases.is_empty())?;
    Some(format!("{:x}",Sha256::digest(serde_json::to_vec(cases).ok()?)))
}

/// MGM-style other-lineage references: the built successor may inspect
/// independently paired designs from other campaigns sharing the same frozen
/// suite. No foreign patch is imported or credited to this descendant.
pub(crate) fn peer_lineages(
    state:&crate::state::AppState,campaign:&str,input:&Value,
    preferred_role:Option<&str>,
)->Value {
    let profile=comparison_profile(input);
    let mut ranked=Vec::<(i64,Value)>::new();
    let mut patches=BTreeSet::new();
    if let Some(sha)=profile.as_deref(){
        for generation in state.store.all("rsi_generation").into_iter().take(256) {
            if generation["campaignId"].as_str()==Some(campaign)
                || generation["comparisonProfileSha256"].as_str()!=Some(sha)
                || !independent_adoption(&generation) {continue;}
            let Some(candidate)=generation["candidateId"].as_str()
                .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
            if candidate["status"]!="verified"
                || candidate["evaluation"]["metrics"]["eligible"]!=true
                || candidate["baseCommit"]!=generation["generatorCommit"]
                || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
                || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
            let Some(patch)=candidate["patchSha256"].as_str() else{continue};
            if !patches.insert(patch.to_string()){continue;}
            let mut aligned=Vec::new();let mut same=0usize;
            for file in generation["systemDesign"]["targetFiles"].as_array()
                .into_iter().flatten().filter_map(Value::as_str).take(24) {
                if !crate::engine::rsi::safe_path(file){continue;}
                if let Some(current)=crate::engine::code_index::indexed_source_hash(
                    state,input["rsiIndexId"].as_str().unwrap_or(""),file) {
                    same+=usize::from(
                        generation["acceptedSourceHashes"][file].as_str()==Some(current.as_str()));
                    aligned.push(file.to_owned());
                }
            }
            if aligned.is_empty() || same==0 {continue;}
            // An unrelated lineage with a different current source revision
            // is not transferable merely because the filename overlaps.
            let roles=model_design_roles(&generation);
            let match_role=preferred_role.is_some_and(|role|roles.iter().any(|r|r==role));
            let gain=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0);
            let score=i64::from(match_role)*1000+gain.min(20) as i64*30
                +same.min(8) as i64*3;
            ranked.push((score,json!({
                "source":"paired_cross_campaign_design_reference_v1",
                "campaignId":generation["campaignId"],
                "generationId":generation["id"],
                "candidateId":candidate["id"],
                "patchSha256":patch,
                "generatorCommit":generation["generatorCommit"],
                "acceptedCommit":generation["acceptedCommit"],
                "comparisonProfileSha256":sha,
                "roles":roles,"alignedSourceFiles":aligned,
                "identicalSourceFiles":same,
                "nextDomain":generation["systemDesign"]["nextDomain"],
                "hypothesis":generation["systemDesign"]["hypothesis"].as_str()
                    .unwrap_or("").chars().take(400).collect::<String>(),
                "designSummary":generation["systemDesign"]["design"].as_str()
                    .unwrap_or("").chars().take(600).collect::<String>(),
                "automaticPatchReuse":false,
                "qualityProvenForThisDescendant":false
            })));
        }
    }
    ranked.sort_by(|a,b|b.0.cmp(&a.0).then_with(||
        a.1["candidateId"].as_str().cmp(&b.1["candidateId"].as_str())));
    json!({
        "source":"paired_cross_campaign_reference_archive_v1",
        "comparisonProfileSha256":profile,
        "references":ranked.into_iter().take(4).map(|(_,row)|row).collect::<Vec<_>>(),
        "automaticPatchReuse":false
    })
}

/// Verify the peer's raw paired receipt and unchanged source identity;
/// reference text can advise an experiment, never waive independent testing.
pub(crate) fn validate_peer_lineages(
    state:&crate::state::AppState,peers:&Value,
    profile:Option<&str>,campaign:&str,
)->Result<(),String>{
    if peers["source"]!="paired_cross_campaign_reference_archive_v1"
        || peers["comparisonProfileSha256"].as_str()!=profile
        || peers["automaticPatchReuse"]!=false {
        return Err("교차 계보 비교의 평가 조건 또는 출처가 다릅니다.".into());
    }
    let rows=peers["references"].as_array().filter(|rows|rows.len()<=4)
        .ok_or("교차 계보 참고 수가 범위를 벗어났습니다.")?;
    let mut seen=BTreeSet::new();
    for row in rows {
        let id=row["candidateId"].as_str()
            .ok_or("교차 계보 후보 ID가 없습니다.")?;
        if !seen.insert(id)
            || row["source"]!="paired_cross_campaign_design_reference_v1"
            || row["comparisonProfileSha256"].as_str()!=profile
            || row["automaticPatchReuse"]!=false
            || row["campaignId"].as_str()==Some(campaign) {
            return Err("중복 또는 자기 계보 참고가 포함됐습니다.".into());
        }
        let generation_id=row["generationId"].as_str()
            .ok_or("교차 계보 기록 ID가 없습니다.")?;
        let generation=state.store.get("rsi_generation",generation_id)
            .ok_or("교차 계보 원본 세대가 없습니다.")?;
        if generation["campaignId"]!=row["campaignId"]
            || generation["candidateId"]!=id
            || generation["comparisonProfileSha256"].as_str()!=profile
            || generation["acceptedCommit"]!=row["acceptedCommit"]
            || !independent_adoption(&generation) {
            return Err("교차 계보 세대의 원본 검증 기록이 다릅니다.".into());
        }
        let candidate=state.store.get("rsi_candidate",id)
            .ok_or("교차 계보 원본 후보가 없습니다.")?;
        if candidate["status"]!="verified"
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || candidate["patchSha256"]!=row["patchSha256"]
            || candidate["baseCommit"]!=row["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=row["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate){
            return Err("교차 계보 후보의 독립 평가 영수증이 다릅니다.".into());
        }
    }
    Ok(())
}


/// Hybrid innovation is an executable code-search objective, not the reuse of
/// an ancestor's patch. The next compiled task/meta agent gets a bounded
/// structured causal contract: what failed, what was predicted, and what
/// architectural part of the procedure it should experiment with.
pub(crate) fn next_method_hypothesis(policy:&Value,history:&[Value])->Value{
    let selected=&policy["selectionEvidence"];
    let target=&policy["target"];
    let role=target["role"].as_str().unwrap_or("improvement_methods");
    let mut observations=history.iter().rev().filter(|g|
        g["metaPolicy"]["target"]["role"]==role
            && g["metaLearning"]["independentlyMeasured"]==true)
        .take(12);
    let last=observations.next();
    let surprise=last.and_then(|g|g["metaLearning"]["brierError"].as_f64())
        .unwrap_or(0.25);
    let predicted=selected["predictedAdoptionProbability"].as_f64().unwrap_or(0.5);
    let failures=&selected["failurePosterior"];
    let compiler=failures["compiler_failure"].as_f64().unwrap_or(0.2);
    let missing_edit=failures["no_verified_code_candidate"].as_f64().unwrap_or(0.2);
    let regression=failures["behavior_regression"].as_f64().unwrap_or(0.2);
    let plateau=failures["no_independent_gain"].as_f64().unwrap_or(0.2);
    let proposed=if compiler>=0.32 {
        "compiler_guided_meta_repair_and_source_contract_evolution"
    }else if missing_edit>=0.32 {
        "invent_a_new_executable_code_editing_method"
    }else if regression>=0.32 {
        "counterexample_guided_causal_meta_rewrite"
    }else if plateau>=0.36 {
        "diverse_cross_task_architecture_hybridization"
    }else if surprise>0.35 {
        "revise_uncertainty_and_counterfactual_evaluation"
    }else if selected["editsWithoutCandidate"].as_u64().unwrap_or(0)>1 {
        "invent_alternative_source_edit_method"
    }else if selected["productiveDescendantAdoptions"].as_u64().unwrap_or(0)==0
        && selected["verifiedModelAdoptions"].as_u64().unwrap_or(0)>0 {
        "improve_future_generator_productivity"
    }else{
        "combine_independently_adopted_meta_and_task_architecture"
    };
    json!({
        "source":"compiled_meta_method_proposal_v1",
        "targetRole":role,
        "metaRevision":policy["revision"],
        "methodFamily":proposed,
        "estimatedAdoptionProbability":predicted,
        "observedSurprise":surprise,
        "failurePosterior":failures,
        "requiredImplementation":["task_agent_source","self_improvement_source"],
        "qualityProven":false,
        "agentOwnsImplementation":true
    })
}


/// Mendelian recombination at the *hypothesis level*: no foreign code or
/// patches are copied. A current compiled successor fuses distinct verified
/// causal mechanisms into one falsifiable cross-component design contract.
/// Parent and worker can recompute the same contract from the checked peers.
pub(crate) fn compositional_hypothesis(peers:&Value,policy:&Value)->Value{
    let rows=peers["references"].as_array().cloned().unwrap_or_default();
    let primary=policy["target"]["role"].as_str().unwrap_or("");
    let mut ranked=rows.into_iter().filter(|row|
        row["source"]=="paired_cross_campaign_design_reference_v1"
            && row["automaticPatchReuse"]==false
            && row["identicalSourceFiles"].as_u64().unwrap_or(0)>0)
        .collect::<Vec<_>>();
    ranked.sort_by(|a,b|{
        let matches=|row:&Value|row["roles"].as_array().is_some_and(|roles|
            roles.iter().any(|v|v.as_str()==Some(primary)));
        matches(b).cmp(&matches(a))
            .then(b["identicalSourceFiles"].as_u64()
                .cmp(&a["identicalSourceFiles"].as_u64()))
            .then(a["candidateId"].as_str().cmp(&b["candidateId"].as_str()))
    });
    let Some(anchor)=ranked.first() else{return Value::Null};
    let anchor_roles=anchor["roles"].as_array().cloned().unwrap_or_default();
    let complementary=ranked.iter().skip(1).find(|other|
        other["patchSha256"]!=anchor["patchSha256"]
            && other["roles"].as_array().is_some_and(|other_roles|
                other_roles.iter().any(|role|!anchor_roles.contains(role))));
    let Some(second)=complementary else{return Value::Null};
    let mut roles=BTreeSet::new();
    for row in [anchor,second]{
        for role in row["roles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str){
            roles.insert(role.to_string());
        }
    }
    let mut target_sources=BTreeSet::new();
    for row in [anchor,second] {
        for path in row["alignedSourceFiles"].as_array().into_iter()
            .flatten().filter_map(Value::as_str) {
            target_sources.insert(path.to_string());
        }
    }
    let design=json!({
        "source":"compiled_comparative_meta_design_hypothesis_v1",
        "parentMetaRevision":policy["revision"],
        "peerCandidateIds":[anchor["candidateId"],second["candidateId"]],
        "peerPatchHashes":[anchor["patchSha256"],second["patchSha256"]],
        "roles":roles.into_iter().collect::<Vec<_>>(),
        "sourceAnchors":target_sources.into_iter().take(8).collect::<Vec<_>>(),
        "competingHypotheses":[anchor["hypothesis"],second["hypothesis"]],
        "inheritedMethods":[anchor["designSummary"],second["designSummary"]],
        "causalQuestion":"Can a newly authored task+meta architecture combine the two independently useful mechanisms without their original failure modes?",
        "requiredNewWork":["inspect_exact_current_source","design_new_coupled_mechanism",
            "edit_actual_agent_and_meta_controller","evaluate_independently"],
        "automaticPatchReuse":false,"qualityProvenForThisDescendant":false
    });
    design
}

/// The model—not this controller—authors the actual design, programs and
/// reusable procedures. Comparative observations are bounded and source
/// grounded; they carry no adoption rights.
pub(crate) fn instruction(policy:&Value)->String {
    let target=&policy["target"];
    let refs=policy["referenceDesigns"].as_array().into_iter().flatten()
        .take(2).map(|g|format!(
            "verified roles={} causal hypothesis={} design={} next generation gains={}",
            g["roles"],g["hypothesis"],g["design"],g["productiveDescendants"]
        )).collect::<Vec<_>>().join("\n");
    let failures=policy["counterexamples"].as_array().into_iter().flatten()
        .take(3).map(|g|format!(
            "failed roles={} cause={} source={} observed error={}",
            g["roles"],g["failureKind"],g["sourceTarget"],g["error"]
        )).collect::<Vec<_>>().join("\n");
    let mut prompt=format!(
        "[EDITABLE META-IMPROVER revision={}]\n\
        Selection mode={}; chosen intelligence subsystem={} domain={} source={} hash={}. \
        Estimated adoption chance={} uncertainty={}; this is an unproven forecast.\n\
        Compare MULTIPLE independent earlier experiments, not one trajectory. \
        Valid successes and their descendant productivity:\n{}\n\
        Counterexamples to avoid repeating:\n{}\n\
        Author the novel executable redesign yourself, including new modules or \
        multi-component changes. You may also edit src/engine/rsi_meta_controller.rs, \
        src/engine/rsi_system.rs and the task/memory/learning/tool harness, so \
        the next compiled child changes HOW it designs improvements. \
        Explain a counterfactual or failure mode for your proposal and predict \
        observable changes before editing. After an accepted change the NEXT \
        binary must generate the new improvement plan. Frozen verification is \
        external and cannot be changed.",
        policy["revision"],policy["mode"],target["role"],target["domain"],
        target["path"],target["sourceSha256"],
        policy["forecast"]["acceptanceProbability"],policy["forecast"]["uncertainty"],
        refs,failures
    );
    let pair=&policy["reciprocalExperiment"];
    if pair["partner"].is_object() {
        prompt.push_str(&format!(
            "\n[EXECUTABLE RECIPROCAL SYSTEM EXPERIMENT]\n\
            Primary role={} file={} SHA={}. Partner role={} file={} SHA={}. \
            Observed parent-to-child transfers forward={}, reverse={}, jointly adopted designs={}. \
            These temporal observations are NOT proof of a causal improvement. \
            When automatic whole-system focus includes both files, inspect \
            both exact source versions, invent an interaction in which each \
            capability improves the other's future improvement procedure, \
            and WRITE both implementations. Test with the original frozen \
            evaluator; only the next rebuilt binary may inherit adopted code.\n",
            pair["primary"]["role"],pair["primary"]["path"],
            pair["primary"]["sourceSha256"],pair["partner"]["role"],
            pair["partner"]["path"],pair["partner"]["sourceSha256"],
            pair["observedForwardTransfers"],pair["observedReverseTransfers"],
            pair["observedJointAdoptions"]
        ));
    }
    prompt
}

/// New prediction errors are durable observations available to later compiled
/// meta-controllers. No model statement can turn a prediction into evidence.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    if policy["source"]!="compiled_editable_meta_controller_v1" {
        return Value::Null;
    }
    // An inherited, independently adopted method stage may legitimately
    // override the generic meta-controller. Its real outcome must not be
    // used to calibrate a forecast made for a different source/role.
    if generation["systemAgenda"]["target"]!=policy["target"] {
        return json!({"policyRevision":policy["revision"],
            "selectedByInheritedStage":true,
            "actualRole":generation["systemAgenda"]["target"]["role"],
            "forecastRole":policy["target"]["role"],
            "independentlyMeasured":false,
            "brierError":null,"categoricalBrier":null,
            "qualityProven":false});
    }
    let probability=policy["forecast"]["acceptanceProbability"]
        .as_f64().filter(|p|p.is_finite()&&*p>=0.0&&*p<=1.0);
    let assessed=generation["evaluation"]["verdict"].as_str().is_some()
        && generation["evaluation"]["metrics"]["eligible"].is_boolean();
    let observed=if assessed{Some(independent_adoption(generation))}else{None};
    let brier=match(probability,observed){
        (Some(p),Some(success))=>Some((p-if success{1.0}else{0.0}).powi(2)),
        _=>None
    };
    let class=observed_class(generation);
    let categorical_brier=class.and_then(|actual|{
        let probs=&policy["forecast"]["failurePosterior"];
        let mut error=0.0;
        for label in OUTCOME_CLASSES {
            let probability=probs[label].as_f64().filter(|p|p.is_finite()
                && *p>=0.0&&*p<=1.0)?;
            let truth=if label==actual{1.0}else{0.0};
            error+=(probability-truth).powi(2);
        }
        Some((error/2.0).min(1.0))
    });
    json!({
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "predictedProbability":probability,
        "independentlyMeasured":observed.is_some(),
        "actualModelAuthoredAdoption":observed,
        "brierError":brier,
        "categoricalBrier":categorical_brier,
        "observedFailureClass":class,
        "failureKind":if independent_adoption(generation){"adopted"}
            else{failure_kind(generation)},
        "attemptedSourceChange":generation["candidateId"].is_string(),
        "nextPolicyCanTrainFromObservation":observed.is_some(),
        "verifiedMetaImplementationChanged":meta_source_changed(generation),
        "qualityProven":observed==Some(true)
    })
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn reciprocal_evolution_chooses_second_real_role_and_reaches_edit_focus(){
        let row=|role:&str,path:&str|json!({
            "role":role,"domain":"improver","path":path,
            "sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "attempts":0,"verifiedRedesigns":0
        });
        let frontier=vec![
            row("harness","src/engine/jobs.rs"),
            row("improvement_methods","src/engine/rsi_meta_controller.rs"),
            row("memory","src/engine/memory.rs")
        ];
        let plan=synthesize(&[],&frontier,&["improver"],None);
        assert!(plan["reciprocalExperiment"]["partner"].is_object());
        assert_ne!(plan["target"]["role"],
            plan["reciprocalExperiment"]["partner"]["role"]);
        let mut agenda=json!({"target":plan["target"],
            "frontier":frontier,"focus":[plan["target"]],"inheritedStage":null});
        attach_reciprocal_focus(&mut agenda,&plan);
        assert_eq!(agenda["focus"].as_array().unwrap().len(),2);
        assert_eq!(agenda["focus"][1],plan["reciprocalExperiment"]["partner"]);
        agenda["inheritedStage"]=json!({"source":"model_authored_stage"});
        agenda["focus"]=json!([plan["target"]]);
        attach_reciprocal_focus(&mut agenda,&plan);
        assert_eq!(agenda["focus"].as_array().unwrap().len(),1);
    }
    #[test]
    fn reciprocal_evolution_uses_only_real_adopted_cross_role_descendants(){
        let adopted=|id:&str,base:&str,commit:String,role:&str|json!({
            "jobId":id,"generatorCommit":base,
            "accepted":true,"pairedRevalidated":true,"selfControllerVerified":true,
            "evaluation":{"metrics":{"eligible":true}},
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":commit,
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":id,
                "component":"system","components":[role],
                "targetFiles":["src/engine/jobs.rs"]},
            "acceptedSourceHashes":{"src/engine/jobs.rs":"b".repeat(64)}
        });
        let a="a".repeat(40);let b="b".repeat(40);
        let c="c".repeat(40);let d="d".repeat(40);
        let history=vec![
            adopted("meta-start","original",a.clone(),"improvement_methods"),
            adopted("harness-next",&a,b,"harness"),
            adopted("harness-start","independent",c.clone(),"harness"),
            adopted("meta-next",&c,d,"improvement_methods")
        ];
        let role=|role:&str|json!({"role":role,"domain":"improver",
            "path":if role=="harness"{"src/engine/jobs.rs"}
                else{"src/engine/rsi_meta_controller.rs"},
            "sourceSha256":"a".repeat(64),"sourceBytes":100});
        let primary=role("harness");
        let experiment=reciprocal_evolution_contract(
            &history,&primary,&[primary.clone(),role("improvement_methods")]);
        assert_eq!(experiment["observedForwardTransfers"],1);
        assert_eq!(experiment["observedReverseTransfers"],1);
        assert_eq!(experiment["observedJointAdoptions"],0);
        assert_eq!(experiment["twoWayTemporalEvidence"],true);
        assert_eq!(experiment["causalImprovementProven"],false);
        let mut forged=history;
        forged[3]["pairedRevalidated"]=json!(false);
        let weakened=reciprocal_evolution_contract(
            &forged,&primary,&[primary.clone(),role("improvement_methods")]);
        assert_eq!(weakened["observedForwardTransfers"],0);
        assert_eq!(weakened["observedReverseTransfers"],1);
    }
    #[test]
    fn source_mapped_holdout_edges_separate_isolated_and_joint_changes(){
        let single=json!({"jobId":"j","generatorCommit":"base",
            "accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":"a".repeat(40),
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"base","generationJobId":"j",
                "component":"harness","targetFiles":["src/engine/jobs.rs"]},
            "acceptedSourceHashes":{"src/engine/jobs.rs":"a".repeat(64)},
            "evaluation":{"metrics":{"eligible":true,"holdoutEvidence":[
                {"domain":"memory","baselinePass":false,"candidatePass":true,
                    "metricGains":0,"metricRegressions":0}]}}});
        let edges=measured_interaction_edges(&[single.clone()]);
        assert_eq!(edges[&("harness".to_string(),"memory".to_string())].0,1);
        let mut joint=single;
        joint["systemDesign"]["component"]="system".into();
        joint["systemDesign"]["components"]=json!(["harness","learning"]);
        joint["systemDesign"]["componentFiles"]=json!({
            "harness":["src/engine/jobs.rs"],
            "learning":["src/engine/learning.rs"]});
        joint["systemDesign"]["targetFiles"]=json!([
            "src/engine/jobs.rs","src/engine/learning.rs"]);
        joint["acceptedSourceHashes"]["src/engine/learning.rs"]=
            json!("b".repeat(64));
        let edges=measured_interaction_edges(&[joint]);
        assert_eq!(edges[&("harness".to_string(),"memory".to_string())].0,0);
        assert_eq!(edges[&("harness".to_string(),"memory".to_string())].2,1);
    }
    #[test]
    fn reverse_redesign_requires_both_accepted_component_source_receipts(){
        let source=|role:&str,path:&str|json!({
            "role":role,"path":path,"domain":"improver",
            "sourceSha256":"c".repeat(64),"sourceBytes":100,
            "verifiedRedesigns":0});
        let harness=source("harness","src/engine/jobs.rs");
        let learning=source("learning","src/engine/learning.rs");
        let ancestor=json!({
            "jobId":"j","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"accepted":true,
            "pairedRevalidated":true,"selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "systemAgenda":{"reciprocalExperiment":{
                "source":"compiled_descendant_reciprocal_evolution_v1",
                "primary":harness,"partner":learning}},
            "systemDesign":{"source":"model_authored_system_design",
                "component":"system","components":["harness","learning"],
                "generatorCommit":"a".repeat(40),"generationJobId":"j",
                "targetFiles":["src/engine/jobs.rs","src/engine/learning.rs"],
                "componentFiles":{"harness":["src/engine/jobs.rs"],
                    "learning":["src/engine/learning.rs"]}},
            "acceptedSourceHashes":{"src/engine/jobs.rs":"c".repeat(64),
                "src/engine/learning.rs":"d".repeat(64)}
        });
        let frontier=vec![source("harness","src/engine/jobs.rs"),
            source("learning","src/engine/learning.rs")];
        let result=adopted_reciprocal_reversal(&[ancestor.clone()],&frontier);
        assert_eq!(result.unwrap().0["role"],"learning");
        let mut invalid=ancestor;
        invalid["acceptedSourceHashes"]["src/engine/learning.rs"]=Value::Null;
        assert!(adopted_reciprocal_reversal(&[invalid],&frontier).is_none());
    }
    #[test]
    fn comparison_prioritizes_novelty_then_updates_after_grounded_failures(){
        let target=|role:&str,path:&str|json!({"role":role,"domain":if role=="memory"{"memory"}else{"learning"},
            "path":path,"sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "verifiedRoleChanged":false});
        let frontier=vec![target("memory","src/engine/memory.rs"),
            target("learning","src/engine/learning.rs")];
        let first=synthesize(&[],&frontier,&["memory","learning"],None);
        assert!(first["target"].is_object());
        assert_eq!(first["historyCount"],0);
        let mut failures=Vec::new();
        for n in 0..6{
            failures.push(json!({"jobId":format!("job-{n}"),"generatorCommit":"base",
                "accepted":false,"candidateId":null,
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":"base","generationJobId":format!("job-{n}"),
                    "component":"memory","targetFiles":["src/engine/memory.rs"]}}));
        }
        let later=synthesize(&failures,&frontier,&["memory","learning"],None);
        assert_eq!(later["target"]["role"],"learning");
        assert!(later["counterexamples"].as_array().unwrap().len()>0);
        assert_eq!(feedback(&later,&failures[0])["independentlyMeasured"],false);
    }
}
