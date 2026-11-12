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
        let causal=crate::engine::rsi_causal_curriculum::role_interaction_feedback(
            history,role);
        let score=causal["selectionBonus"].as_f64().unwrap_or(0.0)
            +transfer_bonus+0.55*probability+0.48*epistemic*calibration
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
            "factorialInteractionEvidence":causal,
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

// Attribution is to one executed donor method on one exact source version
// and frozen comparison profile. A donor mentioned among several references
// is not evidence that its method caused a recipient's outcome.
fn measured_recipient(g:&Value)->bool{
    g["evaluation"]["verdict"].is_string()
        && g["evaluation"]["metrics"]["eligible"].is_boolean()
}
fn executed_recipient_context(
    g:&Value,role:&str,path:&str,source_sha:&str,profile:&Value
)->bool{
    g["metaPolicyExecuted"]==true
        && &g["comparisonProfileSha256"]==profile
        && g["metaPolicy"]["target"]["role"]==role
        && g["metaPolicy"]["target"]["path"]==path
        && g["metaPolicy"]["target"]["sourceSha256"]==source_sha
        && g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(path)))
}
fn recipient_source_adopted(g:&Value,path:&str,previous_sha:&str)->bool{
    independent_adoption(g)
        && g["systemDesign"]["targetFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|sha|
            sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())
                && sha!=previous_sha)
}

