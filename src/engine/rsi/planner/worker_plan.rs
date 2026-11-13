//! planner worker plan — extracted RSI responsibility, sharing only parent interfaces.
use super::*;

/// Executes only one durable generation, without starting a server, scheduler,
/// migrations or recovering another process's jobs in the shared ledger.
/// Run the next plan with the newly compiled generation's own policy.
pub async fn worker_plan(campaign_id:&str)->Result<(),String>{
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let campaign=state.store.get("rsi_campaign",campaign_id).ok_or("후손 계획 기록 없음")?;
    let parent=state.store.get("job",campaign_id).ok_or("후손 계획 작업 없음")?;
    let revision=option_env!("DOMINION_BUILD_COMMIT").ok_or("후손 빌드 revision 없음")?;
    if parent["kind"]!="rsi_evolve" || parent["status"]!="running"
        || parent["cancelRequested"]==true
        || parent["input"]["rsiSystemEvolution"]!=true
        || campaign["headCommit"]!=revision
        || campaign["planAttempt"].as_u64().is_none() {
        return Err("후손 계획의 권한·빌드 revision·캠페인 상태 불일치".into());
    }
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]==campaign_id).collect::<Vec<_>>();
    history.sort_by_key(|row|row["attempt"].as_u64().unwrap_or(0));
    if campaign["history"].as_array().is_none_or(|rows|rows.len()!=history.len())
        || campaign["planAttempt"].as_u64()!=Some(history.len() as u64+1) {
        return Err("후손 계획과 저장된 세대 계보 불일치".into());
    }
    let history=crate::engine::rsi_system::revalidated_history(&state,&history);
    let objective=campaign["planObjective"].as_str()
        .map(str::to_string).unwrap_or_else(||selection_objective(&parent["input"]));
    let inherited=if parent["input"]["selectionPolicy"]=="automatic"{
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    let lab=campaign["labDir"].as_str().ok_or("후손 소스 경로 없음")?;
    let blueprint=crate::engine::rsi_system::blueprint(Path::new(lab),revision,&history);
    let mut frontier=crate::engine::rsi_system::architecture_frontier(
        &state,&blueprint,&history,campaign["planIndexId"].as_str().unwrap_or(""),Path::new(lab));
    crate::engine::rsi_recursive_rewrite::extend_frontier(&blueprint,&mut frontier);
    // This meta-controller lives in the REBUILT BINARY. The frozen previous
    // coordinator cannot overwrite its comparative learning or target.
    let permitted=candidate_domains(&objective);
    let mut peer_input=parent["input"].clone();
    peer_input["rsiIndexId"]=campaign["planIndexId"].clone();
    let peers=crate::engine::rsi_lineage_transfer::peers(
        &state,campaign_id,&peer_input,
        campaign["planIndexId"].as_str().unwrap_or(""),None);
    let raw_meta=crate::engine::rsi_meta_controller::synthesize(
        &history,&frontier,&permitted,inherited.as_deref());
    let transferred_meta=crate::engine::rsi_meta_controller::integrate_peer_evidence(
        raw_meta,&peers);
    let recursive_graph=crate::engine::rsi_symbiosis::transition_graph(&history);
    let transferred_meta=crate::engine::rsi_symbiosis::apply_reciprocal_feedback(
        transferred_meta,&recursive_graph);
    let proposed_stage=crate::engine::rsi_system::inherited_stage(&history,&frontier);
    // Owner-constrained goals cannot be hijacked by older optional stages.
    let stage=if proposed_stage["domain"].as_str()
        .is_some_and(|domain|permitted.contains(&domain)){
        proposed_stage
    }else{Value::Null};
    let inherited_meta=crate::engine::rsi_meta_controller::apply_successor_stage(
        transferred_meta,&stage);
    let auto=parent["input"]["selectionPolicy"]=="automatic";
    let inherited_meta=crate::engine::rsi_recursive_rewrite::reconfigure(
        &history,&frontier,inherited_meta,&permitted,auto,stage.is_object());
    let inherited_meta=crate::engine::rsi_principle_learning::recompose(
        &history,&frontier,inherited_meta,&permitted,auto,stage.is_object());
    // A statistically triggered self-repair must be the actual next source
    // experiment, not a forecast silently superseded by another scheduler.
    let inherited_meta=crate::engine::rsi_prequential::arbitrate_repair(
        inherited_meta,&frontier,&permitted,auto,stage.is_object());
    let initial_symbiosis=crate::engine::rsi_symbiosis::curriculum(
        &history,&frontier,&inherited_meta,&permitted);
    let profile=crate::engine::rsi_lineage_transfer::profile(&peer_input);
    let inherited_experiment_method=
        crate::engine::rsi_intervention_lab::inherited_method(&history);
    let proposed_factorial=crate::engine::rsi_intervention_lab::design(
        &history,&frontier,&inherited_meta,&initial_symbiosis,
        &permitted,revision,profile.as_deref(),&inherited_experiment_method);
    // An active single-source experiment must not starve open-ended
    // inventions. Continue measured sibling interventions on this EXACT
    // source revision; otherwise allocate every third task to experiments.
    // A model-authored adopted experimental method can change this balance.
    let measured_siblings=proposed_factorial["selected"]["evidence"]
        ["candidateCount"].as_u64().unwrap_or(0);
    let method_promoted=inherited_experiment_method["source"]
        =="model_authored_causal_experiment_policy_v1";
    // Unknown contribution in a jointly adopted redesign is a reason to
    // isolate components on the next frozen profile, not to award every role
    // the full success or continue exploiting an ambiguous result.
    let isolation_debt=inherited_meta["selectionEvidence"]["causalCredit"]
        ["requiresIsolation"]==true;
    let factorial_active=auto && stage.is_null()
        // A learned principle experiment has to execute on its selected
        // source; otherwise its downstream payoff can never be observed.
        && inherited_meta["principleLearning"]["active"]!=true
        && inherited_meta["predictionRuleRepair"]["active"]!=true
        // Complete an actual descendant meta-code rewrite before allowing
        // a speculative factorial schedule to replace its source target.
        && !(inherited_meta["recursiveProgram"]["active"]==true
            && inherited_meta["recursiveProgram"]["phase"]=="rewrite_meta_method")
        && parent["input"]["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty)
        && proposed_factorial["selected"].is_object()
        && (measured_siblings>0 || history.len()%3==1
            || method_promoted || isolation_debt);
    // An explicit owner objective or a previously adopted successor program
    // must NOT inherit file-write restrictions from an unused experiment.
    // Null means no actual factorial intervention was scheduled.
    let factorial=if factorial_active{proposed_factorial}else{Value::Null};
    let mut meta=if factorial_active {
        crate::engine::rsi_intervention_lab::steer(inherited_meta,&factorial)
    }else{inherited_meta};
    // An independent factorial trial can legitimately displace a planned
    // invention-to-meta action. Do not record an action as EXECUTED when
    // the concrete source target was changed by that other scheduler.
    if meta["recursiveProgram"]["active"]==true
        && meta["target"]!=meta["recursiveProgram"]["selectedMethodOrCapability"] {
        let prior=meta["revision"].clone();
        meta["recursiveProgram"]["active"]=json!(false);
        meta["recursiveProgram"]["deferredBy"]=json!("source_matched_factorial_trial");
        meta["revision"]=json!(format!("{:x}",Sha256::digest(
            serde_json::to_vec(&json!([prior,meta["recursiveProgram"],meta["target"]]))
                .unwrap_or_default())));
    }
    // A later empirically selected principle supersedes only the targeting
    // action, not an already independently measured recursive source receipt.
    if meta["principleLearning"]["active"]==true
        && meta["recursiveProgram"]["active"]==true
        && meta["recursiveProgram"]["selectedMethodOrCapability"]!=meta["target"]{
        meta["recursiveProgram"]["active"]=json!(false);
        meta["recursiveProgram"]["deferredBy"]=json!("principle_generalization_trial");
    }
    let architecture_target=meta["target"].clone();
    // The same compiled child can rewrite its coupling search algorithm.
    let symbiosis=crate::engine::rsi_symbiosis::curriculum(
        &history,&frontier,&meta,&permitted);
    let selected=if auto{
        architecture_target["domain"].as_str()
            .map(str::to_string).or(inherited.clone())
    }else{inherited.clone()};
    let (domain,selection,strategy)=select_generation_strategy(
        &state,&history,&objective,None,None,selected,revision);
    let model_policy=crate::engine::rsi_model_ecology::plan(
        &state,&history,&parent["input"],&domain,revision);
    // The archive search law runs in THIS rebuilt descendant binary.
    // A previous-generation coordinator may verify the selected parent's
    // original evidence but cannot silently replace its learned ranking.
    let archive_selection=crate::engine::rsi_archive::select(
        &state,&history,revision,&parent["input"]["rsiFrozenCases"]);
    let mut focus=parent["input"].clone();
    focus["rsiTargetDomain"]=json!(domain);
    focus["rsiIndexId"]=campaign["planIndexId"].clone();
    let policy=strategy["id"].as_str()
        .and_then(|id|state.store.get("algorithm",id))
        .or_else(||crate::engine::algorithms::active(&state,"selfimprove.search"));
    let mut ranked=if focus["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
        choose_focus_files(&state,&focus,&history,policy.as_ref())
    }else{Vec::new()};
    if auto && architecture_target["domain"]==domain
        && focus["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
        if let (Some(path),Some(hash))=(
            architecture_target["path"].as_str(),
            architecture_target["sourceSha256"].as_str()) {
            ranked.retain(|item|item["path"]!=path);
            ranked.insert(0,json!({"path":path,"sourceHash":hash,
                "source":"compiled_descendant_architecture_frontier",
                "role":architecture_target["role"],
                "sourceIndexId":campaign["planIndexId"]}));
            ranked.truncate(3);
        }
    }
    let comparative_mode=if peers["references"].as_array().is_some_and(|v|!v.is_empty()){
        "cross_lineage_design"
    }else if meta["counterexamples"].as_array().is_some_and(|v|v.len()>=2){
        "multi_trajectory_design"
    }else{"local_redesign"};
    if stage["source"]=="rebound_verified_model_successor_program_v1"
        && ranked.first().is_some_and(|row|
            row["path"]==stage["path"] && row["sourceHash"]==stage["sourceSha256"]){
        ranked.truncate(1);
    }
    if auto && stage.is_null() && !factorial_active
        && symbiosis["complement"]["fromRole"]==architecture_target["role"]
        && symbiosis["primary"]["domain"]==domain {
        let partner=&symbiosis["complement"];
        if let (Some(path),Some(hash))=(partner["toPath"].as_str(),
            partner["toSha256"].as_str()){
            if ranked.iter().any(|row|row["path"]==architecture_target["path"]) {
                ranked.retain(|row|row["path"]!=path);
                ranked.insert(1,json!({"path":path,"sourceHash":hash,
                    "source":"compiled_bidirectional_intelligence_curriculum",
                    "role":partner["toRole"],
                    "sourceIndexId":campaign["planIndexId"]}));
                ranked.truncate(4);
            }
        }
    }
    // Turn an information-gain experiment into exact executable edit scope.
    // It is invalid as factorial evidence if the model edits outside scope;
    // normal independent adoption remains a separate decision.
    if factorial_active && architecture_target["domain"]==domain
        && focus["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
        let selected=&factorial["selected"];
        let targets=if selected["phase"]=="test_joint" {
            vec![&selected["a"],&selected["b"]]
        } else {vec![&selected["target"]]};
        ranked=targets.into_iter().map(|row|json!({
            "path":row["path"],"role":row["role"],
            "sourceHash":row["sourceSha256"],
            "source":"compiled_open_factorial_intervention",
            "sourceIndexId":campaign["planIndexId"]
        })).collect();
    }
    let mut descendant_policy=learned_edit_method(&state,&focus,&history);
    if meta["predictionRuleRepair"]["active"]==true {
        descendant_policy["checkpointAfter"]=json!(2);
        descendant_policy["preferredTool"]=json!("workspace_replace_range");
        descendant_policy["predictionMethodRewriteSource"]=
            meta["predictionRuleRepair"]["executionTarget"]["path"].clone();
    }
    if meta["recursiveProgram"]["active"]==true
        && meta["recursiveProgram"]["phase"]=="rewrite_meta_method" {
        descendant_policy["checkpointAfter"]=json!(2);
        descendant_policy["preferredTool"]=json!("workspace_replace_range");
        descendant_policy["recursiveRewriteSource"]=
            meta["recursiveProgram"]["nextRequiredSourceChange"].clone();
    }
    if stage["source"]=="rebound_verified_model_successor_program_v1" {
        if stage["executionPolicy"]=="grounded_edit" {
            descendant_policy["checkpointAfter"]=json!(2);
            descendant_policy["preferredTool"]=json!("workspace_replace_range");
        }else{
            descendant_policy["checkpointAfter"]=json!(4);
        }
        descendant_policy["successorDesignId"]=stage["designId"].clone();
        descendant_policy["selectedBySuccessorStage"]=stage["stageIndex"].clone();
    }
    let plan=json!({
        "source":"compiled_descendant_system_plan_v1",
        "generatorCommit":revision,"campaignId":campaign_id,
        "attempt":campaign["planAttempt"],"historyCount":history.len(),
        "indexId":campaign["planIndexId"],
        "domain":domain,"selection":selection,"strategy":strategy,
        "rankedFiles":ranked,"systemBlueprint":blueprint,
        "architectureFrontier":frontier,
        "architectureTarget":architecture_target,
        "archiveSelection":archive_selection,
        "modelPolicy":model_policy,
        "metaPolicy":meta,
        "peerLineages":peers,"comparativeMode":comparative_mode,
        "successorStage":stage,"editPolicy":descendant_policy,
        "symbioticCurriculum":symbiosis,
        "factorialExperiment":factorial,
        "sourceTreeSha256":blueprint["sourceTree"]["sourceTreeSha256"],
        "decisionOwner":"rebuilt_compiled_descendant"
    });
    println!("{plan}");
    Ok(())
}
