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
        && crate::engine::rsi_system::design_implemented(&g["systemDesign"],
            &g["acceptedSourceHashes"].as_object().map(|object|
                object.keys().map(|path|json!(path)).collect::<Vec<_>>())
                .unwrap_or_default())
        && g["systemDesign"]["targetFiles"].as_array()
            .is_some_and(|files|!files.is_empty() && files.iter().all(|path|
                path.as_str().is_some_and(|p|
                    g["acceptedSourceHashes"][p].as_str().is_some_and(|sha|
                        sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
}
fn role_touched(g:&Value,role:&str)->bool{
    // Failed turns can teach execution feasibility without creating a
    // misleading claim of a model-authored, adopted improvement.
    model_design_roles(g).iter().any(|name|name==role)
        || ((g["metaPolicyExecuted"]==true
            && g["metaPolicy"]["source"]=="compiled_editable_meta_controller_v1"
            || g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary")
            && g["metaPolicy"]["target"]["role"]==role)
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
            && (child["metaPolicyExecuted"]==true
                || child["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary")
            && independent_adoption(child)
    ).count()
}
fn confidence_adjustment(history:&[Value])->f64{
    let errors=history.iter().filter_map(|row|
        row["metaLearning"]["publicCaseBrierError"].as_f64()
            .or_else(||row["metaLearning"]["proposalBrierError"].as_f64())
            .or_else(||row["metaLearning"]["brierError"].as_f64())
            .filter(|value|value.is_finite() && *value>=0.0 && *value<=1.0))
        .rev().take(12).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.6*mean).clamp(0.4,1.0)
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
    // The editable meta-controller consumes observed cross-role continuity.
    // This changes the next target, not merely the successor's commentary.
    let mutual=crate::engine::rsi_mutual_evolution::graph(history,frontier);
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
        // Predict observable task outcomes separately. Unevaluated candidates
        // are never mislabeled as independent evidence of quality.
        let assessed=relevant.iter().filter(|g|
            g["evaluation"]["verdict"].is_string()
                && g["evaluation"]["metrics"]["eligible"].is_boolean())
            .copied().collect::<Vec<_>>();
        let measured=assessed.len();
        let captured=relevant.iter().filter(|g|g["candidateId"].is_string()).count();
        let meta_gains=assessed.iter().filter(|g|
            g["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0).count();
        let probability=((adopted as f64+1.0)/(measured as f64+2.0)).clamp(0.0,1.0);
        let epistemic=(probability*(1.0-probability)/(measured as f64+3.0)).sqrt();
        // Learn case-specific candidate behavior ONLY from public training
        // observations, never frozen holdouts. The compiled meta-controller
        // may predict held-out aggregate gains, but cannot see held-out IDs
        // or outcomes as training examples.
        let mut cases=BTreeMap::<String,(u64,u64)>::new();
        for generation in &relevant{
            for row in generation["evaluation"]["metrics"]["measurements"]
                .as_array().into_iter().flatten(){
                if row["holdout"]!=false{continue;}
                let Some(id)=row["id"].as_str().filter(|id|!id.is_empty()
                    && id.len()<=128) else{continue;};
                let Some(passed)=row["candidatePass"].as_bool() else{continue;};
                let counts=cases.entry(id.to_string()).or_insert((0,0));
                counts.0+=1;
                counts.1+=u64::from(passed);
            }
        }
        let public_cases=cases.into_iter().take(16)
            .map(|(id,(attempts,passes))|json!({
                "caseId":id,"candidatePassProbability":
                    (passes as f64+1.0)/(attempts as f64+2.0),
                "observedTrials":attempts,
                "holdout":false,"qualityProven":false
            })).collect::<Vec<_>>();
        let public_case_mean=if public_cases.is_empty(){0.5}else{
            public_cases.iter().map(|row|row["candidatePassProbability"]
                .as_f64().unwrap_or(0.5)).sum::<f64>()/public_cases.len() as f64
        };
        let outcomes=json!({
            "candidateCaptured":(captured as f64+1.0)/(attempts as f64+2.0),
            "adoption":probability,
            "metaImprovement":(meta_gains as f64+1.0)/(measured as f64+2.0),
            "regression":(regressions as f64+1.0)/(measured as f64+2.0),
            "publicCaseForecasts":public_cases,
            "assessedGenerations":measured,
            "predictionProven":false
        });
        let coverage=if target["verifiedRoleImproved"]==true{0.0}else{0.22};
        let inherited_bonus=if inherited_domain==target["domain"].as_str(){0.12}else{0.0};
        let size=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(size/12_000.0).min(1.0);
        let penalty=regressions as f64/(attempts.max(1) as f64);
        let stall=fruitless as f64/(attempts.max(1) as f64);
        let clade=(productivity as f64/(adopted.max(1) as f64)).min(3.0);
        // UCB-style uncertainty, Bayesian shrinkage, and descendant
        // productivity provide a bounded, auditable choice. A failed edit is
        // evidence about execution feasibility, not proof the design is bad.
        let incoming=mutual["edges"].as_array().into_iter().flatten()
            .filter(|edge|edge["to"]==role)
            .filter_map(|edge|edge["priority"].as_i64()).max().unwrap_or(0);
        let transfer_bonus=0.20*((incoming as f64)/100.0).clamp(-0.5,1.5);
        let score=transfer_bonus+0.55*probability+0.48*epistemic*calibration
            +0.14*((trials+2.0).ln()/(attempts as f64+1.0)).sqrt()
            +coverage+inherited_bonus+0.14*clade
            -0.12*penalty-0.07*stall-0.035*cost
            +0.08*outcomes["metaImprovement"].as_f64().unwrap_or(0.5)
            -0.12*outcomes["regression"].as_f64().unwrap_or(0.5)
            +0.07*public_case_mean
            +crate::engine::rsi_open_topology::exploration_bonus(history,role);
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "sourceSha256":target["sourceSha256"],
            "score":score,"mutualTransferPriority":incoming,
            "openTopologyBonus":crate::engine::rsi_open_topology::exploration_bonus(history,role),
            "predictedAdoptionProbability":probability,
            "uncertainty":epistemic*calibration,"attempts":attempts,
            "verifiedModelAdoptions":adopted,"regressions":regressions,
            "editsWithoutCandidate":fruitless,
            "outcomeForecast":outcomes,
            "productiveDescendantAdoptions":productivity,
            "newComponent":target["verifiedRoleImproved"]!=true
        }),score));
    }
    ranked.sort_by(|(left,a),(right,b)|
        b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||left["role"].as_str().cmp(&right["role"].as_str())));
    let mut winner=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    // Repeated forecast misses can trigger improvement of the forecast rule
    // itself, not just another patch to the task agent. Bound this choice so
    // it cannot become an endless metacognitive self-edit loop.
    let recent_errors=history.iter().rev().filter_map(|g|
        g["metaLearning"]["publicCaseBrierError"].as_f64()
            .or_else(||g["metaLearning"]["proposalBrierError"].as_f64())
            .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p)))
        .take(6).collect::<Vec<_>>();
    let meta_recent=history.iter().rev().take(3).filter(|g|
        g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|
                file=="src/engine/rsi_meta_controller.rs"))).count();
    let forecast_drift=recent_errors.len()>=3
        && recent_errors.iter().sum::<f64>()/recent_errors.len() as f64>0.30;
    let repair_predictor=forecast_drift && meta_recent<2;
    if repair_predictor{
        if let Some(repair)=ranked.iter().find(|(row,_)|
            row["target"]["path"]=="src/engine/rsi_meta_controller.rs") {
            winner=repair.0.clone();
        }
    }
    let target=winner["target"].clone();
    let mutual_next=mutual["edges"].as_array().into_iter().flatten()
        .find(|edge|edge["to"]==target["role"]).cloned().unwrap_or(Value::Null);
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
    let mode=if repair_predictor
        && target["path"]=="src/engine/rsi_meta_controller.rs"{
            "repair_own_prediction_mechanism"
        }else if counterexamples.iter().any(|row|
        row["failureKind"]=="compiler_failure"){"causal_repair_and_new_design"}
        else if !reference.is_empty(){"comparative_hybridization"}
        else{"open_ended_mechanism_discovery"};
    let alternatives=ranked.iter().take(6)
        .map(|(row,_)|row.clone()).collect::<Vec<_>>();
    // This native successor policy changes source focus, execution mode,
    // and tool admission. An accepted model-authored successorStage overrides it.
    let path=target["path"].as_str().unwrap_or("");
    let repeated_create_failure=history.iter().rev().take(12).any(|g|
        g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|item|item.as_str()==Some(path)))
        && g["error"].as_str().is_some_and(|error|
            error.contains("workspace_write") || error.contains("새 파일 생성 전용")));
    let learning_mode=if winner["editsWithoutCandidate"].as_u64().unwrap_or(0)>0
        && winner["verifiedModelAdoptions"].as_u64().unwrap_or(0)>0{
        "retry_failed"
    }else{"avoid_failed"};
    let edit_mode=if target["sourceSha256"].is_string()
        && target["sourceBytes"].as_u64().unwrap_or(u64::MAX)<=24_000
        && (winner["attempts"].as_u64().unwrap_or(0)>0 || !reference.is_empty()){
        "grounded_edit"
    }else{"explore_then_edit"};
    let tool_mode=if repeated_create_failure{"patch_only"}else{"allow_new_modules"};
    let control=json!({
        "source":"editable_compiled_meta_control_v2",
        "role":target["role"],"domain":target["domain"],
        "path":target["path"],"sourceSha256":target["sourceSha256"],
        "learningPolicy":learning_mode,
        "toolPolicy":tool_mode,
        "executionPolicy":edit_mode,
        "qualityProven":false
    });
    let serialized=json!([target,alternatives,reference,counterexamples,mode,
        inherited_domain,history.len(),control,&mutual, &mutual_next]);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&serialized).unwrap_or_default()));
    json!({
        "source":"compiled_editable_meta_controller_v1",
        "revision":revision,"historyCount":history.len(),
        "mode":mode,"target":target,"selectionEvidence":winner,
        "alternatives":alternatives,
        "controlPolicy":control,
        "mutualDevelopmentGraph":mutual,
        "nextMutualRedesign":mutual_next,
        "taskOutcomeForecast":winner["outcomeForecast"],
        "forecastDrift":forecast_drift,
        "metaRepairChosen":repair_predictor
            && target["path"]=="src/engine/rsi_meta_controller.rs",
        "recentPredictionErrors":recent_errors,
        "referenceDesigns":reference,"counterexamples":counterexamples,
        "forecast":{"acceptanceProbability":winner["predictedAdoptionProbability"],
            "uncertainty":winner["uncertainty"],
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
            if aligned.is_empty(){continue;}
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
    format!(
        "[EDITABLE META-IMPROVER revision={}]\n\
        Selection mode={}; chosen intelligence subsystem={} domain={} source={} hash={}. \
        Estimated adoption chance={} uncertainty={}; this is an unproven forecast.\n\
        Predicted downstream outcomes={} and proposed executable control policy={}.\n\
        Next mutually developing subsystem link={} (observed succession, NOT causal proof).\n\
        If mode=repair_own_prediction_mechanism, redesign the editable prediction/search rule ITSELF using observed prediction errors, not the benchmark scorer. Before editing call rsi_system_plan with expectedOutcomes probabilities (candidateCaptured, adoption, metaImprovement, regression), then let independent assessment calibrate this proposal.\n\
        Compare MULTIPLE independent earlier experiments, not one trajectory. \
        Valid successes and their descendant productivity:\n{}\n\
        Counterexamples to avoid repeating:\n{}\n\
        Author the novel executable redesign yourself, including new modules or \
        multi-component changes. Explain how one changed subsystem could \
        improve the ability of ANOTHER subsystem to make subsequent changes. \
        For a coupled change, use component=system, components, componentFiles \
        and successorStages so the rebuilt child can carry out the next link. You may also edit src/engine/rsi_meta_controller.rs, \
        src/engine/rsi_system.rs and the task/memory/learning/tool harness, so \
        the next compiled child changes HOW it designs improvements. \
        Explain a counterfactual or failure mode for your proposal and predict \
        observable changes before editing. After an accepted change the NEXT \
        binary must generate the new improvement plan. Frozen verification is \
        external and cannot be changed.",
        policy["revision"],policy["mode"],target["role"],target["domain"],
        target["path"],target["sourceSha256"],
        policy["forecast"]["acceptanceProbability"],policy["forecast"]["uncertainty"],
        policy["taskOutcomeForecast"],policy["controlPolicy"],
        policy["nextMutualRedesign"],refs,failures
    )
}

/// Bounded, independently evaluated ideas from compatible foreign lineages.
pub(crate) fn peer_instruction(peers:&Value)->String {
    let lines=peers["references"].as_array().into_iter().flatten().take(3)
        .map(|row|format!("peer={} roles={} hypothesis={} design={} aligned={}",
            row["campaignId"],row["roles"],row["hypothesis"],
            row["designSummary"],row["alignedSourceFiles"]))
        .collect::<Vec<_>>();
    if lines.is_empty(){String::new()}else{
        format!("\nVERIFIED PEER LINEAGE REFERENCES (ideas only):\n{}\n",lines.join("\n"))
    }
}

/// New prediction errors are durable observations available to later compiled
/// meta-controllers. No model statement can turn a prediction into evidence.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    if policy["source"]!="compiled_editable_meta_controller_v1" {
        return Value::Null;
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
    // Capture the model's PROPOSAL-time forecast before editing. The
    // frozen evaluator supplies later observations, and missing evidence
    // must never become a fabricated regression or success.
    let proposal=&generation["systemDesign"]["expectedOutcomes"];
    let forecast=if proposal.is_object(){proposal}else{&policy["taskOutcomeForecast"]};
    let actuals=[
        ("candidateCaptured",Some(generation["candidateId"].is_string())),
        ("adoption",observed),
        ("metaImprovement",if assessed{Some(generation["evaluation"]["metrics"]
            ["metaHoldoutGains"].as_u64().unwrap_or(0)>0)}else{None}),
        ("regression",if assessed{Some(generation["evaluation"]["metrics"]
            ["regressions"].as_u64().unwrap_or(0)>0)}else{None})
    ];
    let mut errors=serde_json::Map::new();
    let mut losses=Vec::new();
    for (name,actual) in actuals{
        if let (Some(p),Some(result))=(forecast[name].as_f64()
            .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p)),actual){
            let loss=(p-if result{1.0}else{0.0}).powi(2);
            errors.insert(name.into(),json!({"predicted":p,"observed":result,
                "brierError":loss,"independentAssessment":name=="candidateCaptured"||assessed}));
            losses.push(loss);
        }
    }
    let proposal_error=if losses.is_empty(){Value::Null}else{
        json!(losses.iter().sum::<f64>()/losses.len() as f64)
    };
    // Compare only the visible training case predictions against actual
    // measurements. Held-out case IDs and results are never used as
    // predictive training labels.
    let mut public_losses=Vec::new();
    let mut public_observations=Vec::new();
    if assessed{
        for expected in policy["taskOutcomeForecast"]["publicCaseForecasts"]
            .as_array().into_iter().flatten().take(16){
            let Some(id)=expected["caseId"].as_str() else{continue;};
            let Some(p)=expected["candidatePassProbability"].as_f64()
                .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p))
                else{continue;};
            let actual=generation["evaluation"]["metrics"]["measurements"].as_array()
                .into_iter().flatten().find(|row|row["id"]==id
                    && row["holdout"]==false)
                .and_then(|row|row["candidatePass"].as_bool());
            if let Some(passed)=actual{
                let error=(p-if passed{1.0}else{0.0}).powi(2);
                public_losses.push(error);
                public_observations.push(json!({
                    "caseId":id,"predicted":p,"observed":passed,
                    "brierError":error,"holdout":false
                }));
            }
        }
    }
    let public_brier=if public_losses.is_empty(){Value::Null}else{
        json!(public_losses.iter().sum::<f64>()/public_losses.len() as f64)
    };
    json!({
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "taskPredictionErrors":errors,
        "proposalBrierError":proposal_error,
        "publicCasePredictionErrors":public_observations,
        "publicCaseBrierError":public_brier,
        "publicCasePredictionsAssessed":public_losses.len(),
        "forecastOrigin":if proposal.is_object(){"model_authored_pre_edit"}
            else{"compiled_meta_prior"},
        "predictedProbability":probability,
        "independentlyMeasured":observed.is_some(),
        "actualModelAuthoredAdoption":observed,
        "brierError":brier,
        "failureKind":if independent_adoption(generation){"adopted"}
            else{failure_kind(generation)},
        "attemptedSourceChange":generation["candidateId"].is_string(),
        "nextPolicyCanTrainFromObservation":observed.is_some(),
        "qualityProven":observed==Some(true)
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn comparison_prioritizes_novelty_then_updates_after_grounded_failures(){
        let target=|role:&str,path:&str|json!({"role":role,"domain":if role=="memory"{"memory"}else{"learning"},
            "path":path,"sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "verifiedRoleImproved":false});
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