/// Turn independently paired *other-lineage* results into a bounded
/// experimental hypothesis rather than copying a foreign patch. A donor's
/// nextDomain is a proposed transferable METHOD, not an observed improvement
/// of the new role. The compiled descendant itself chooses a source-bound
/// recipient and must still author, build and independently test its changes.
///
/// Source compatibility (at least one identical current indexed source file)
/// and the same frozen comparison profile are required. A donor cannot
/// promote its own role as a repeated success by reporting a nextDomain.
pub(crate) fn evolve_from_peer_methods(mut policy:Value,peers:&Value,history:&[Value])->Value{
    if policy["source"]!="compiled_editable_meta_controller_v1"
        || policy["metaRepairChosen"]==true
        || peers["source"]!="paired_cross_campaign_reference_archive_v1"
        || peers["automaticPatchReuse"]!=false
        || peers["comparisonProfileSha256"].as_str()
            .is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){
        return policy;
    }
    let Some(donors)=peers["references"].as_array()else{return policy;};
    let Some(mut alternatives)=policy["alternatives"].as_array().cloned()
        else{return policy;};
    let original_revision=policy["revision"].clone();
    let mut transferred=false;
    for candidate in &mut alternatives{
        let (Some(role),Some(domain),Some(path),Some(hash))=(
            candidate["target"]["role"].as_str(),
            candidate["target"]["domain"].as_str(),
            candidate["target"]["path"].as_str(),
            candidate["target"]["sourceSha256"].as_str()
        )else{continue};
        if !crate::engine::rsi::safe_path(path) || hash.len()!=64
            || !hash.bytes().all(|b|b.is_ascii_hexdigit()) {continue;}
        let mut seen=BTreeSet::new();
        let mut bridges=Vec::<Value>::new();
        let mut learned_factors=Vec::<(String,f64)>::new();
        // Observational matched control: same role, source revision, frozen
        // suite and executed focus, but no peer-method intervention.
        let mut control_seen=BTreeSet::<String>::new();
        let mut control_trials=0_u64;
        let mut control_adoptions=0_u64;
        for g in history.iter().rev().take(256){
            if !executed_recipient_context(g,role,path,hash,
                &peers["comparisonProfileSha256"])
                || g["metaPolicy"]["peerMethodTransfer"]["active"]==true
                || !measured_recipient(g){continue;}
            let Some(job)=g["jobId"].as_str()else{continue};
            if !control_seen.insert(job.to_owned()){continue;}
            control_trials+=1;
            control_adoptions+=u64::from(recipient_source_adopted(g,path,hash));
        }
        let control_p=(control_adoptions as f64+1.0)/(control_trials as f64+2.0);
        for donor in donors.iter().take(4){
            if donor["source"]!="paired_cross_campaign_design_reference_v1"
                || donor["automaticPatchReuse"]!=false
                || donor["comparisonProfileSha256"]!=peers["comparisonProfileSha256"]
                || donor["nextDomain"]!=domain
                || donor["identicalSourceFiles"].as_u64().unwrap_or(0)==0
                || donor["alignedSourceFiles"].as_array()
                    .is_none_or(|v|v.is_empty())
                || donor["roles"].as_array().is_none_or(|roles|
                    roles.is_empty() || roles.iter().any(|r|r.as_str()==Some(role))){
                continue;
            }
            let Some(patch)=donor["patchSha256"].as_str()
                .filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))
                else{continue};
            if !seen.insert(patch.to_string()){continue;}
            let Some(donor_id)=donor["candidateId"].as_str()
                .filter(|id|!id.is_empty()) else{continue};
            // Learn the portability of this donor's METHOD from subsequent
            // executable recipient attempts, not from the donor's own win.
            // Match role, real focused source path, frozen suite, and donor.
            // Count unique generations. No held-out case data enter this law.
            let mut observed=BTreeSet::<String>::new();
            let mut attempts=0_u64;
            let mut measured=0_u64;
            let mut adopted=0_u64;
            for g in history.iter().rev().take(256){
                let observed_transfer=&g["metaPolicy"]["peerMethodTransfer"];
                if !executed_recipient_context(g,role,path,hash,
                    &peers["comparisonProfileSha256"])
                    || observed_transfer["active"]!=true
                    || observed_transfer["activeDonorCandidateId"]!=donor_id
                    || observed_transfer["comparisonProfileSha256"]
                        !=peers["comparisonProfileSha256"]
                    || observed_transfer["selectedRecipient"]["role"]!=role
                    || observed_transfer["selectedRecipient"]["path"]!=path
                    || observed_transfer["selectedRecipient"]["sourceSha256"]!=hash
                    || !observed_transfer["references"]
                        .as_array().is_some_and(|refs|refs.iter().any(|ref_row|
                            ref_row["candidateId"]==donor_id
                            && ref_row["sharedFrozenProfileSha256"]
                                ==peers["comparisonProfileSha256"])) {
                    continue;
                }
                let Some(id)=g["jobId"].as_str()else{continue};
                if !observed.insert(id.to_string()){continue;}
                attempts+=1;
                if measured_recipient(g){
                    measured+=1;
                    adopted+=u64::from(recipient_source_adopted(g,path,hash));
                }
            }
            let posterior=(adopted as f64+1.0)/(measured as f64+2.0);
            let unresolved=attempts.saturating_sub(measured);
            // A negative matched-control signal suppresses this method's
            // exploration bonus; it does not falsely establish causality.
            let negative_transfer=measured>=3 && control_trials>=3
                && posterior+0.12<control_p;
            let factor=if negative_transfer{0.0}else{
                (2.0*posterior).clamp(0.25,1.5)
                    *(1.0-0.10*unresolved.min(4) as f64)
                    *(1.0+(posterior-control_p).max(0.0)).min(1.5)
            };
            learned_factors.push((donor_id.to_owned(),factor));
            bridges.push(json!({
                "candidateId":donor["candidateId"],
                "fromCampaign":donor["campaignId"],
                "fromRoles":donor["roles"],
                "donorAcceptedCommit":donor["acceptedCommit"],
                "toRole":role,"toDomain":domain,
                "toSourcePath":path,"toSourceSha256":hash,
                "sharedFrozenProfileSha256":peers["comparisonProfileSha256"],
                "identicalIndexedSourceFiles":donor["identicalSourceFiles"],
                "observedTransferAttempts":attempts,
                "independentlyMeasuredTransferTrials":measured,
                "recipientSourceAdoptions":adopted,
                "empiricalTransferPosterior":posterior,
                "matchedControlTrials":control_trials,
                "matchedControlAdoptions":control_adoptions,
                "matchedControlPosterior":control_p,
                "negativeTransferSignal":negative_transfer,
                "singleDonorAttribution":true,
                "automaticPatchReuse":false,
                "transferProven":false
            }));
        }
        // Select exactly one donor intervention. Other references remain
        // hypotheses, never co-credited after the recipient's evaluation.
        let primary=learned_factors.iter().filter(|(_,factor)|*factor>0.0)
            .max_by(|(id_a,a),(id_b,b)|a.total_cmp(b)
                .then_with(||id_b.cmp(id_a)));
        let active_donor=primary.map(|(id,_)|id.as_str());
        let bonus=primary.map(|(_,factor)|0.12*factor).unwrap_or(0.0);
        let prior=candidate["score"].as_f64().filter(|s|s.is_finite())
            .unwrap_or(0.0);
        candidate["score"]=json!(prior+bonus);
        candidate["portableMethodExplorationBonus"]=json!(bonus);
        candidate["activeTransferDonorCandidateId"]=json!(active_donor);
        candidate["transferHypotheses"]=json!(bridges);
        transferred|=bonus>0.0;
    }
    // Keep negative and neutral trial evidence even when no peer method is
    // activated. The next compiled descendant can change its search rule
    // without mislabeling a rejected transfer as a successful design.
    let learning=alternatives.iter()
        .flat_map(|candidate|candidate["transferHypotheses"].as_array()
            .into_iter().flatten())
        .map(|bridge|json!({
            "donorCandidateId":bridge["candidateId"],
            "recipientRole":bridge["toRole"],
            "recipientSourceSha256":bridge["toSourceSha256"],
            "observedTransferTrials":bridge["independentlyMeasuredTransferTrials"],
            "adoptedTransferTrials":bridge["recipientSourceAdoptions"],
            "matchedControlTrials":bridge["matchedControlTrials"],
            "matchedControlAdoptions":bridge["matchedControlAdoptions"],
            "negativeTransferSignal":bridge["negativeTransferSignal"]
        })).collect::<Vec<_>>();
    policy["peerMethodLearning"]=json!({
        "source":"matched_executed_peer_method_observation_v1",
        "observations":learning,
        "causalSuperiorityProven":false,
        "isolationRequired":true
    });
    if !transferred{
        if !learning.is_empty(){
            policy["revision"]=json!(format!("{:x}",Sha256::digest(
                serde_json::to_vec(&json!([
                    original_revision,policy["peerMethodLearning"]
                ])).unwrap_or_default())));
        }
        return policy;
    }
    alternatives.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
        .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
        .then_with(||a["role"].as_str().cmp(&b["role"].as_str()))
        .then_with(||a["target"]["path"].as_str().cmp(&b["target"]["path"].as_str())));
    let Some(winner)=alternatives.first().cloned()else{return policy;};
    let target=winner["target"].clone();
    let bridge=winner["activeTransferDonorCandidateId"].is_string();
    policy["target"]=target.clone();
    policy["selectionEvidence"]=winner.clone();
    policy["alternatives"]=json!(alternatives);
    policy["taskOutcomeForecast"]=winner["outcomeForecast"].clone();
    policy["forecast"]["acceptanceProbability"]=
        winner["predictedAdoptionProbability"].clone();
    policy["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    // The selected target must also become the native action, not just a
    // ranking comment. The coordinator still owns independent evaluation.
    for (key,field) in [("role","role"),("domain","domain"),
        ("path","path"),("sourceSha256","sourceSha256")]{
        policy["controlPolicy"][key]=target[field].clone();
    }
    let mutual=policy["mutualDevelopmentGraph"]["edges"].as_array()
        .into_iter().flatten()
        .find(|edge|edge["to"]==target["role"])
        .cloned().unwrap_or(Value::Null);
    policy["nextMutualRedesign"]=mutual;
    policy["peerMethodTransfer"]=json!({
        "source":"compiled_cross_lineage_method_hypothesis_v1",
        "selectedRecipient":target,
        "references":winner["transferHypotheses"],
        "comparisonProfileSha256":peers["comparisonProfileSha256"],
        "activeDonorCandidateId":winner["activeTransferDonorCandidateId"],
        "attributionMode":"one_executed_donor_source_revision_and_suite",
        "active":bridge,
        "sameFrozenComparisonProfile":true,
        "automaticallyApplied":false,
        "transferProven":false
    });
    if bridge{
        policy["mode"]=json!("test_cross_role_portable_method");
    }
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([
            original_revision,policy["target"],policy["alternatives"],
            policy["controlPolicy"],policy["peerMethodTransfer"],
            policy["peerMethodLearning"]
        ])).unwrap_or_default())));
    policy
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
    let mut message=format!(
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
    );
    if policy["peerMethodTransfer"]["active"]==true {
        let rows=policy["peerMethodTransfer"]["references"].as_array()
            .into_iter().flatten()
            .filter(|r|r["candidateId"]==policy["peerMethodTransfer"]["activeDonorCandidateId"])
            .take(1)
            .map(|r|format!(
                "isolated donor={} source roles={} target={} current file={} \
                 measured recipient trials={} actual source adoptions={} \
                 smoothed portability={} matched control trials={} control adoptions={} negative transfer={}",
                r["candidateId"],r["fromRoles"],r["toRole"],
                r["toSourcePath"],r["independentlyMeasuredTransferTrials"],
                r["recipientSourceAdoptions"],r["empiricalTransferPosterior"],
                r["matchedControlTrials"],r["matchedControlAdoptions"],
                r["negativeTransferSignal"]
            )).collect::<Vec<_>>();
        message.push_str(&format!(
            "\nPORTABLE METHOD HYPOTHESIS (not a performance claim): \
             re-derive the improvement METHOD from these independently paired \
             donor experiments and test it on the newly chosen component. \
             Do not reuse their patch or label this transfer successful. \
             Make a new source edit, predict the result, and allow the frozen \
             evaluator to measure it:\n{}\n",rows.join("\n")
        ));
    }
    let negative=policy["peerMethodLearning"]["observations"].as_array()
        .into_iter().flatten()
        .filter(|row|row["negativeTransferSignal"]==true)
        .take(4)
        .map(|row|format!("donor={} toRole={} transfer {}/{} vs matched non-transfer {}/{}",
            row["donorCandidateId"],row["recipientRole"],
            row["adoptedTransferTrials"],row["observedTransferTrials"],
            row["matchedControlAdoptions"],row["matchedControlTrials"]))
        .collect::<Vec<_>>();
    if !negative.is_empty(){
        message.push_str(&format!(
            "\nOBSERVATIONAL NEGATIVE TRANSFER (not causal proof):\n{}\n\
             Prefer redesigning the method or testing a clean alternative; \
             do not reward a failing foreign method merely for being cited.\n",
             negative.join("\n")));
    }
    message
}

/// Bounded, independently evaluated ideas from compatible foreign lineages.
pub(crate) fn peer_instruction(peers:&Value,policy:&Value)->String {
    // The non-transfer control must not see foreign method prompts.
    // Transferred variants see only the one assigned, source-matched donor.
    if policy["peerMethodTransfer"]["active"]!=true
        || peers["comparisonProfileSha256"]
            !=policy["peerMethodTransfer"]["comparisonProfileSha256"] {
        return String::new();
    }
    let Some(donor)=policy["peerMethodTransfer"]["activeDonorCandidateId"].as_str()
        else{return String::new()};
    let lines=peers["references"].as_array().into_iter().flatten()
        .filter(|row|row["candidateId"]==donor
            && row["comparisonProfileSha256"]==peers["comparisonProfileSha256"])
        .take(1)
        .map(|row|format!(
            "single assigned donor={} roles={} hypothesis={} design={} aligned={}",
            row["campaignId"],row["roles"],row["hypothesis"],
            row["designSummary"],row["alignedSourceFiles"]))
        .collect::<Vec<_>>();
    if lines.is_empty(){String::new()}else{
        format!("\nISOLATED PEER METHOD REFERENCE (ideas only, never automatic patch reuse):\n{}\n",
            lines.join("\n"))
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
    // A peer-guided *attempt* is distinct from both donor success and
    // recipient adoption. Future generations learn method portability only
    // from attempts that actually reached the chosen indexed source.
    let transfer=&policy["peerMethodTransfer"];
    let assigned_donor=transfer["activeDonorCandidateId"].as_str()
        .filter(|id|!id.is_empty());
    let matching_assignment=assigned_donor.is_some_and(|id|
        generation["metaPolicy"]["peerMethodTransfer"]["activeDonorCandidateId"]==id
            && generation["metaPolicy"]["revision"]==policy["revision"]
            && transfer["references"].as_array().is_some_and(|refs|
                refs.iter().any(|row|row["candidateId"]==id)));
    let source_focus=transfer["active"]==true && matching_assignment
        && transfer["comparisonProfileSha256"]==generation["comparisonProfileSha256"]
        && transfer["selectedRecipient"]["role"].as_str().is_some_and(|role|
            transfer["selectedRecipient"]["path"].as_str().is_some_and(|path|
                transfer["selectedRecipient"]["sourceSha256"].as_str().is_some_and(
                    |sha|executed_recipient_context(generation,role,path,sha,
                        &transfer["comparisonProfileSha256"]))));
    let source_adopted=source_focus && transfer["selectedRecipient"]["path"].as_str()
        .zip(transfer["selectedRecipient"]["sourceSha256"].as_str())
        .is_some_and(|(path,sha)|recipient_source_adopted(generation,path,sha));
    let transfer_observation=json!({
        "source":"measured_cross_lineage_recipient_observation_v1",
        "hypothesisRevision":policy["revision"],
        "fromPeerCandidates":transfer["references"].as_array()
            .into_iter().flatten()
            .filter_map(|row|row["candidateId"].as_str())
            .take(4).collect::<Vec<_>>(),
        "recipient":transfer["selectedRecipient"],
        "attributableDonorCandidateId":if source_focus{json!(assigned_donor)}else{Value::Null},
        "singleDonorSourceSuiteMatched":source_focus,
        "sourceFocusExecuted":source_focus,
        "independentlyAssessed":source_focus && assessed,
        "recipientSourceIndependentlyAdopted":source_adopted,
        "portableMethodSuperiorityProven":false,
        "automaticPatchReuse":false
    });
    json!({
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "portableMethodObservation":transfer_observation,
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
    fn portable_peer_method_changes_executable_recipient_not_adoption_credit(){
        let target=|role:&str,domain:&str,path:&str|json!({
            "role":role,"domain":domain,"path":path,
            "sourceSha256":"a".repeat(64),"sourceBytes":1200
        });
        let a=target("memory","memory","src/engine/memory.rs");
        let b=target("learning","learning","src/engine/learning.rs");
        let original=json!({
            "source":"compiled_editable_meta_controller_v1",
            "revision":"prior","target":a,
            "controlPolicy":{"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceSha256":"a".repeat(64)},
            "alternatives":[
                {"role":"memory","score":1.0,"target":a,
                    "outcomeForecast":{"adoption":0.5},
                    "predictedAdoptionProbability":0.5,"uncertainty":0.2},
                {"role":"learning","score":0.95,"target":b,
                    "outcomeForecast":{"adoption":0.25},
                    "predictedAdoptionProbability":0.25,"uncertainty":0.3}
            ],
            "forecast":{"acceptanceProbability":0.5,"uncertainty":0.2}
        });
        let peer=json!({
            "source":"paired_cross_campaign_reference_archive_v1",
            "automaticPatchReuse":false,
            "comparisonProfileSha256":"f".repeat(64),
            "references":[{
                "source":"paired_cross_campaign_design_reference_v1",
                "automaticPatchReuse":false,
                "comparisonProfileSha256":"f".repeat(64),
                "campaignId":"other","candidateId":"verified-1",
                "acceptedCommit":"b".repeat(40),
                "patchSha256":"c".repeat(64),
                "nextDomain":"learning","roles":["memory"],
                "identicalSourceFiles":1,
                "alignedSourceFiles":["src/engine/memory.rs"]
            }]
        });
        let evolved=evolve_from_peer_methods(original.clone(),&peer,&[]);
        assert_eq!(evolved["target"]["role"],"learning");
        assert_eq!(evolved["controlPolicy"]["role"],"learning");
        assert_eq!(evolved["controlPolicy"]["path"],"src/engine/learning.rs");
        assert_eq!(evolved["forecast"]["acceptanceProbability"],0.25);
        assert_eq!(evolved["taskOutcomeForecast"]["adoption"],0.25);
        assert_eq!(evolved["peerMethodTransfer"]["active"],true);
        assert_eq!(evolved["peerMethodTransfer"]["automaticallyApplied"],false);
        assert_eq!(evolved["peerMethodTransfer"]["transferProven"],false);
        let observation=feedback(&evolved,&json!({
            "metaPolicyExecuted":true,
            "comparisonProfileSha256":"f".repeat(64),
            "metaPolicy":evolved.clone(),
            "focusFiles":["src/engine/learning.rs"],
            "candidateId":"recipient-1",
            "accepted":false,
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}}
        }));
        assert_eq!(observation["portableMethodObservation"]
            ["sourceFocusExecuted"],true);
        assert_eq!(observation["portableMethodObservation"]
            ["recipientSourceIndependentlyAdopted"],false);
        assert_eq!(observation["portableMethodObservation"]
            ["portableMethodSuperiorityProven"],false);
        assert!(instruction(&evolved).contains("PORTABLE METHOD HYPOTHESIS"));
        assert!(peer_instruction(&peer,&original).is_empty());
        let single_peer=peer_instruction(&peer,&evolved);
        assert!(single_peer.contains("ISOLATED PEER METHOD REFERENCE"));
        assert!(single_peer.contains("peer=other") || single_peer.contains("donor=other"));
        let mut second_peer=peer.clone();
        second_peer["references"].as_array_mut().unwrap().push(json!({
            "campaignId":"unassigned","candidateId":"other-candidate",
            "comparisonProfileSha256":"f".repeat(64),
            "hypothesis":"must not leak into isolated prompt"
        }));
        assert!(!peer_instruction(&second_peer,&evolved)
            .contains("must not leak into isolated prompt"));
        assert_ne!(evolved["revision"],original["revision"]);
        let mut drift=peer.clone();
        drift["references"][0]["identicalSourceFiles"]=json!(0);
        let ignored=evolve_from_peer_methods(original.clone(),&drift,&[]);
        assert_eq!(ignored["target"],original["target"]);
        let mut wrong_suite=peer.clone();
        wrong_suite["references"][0]["comparisonProfileSha256"]=
            json!("0".repeat(64));
        assert_eq!(evolve_from_peer_methods(original.clone(),&wrong_suite,&[])["target"],
            original["target"]);
        // Four independently assessed non-adoptions lower the donor-method
        // posterior; the child returns to the non-transfer candidate.
        let rejected=(0..4).map(|i|json!({
            "jobId":format!("transfer-{i}"),
            "metaPolicyExecuted":true,
            "comparisonProfileSha256":"f".repeat(64),
            "focusFiles":["src/engine/learning.rs"],
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}},
            "metaPolicy":{
                "target":{"role":"learning","path":"src/engine/learning.rs",
                    "sourceSha256":"a".repeat(64)},
                "peerMethodTransfer":{
                "active":true,"activeDonorCandidateId":"verified-1",
                "comparisonProfileSha256":"f".repeat(64),
                "selectedRecipient":{"role":"learning",
                    "path":"src/engine/learning.rs","sourceSha256":"a".repeat(64)},
                "references":[{"candidateId":"verified-1",
                    "sharedFrozenProfileSha256":"f".repeat(64)}]
            }}
        })).collect::<Vec<_>>();
        let later=evolve_from_peer_methods(original.clone(),&peer,&rejected);
        assert_eq!(later["target"]["role"],"memory");
        let alternative=later["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(alternative["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],4);
        assert_eq!(alternative["transferHypotheses"][0]
            ["recipientSourceAdoptions"],0);
        // An execution referencing two donors may only credit the
        // explicitly assigned one, never each name in the prompt.
        let mut ambiguous=rejected.clone();
        for row in &mut ambiguous{
            row["metaPolicy"]["peerMethodTransfer"]["references"]
                .as_array_mut().unwrap().push(json!({
                    "candidateId":"verified-other",
                    "sharedFrozenProfileSha256":"f".repeat(64)
                }));
            row["metaPolicy"]["peerMethodTransfer"]["activeDonorCandidateId"]=
                json!("verified-other");
        }
        let unmatched=evolve_from_peer_methods(original.clone(),&peer,&ambiguous);
        let unmatched_bridge=unmatched["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(unmatched_bridge["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],0);
        let mut wrong_source=rejected.clone();
        for row in &mut wrong_source{
            row["metaPolicy"]["target"]["sourceSha256"]=json!("b".repeat(64));
        }
        let source_mismatch=evolve_from_peer_methods(original.clone(),&peer,&wrong_source);
        let source_bridge=source_mismatch["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(source_bridge["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],0);
        let mut wrong_profile=rejected.clone();
        for row in &mut wrong_profile{
            row["comparisonProfileSha256"]=json!("e".repeat(64));
        }
        let suite_mismatch=evolve_from_peer_methods(original.clone(),&peer,&wrong_profile);
        let suite_bridge=suite_mismatch["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(suite_bridge["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],0);
        let mut failed_receipt=json!({
            "metaPolicyExecuted":true,
            "metaPolicy":evolved.clone(),
            "comparisonProfileSha256":"e".repeat(64),
            "focusFiles":["src/engine/learning.rs"],
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}}
        });
        assert_eq!(feedback(&evolved,&failed_receipt)["portableMethodObservation"]
            ["independentlyAssessed"],false);
        failed_receipt["comparisonProfileSha256"]=json!("f".repeat(64));
        assert_eq!(feedback(&evolved,&failed_receipt)["portableMethodObservation"]
            ["attributableDonorCandidateId"],"verified-1");
        // Four matched non-transfer adoptions against four donor failures
        // suppress transfer and preserve the negative evidence for method learning.
        let controls=(0..4).map(|i|{
            let job=format!("nontransfer-{i}");
            json!({
                "jobId":job,"generatorCommit":"d".repeat(40),
                "metaPolicyExecuted":true,
                "comparisonProfileSha256":"f".repeat(64),
                "metaPolicy":{"target":{"role":"learning",
                    "path":"src/engine/learning.rs",
                    "sourceSha256":"a".repeat(64)}},
                "focusFiles":["src/engine/learning.rs"],
                "candidateOrigin":"model_driven_detached_worktree",
                "accepted":true,"pairedRevalidated":true,
                "acceptedCommit":"b".repeat(40),
                "acceptedSourceHashes":{"src/engine/learning.rs":"c".repeat(64)},
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":"d".repeat(40),
                    "generationJobId":job,"component":"learning",
                    "targetFiles":["src/engine/learning.rs"]},
                "evaluation":{"verdict":"accepted","metrics":{"eligible":true}}
            })
        }).collect::<Vec<_>>();
        assert!(controls.iter().all(independent_adoption));
        let mut matched=rejected.clone();
        matched.extend(controls);
        let suppressed=evolve_from_peer_methods(original.clone(),&peer,&matched);
        assert_eq!(suppressed["target"]["role"],"memory");
        assert_eq!(suppressed["peerMethodLearning"]["observations"][0]
            ["negativeTransferSignal"],true);
        assert!(instruction(&suppressed).contains("OBSERVATIONAL NEGATIVE TRANSFER"));
        assert!(!peer_instruction(&peer,&suppressed).contains("ISOLATED"));
        let mut repair=original.clone();
        repair["metaRepairChosen"]=json!(true);
        assert_eq!(evolve_from_peer_methods(repair.clone(),&peer,&[])["target"],
            repair["target"]);
    }

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
