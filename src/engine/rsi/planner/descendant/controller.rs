//! Compiled descendant planning pipeline. Source-editable strategy synthesis
//! and target resolution, separate from planner input/target reconciliation.
use super::*;

pub fn plan_generation_from_file(path:&str)->Result<(),String> {
    let snapshot:Value=serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?)
        .map_err(|e|e.to_string())?;
    let commit=snapshot["generatorCommit"].as_str().ok_or("계획 생성기 버전이 없습니다.")?;
    if option_env!("DOMINION_BUILD_COMMIT")!=Some(commit) {
        return Err("계획 생성기 바이너리와 소스 커밋이 다릅니다.".into());
    }
    let input=&snapshot["input"];
    if input["rsiSystemEvolution"]!=true {
        return Err("전체 시스템 진화 작업만 후손 계획을 호출할 수 있습니다.".into());
    }
    let campaign=input["rsiCampaignId"].as_str().ok_or("계획 캠페인 ID가 없습니다.")?;
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]==campaign).collect::<Vec<_>>();
    history.sort_by_key(|row|row["attempt"].as_u64().unwrap_or(0));
    let history=crate::engine::rsi_system::revalidated_history(&state,&history);
    let source=input["rsiSourceDir"].as_str()
        .ok_or("후손 시스템 소스 위치가 없습니다.")?;
    let source_root=Path::new(source);
    let system_blueprint=crate::engine::rsi_system::blueprint(
        source_root,commit,&history);
    // Planning intelligence belongs to the newly compiled descendant.
    // The former coordinator never recomputes this source-grounded ranking.
    let frontier=crate::engine::rsi_system::architecture_frontier(
        &state,&system_blueprint,&history,
        input["rsiIndexId"].as_str().unwrap_or(""),source_root);
    let owner_focus=matches!(
        input["rsiFocusSelection"]["source"].as_str(),
        Some("owner_selected"|"user_selected"));
    let paired=paired_descendant_target(input);
    let peers=crate::engine::rsi_meta_controller::peer_lineages(
        &state,campaign,input,None);
    let interaction_graph=crate::engine::rsi_interaction::influence_graph(
        &history,&frontier);
    let negative_transfer=crate::engine::rsi_negative_transfer::plan(
        &state,&history,&frontier,commit);
    let authored_algebra=crate::engine::rsi_improvement_algebra::compile(
        &history,&frontier,commit);
    let method_learning=crate::engine::rsi_method_generalization::learn(
        &state,&history,&frontier,&authored_algebra,commit);
    let algebra=crate::engine::rsi_method_generalization::adapt(
        authored_algebra,&method_learning);
    let model_evolution=crate::engine::rsi_model_lineage::plan(
        &state,&history,input,commit)?;
    let science_active=input["rsiAutonomous"]==true && !owner_focus && !paired;
    // A measured B1 control or a newly rewritten A2 method reserves the
    // immediate successor for the next source-grounded intervention. Other
    // diagnostic programs remain available once this trial has completed.
    let controlled_transition=science_active && matches!(
        algebra["selected"]["intent"].as_str(),
        Some("evolve_measured_method_after_controlled_downstream_gain"
            |"run_revised_method_in_controlled_descendant"));
    let predesign=crate::engine::rsi_evolve::meta_experiment_policy::synthesize(
        &history,&frontier,&algebra,input,commit);
    let redesign=crate::engine::rsi_evolve::meta_experiment_policy::design_target(
        &predesign,commit,input);
    let preferred_source=crate::engine::rsi_evolve::meta_experiment_policy::preferred_focus(
        &predesign,commit,input);
    let open_source_focus=preferred_source.as_ref().and_then(|target|
        frontier.iter().find(|row|row["path"]==target["path"]
            && row["sourceSha256"]==target["sourceSha256"])).cloned();
    let meta_redesign_target=redesign.as_ref().and_then(|target|
        frontier.iter().find(|source|source["path"]==target["path"]
            && source["sourceSha256"]==target["sourceSha256"])).cloned();
    let meta_redesign_active=science_active && !controlled_transition
        && meta_redesign_target.is_some();
    let mut negative_transfer_active=science_active
        && !controlled_transition && !meta_redesign_active
        && negative_transfer["selected"]["victim"].is_object()
        && negative_transfer["selected"]["source"].is_object();
    let mut capability_active=science_active && !meta_redesign_active
        && !negative_transfer_active
        && crate::engine::rsi_method_generalization::prefer_capability_target(
            &algebra,&method_learning);
    let mut algebra_active=science_active && !meta_redesign_active
        && !negative_transfer_active
        && !capability_active && algebra["selected"]["target"].is_object();
    let raw_meta=crate::engine::rsi_meta_controller::synthesize(
        &history,&frontier,DOMAINS,
        crate::engine::rsi_system::inherited_domain(&history).as_deref());
    // Genuine isolated cross-role effects reshape the editable meta
    // algorithm BEFORE peer references or reciprocal curricula are ranked.
    let holdout_meta=crate::engine::rsi_interaction::reinforce_meta_with_holdout(
        raw_meta,&interaction_graph);
    let peer_meta=crate::engine::rsi_meta_controller::integrate_peer_evidence(
        holdout_meta,&peers);
    let reciprocal_graph=crate::engine::rsi_symbiosis::transition_graph(&history);
    let meta=crate::engine::rsi_symbiosis::apply_reciprocal_feedback(
        peer_meta,&reciprocal_graph);
    let suggested_stage=if input["rsiAutonomous"]==true && !owner_focus
        && !paired && !meta_redesign_active && !negative_transfer_active
        && !algebra_active && !capability_active {
        crate::engine::rsi_system::inherited_stage(&state,&history,&frontier)
    }else{Value::Null};
    let meta_policy=crate::engine::rsi_meta_controller::apply_successor_stage(
        meta,&suggested_stage);
    let successor_stage=if suggested_stage.is_object()
        && meta_policy["successorStage"]==suggested_stage {
        suggested_stage
    }else{Value::Null};
    // On sustained verified prediction drift, the compiled child must
    // actually try revising its prediction implementation before unrelated
    // exploratory curricula overwrite the chosen source.
    let predictor_repair_active=science_active && !controlled_transition
        && !meta_redesign_active
        && meta_policy["predictionRuleRepair"]["repairRuleSourceSelected"]==true
        && meta_policy["target"]["path"]=="src/engine/rsi_prequential.rs";
    if predictor_repair_active {
        negative_transfer_active=false;
        capability_active=false;
        algebra_active=false;
    }
    let mut architecture_target=if input["rsiAutonomous"]==true
        && !owner_focus && !paired && meta_policy["target"].is_object() {
        meta_policy["target"].clone()
    }else{frontier.first().cloned().unwrap_or(Value::Null)};
    // Frozen holdout measurements and actual candidate paths provide a
    // complementary graph to the adopted-generation A→B→A lineage. Joint
    // edits remain confounded, never evidence of a single role's causal gain.
    let coupled_portfolio=if input["rsiAutonomous"]==true
        && !meta_redesign_active && !owner_focus && !paired
        && architecture_target.is_object() {
        crate::engine::rsi_interaction::coupled_portfolio(
            &architecture_target,&frontier,&interaction_graph,DOMAINS)
    }else{Value::Null};
    // The recursive scientist does not merely collect observations.
    // It decides whether to isolate a missing contrast, invent a new
    // executable intelligence role, return an invented capability to its
    // origin, or rewrite the scientific improvement policy itself.
    let factorial=crate::engine::rsi_factorial::design(
        &history,&frontier,&interaction_graph,DOMAINS,commit);
    let science=crate::engine::rsi_recursive_scientist::plan(
        &history,&frontier,&factorial,&coupled_portfolio,commit);
    // Independently adopted model-authored improvement operators can compose
    // across any number of roles; no fixed scientist stage is needed.
    if meta_redesign_active {
        architecture_target=meta_redesign_target.clone().unwrap_or(Value::Null);
    }else if predictor_repair_active {
        architecture_target=meta_policy["target"].clone();
    }else if negative_transfer_active {
        architecture_target=negative_transfer["selected"]["victim"].clone();
    }else if algebra_active {
        architecture_target=algebra["selected"]["target"].clone();
    }else if capability_active {
        architecture_target=method_learning["suggestedCapabilityTarget"].clone();
    }else if science_active && science["phase"]=="discover_new_mechanism"
        && open_source_focus.is_some() {
        // The policy may change source targets with ANY role in the current
        // actual topology. A self-improving experimental designer changes
        // this exploration algorithm in its own next compiled revision.
        architecture_target=open_source_focus.clone().unwrap_or(Value::Null);
    }else if science_active && !predictor_repair_active
        && science["phase"]!="discover_new_mechanism" {
        if let Some(first)=science["focus"].as_array().and_then(|f|f.first()){
            architecture_target=first.clone();
        }
    }
    // Cross-lineage methods influence actual source search ONLY when the
    // scientific program or predictor repair has not reserved a different
    // descendant experiment. Exactly one donor can be active.
    let mut meta_policy=meta_policy;
    if science_active && science["phase"]=="discover_new_mechanism"
        && !negative_transfer_active && !algebra_active && !capability_active
        && successor_stage.is_null() && architecture_target==meta_policy["target"] {
        meta_policy=crate::engine::rsi_peer_transfer::learn(
            meta_policy,&peers,&history);
        architecture_target=meta_policy["target"].clone();
    }
    let symbiosis=crate::engine::rsi_symbiosis::curriculum(
        &history,&frontier,&json!({"target":architecture_target}),DOMAINS);
    let inherited=if input["rsiAutonomous"]==true {
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    let frontier_hint=if input["rsiAutonomous"]==true && !paired && !owner_focus
 {
        architecture_target["domain"].as_str().filter(|domain|DOMAINS.contains(domain))
            .map(str::to_string)
    }else{None};
    let successor_hint=if paired || owner_focus {
        input["rsiSelectedDomain"].as_str().map(str::to_string)
    }else{
        (if negative_transfer_active || algebra_active || capability_active
            || (science_active && science["phase"]!="discover_new_mechanism") {
            architecture_target["domain"].as_str()
        }else{None}).map(str::to_string)
            .or_else(||successor_stage["domain"].as_str().map(str::to_string))
            .or(frontier_hint).or(inherited.clone())
            .or_else(||system_evolution_target_hint(
                input,snapshot["attempt"].as_u64().unwrap_or(0)))
    };
    let objective=selection_objective(input);
    let trial=if paired || negative_transfer_active || algebra_active
        || capability_active || (science_active
        && science["phase"]!="discover_new_mechanism") {None} else {
        input["rsiTrialAlgorithmId"].as_str().map(str::to_string)
    };
    let (domain,selection,strategy)=select_generation_strategy(
        &state,&history,&objective,None,trial,successor_hint,commit);
    let mut source_input=input.clone();
    source_input["rsiTargetDomain"]=json!(domain);
    source_input["rsiSelectedDomain"]=json!(domain);
    let policy=strategy["id"].as_str()
        .and_then(|id|state.store.get("algorithm",id))
        .or_else(||crate::engine::algorithms::active(&state,"selfimprove.search"));
    let mut ranked=choose_focus_files(&state,&source_input,&history,policy.as_ref());
    // A source-bound architecture gap is a descendant-selected candidate,
    // never an independently proven gain.
    if input["rsiAutonomous"]==true && !owner_focus && !paired
        && architecture_target["domain"]==domain {
        if let Some(path)=architecture_target["path"].as_str() {
            if crate::engine::rsi::safe_path(path) && source_root.join(path).is_file() {
                ranked.retain(|row|row["path"]!=path);
                ranked.insert(0,json!({
                    "path":path,"sourceHash":architecture_target["sourceSha256"],
                    "source":"compiled_descendant_architecture_frontier",
                    "role":architecture_target["role"],
                    "domain":domain,"sourceIndexId":input["rsiIndexId"]
                }));
            }
        }
    }
    // The compiled descendant may propose improving TWO separate intelligence
    // subsystems, not merely report an A→B edge after the fact. Current SHA
    // identity is verified by the existing source index and frozen validator.
    if input["rsiAutonomous"]==true && !owner_focus && !paired
        && successor_stage.is_null() && architecture_target["domain"]==domain
        && symbiosis["complement"]["fromPath"]==architecture_target["path"] {
        let partner=&symbiosis["complement"];
        if let Some(path)=partner["toPath"].as_str()
            .filter(|path|crate::engine::rsi::safe_path(path)
                && source_root.join(path).is_file()) {
            ranked.retain(|row|row["path"]!=path);
            ranked.insert(1,json!({
                "path":path,"sourceHash":partner["toSha256"],
                "source":"compiled_reciprocal_intelligence_curriculum",
                "role":partner["toRole"],"domain":domain,
                "sourceIndexId":input["rsiIndexId"]
            }));
        }
    }
    // The descendant can explore cross-DOMAIN causal hypotheses instead
    // of only selecting two files under the same domain heuristic. Each
    // candidate remains source-indexed and the owner-selected focus wins.
    if coupled_portfolio.is_object()
        && architecture_target["domain"]==domain && !owner_focus && !paired {
        let mut insertion=1usize;
        for member in coupled_portfolio["focus"].as_array()
            .into_iter().flatten().skip(1).take(2) {
            let Some(path)=member["path"].as_str().filter(|p|
                crate::engine::rsi::safe_path(p) && source_root.join(p).is_file())
                else{continue};
            let Some(sha)=member["sourceSha256"].as_str() else{continue};
            if crate::engine::code_index::indexed_source_hash(
                &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                .as_deref()!=Some(sha){continue;}
            ranked.retain(|row|row["path"]!=path);
            ranked.insert(insertion.min(ranked.len()),json!({
                "path":path,"sourceHash":sha,
                "source":"compiled_holdout_cross_role_portfolio",
                "role":member["role"],"domain":member["domain"],
                "sourceIndexId":input["rsiIndexId"]
            }));
            insertion+=1;
        }
    }
    // An accepted source-written operator chooses the actual next edit,
    // regardless of whether it was called A, B, C, or invented this week.
    // Its method code becomes the target when previous executions regress.
    if algebra_active {
        let row=&algebra["selected"]["target"];
        if let (Some(path),Some(sha))=(row["path"].as_str(),row["sourceSha256"].as_str()) {
            if crate::engine::rsi::safe_path(path)
                && source_root.join(path).is_file()
                && crate::engine::code_index::indexed_source_hash(
                    &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()==Some(sha) {
                ranked.retain(|entry|entry["path"]!=path);
                ranked.insert(0,json!({
                    "path":path,"sourceHash":sha,"role":row["role"],
                    "domain":row["domain"],"sourceIndexId":input["rsiIndexId"],
                    "source":"compiled_model_authored_improvement_algebra"
                }));
            }
        }
    }
    // The generated experiment is an actual editing decision, not just
    // appended prompt text. Isolated experiments MUST focus only one role;
    // accepted-source recursive stages take priority over older guesses.
    if science_active && !predictor_repair_active && !meta_redesign_active
        && !algebra_active && !capability_active
        && science["phase"]!="discover_new_mechanism" {
        let members=science["focus"].as_array().cloned().unwrap_or_default();
        for member in members.into_iter().rev(){
            let Some(path)=member["path"].as_str()
                .filter(|path|crate::engine::rsi::safe_path(path)
                    && source_root.join(path).is_file()) else{continue};
            let Some(sha)=member["sourceSha256"].as_str() else{continue};
            if crate::engine::code_index::indexed_source_hash(
                &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                .as_deref()!=Some(sha){continue;}
            ranked.retain(|row|row["path"]!=path);
            ranked.insert(0,json!({
                "path":path,"sourceHash":sha,"role":member["role"],
                "domain":member["domain"],
                "source":"compiled_recursive_scientific_program",
                "sourceIndexId":input["rsiIndexId"]
            }));
        }
    }
    // Repeated source-isolated holdout regressions demand a real TWO-file
    // experiment in the next compiled descendant, not another planner note.
    // Source SHA and index identity are checked again before each source edit.
    if negative_transfer_active && architecture_target["domain"]==domain {
        for (position,key) in [(0usize,"victim"),(1usize,"source")] {
            let row=&negative_transfer["selected"][key];
            let (Some(path),Some(sha))=(
                row["path"].as_str().filter(|p|crate::engine::rsi::safe_path(p)),
                row["sourceSha256"].as_str())else{continue};
            if !source_root.join(path).is_file()
                || crate::engine::code_index::indexed_source_hash(
                    &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()!=Some(sha){continue;}
            ranked.retain(|entry|entry["path"]!=path);
            ranked.insert(position.min(ranked.len()),json!({
                "path":path,"sourceHash":sha,"role":row["role"],
                "domain":row["domain"],"sourceIndexId":input["rsiIndexId"],
                "source":"compiled_independent_negative_transfer_pair"
            }));
        }
    }
    let focus=if meta_redesign_active {
        ranked.iter().take(1).filter_map(|row|row["path"].as_str())
            .collect::<Vec<_>>()
    }else if negative_transfer_active {
        ranked.iter().take(2).filter_map(|row|row["path"].as_str())
            .collect::<Vec<_>>()
    }else if algebra_active || capability_active
        || (science_active && !predictor_repair_active
            && matches!(science["phase"].as_str(),
        Some("isolate_left"|"isolate_right"))) {
        ranked.iter().take(1).filter_map(|row|row["path"].as_str())
            .collect::<Vec<_>>()
    }else {
        ranked.iter().filter_map(|row|row["path"].as_str())
            .filter(|p|crate::engine::rsi::safe_path(p))
            .take(8).collect::<Vec<_>>()
    };
    let directive=crate::engine::rsi_system::improvement_directive(&history);
    let archive=crate::engine::rsi_archive::select(
        &state,&history,commit,&input["rsiFrozenCases"]);
    let mut edit_policy=learned_edit_method(&state,input,&history);
    if successor_stage["source"]=="rebound_verified_model_successor_program_v1" {
        let previous=edit_policy["revision"].clone();
        if successor_stage["executionPolicy"]=="grounded_edit" {
            edit_policy["checkpointAfter"]=json!(2);
            edit_policy["preferredTool"]=json!("workspace_replace_range");
        }else{
            edit_policy["checkpointAfter"]=json!(4);
        }
        edit_policy["selectedBySuccessorStage"]=successor_stage["stageIndex"].clone();
        edit_policy["successorDesignId"]=successor_stage["designId"].clone();
        edit_policy["revision"]=json!(format!("{:x}",Sha256::digest(
            serde_json::to_vec(&json!([previous,successor_stage,commit]))
                .unwrap_or_default())));
    }
    let inherited_program=crate::engine::rsi_system::inherited_program(&state,&history);
    if let Some(program)=inherited_program.as_ref(){
        // The existing runtime obeys rsiEditPolicy.program as an actual
        // per-round tool menu, so this affects execution rather than prose.
        let source_revision=edit_policy["revision"].clone();
        edit_policy["program"]=program["stages"].clone();
        edit_policy["inheritedProgram"]=program.clone();
        edit_policy["parentGenomeRevision"]=source_revision.clone();
        edit_policy["revision"]=json!(format!("{:x}",Sha256::digest(
            serde_json::to_vec(&json!([
                source_revision,program["revision"],commit
            ])).unwrap_or_default())));
    }
    // Decisions about the EXPERIMENT are now generated from the source-
    // compiled policy. The closure proof only observes consequences and
    // never chooses the next experiment or target.
    let mut policy_input=input.clone();
    policy_input["rsiMetaOperatorActive"]=json!(algebra_active);
    policy_input["rsiFocusFiles"]=json!(&focus);
    policy_input["rsiSelectedDomain"]=json!(domain);
    policy_input["rsiArchitectureTarget"]=architecture_target.clone();
    let experiment_policy=crate::engine::rsi_evolve::meta_experiment_policy::synthesize(
        &history,&frontier,&algebra,&policy_input,commit);
    let comparative_mode=if peers["references"].as_array()
        .is_some_and(|rows|!rows.is_empty()) {"cross_lineage_hybrid_design"}
        else if meta_policy["counterexamples"].as_array()
            .is_some_and(|rows|rows.len()>=2) {"multi_trajectory_design"}
        else {"local_source_redesign"};
    println!("{}",json!({
        "source":"compiled_descendant_system_controller_v2",
        "generatorCommit":commit,"historyCount":history.len(),
        "domain":domain,"selection":selection,"strategy":strategy,
        "focusFiles":focus,"ranked":ranked,
        "systemBlueprint":system_blueprint,
        "sourceTreeSha256":system_blueprint["sourceTreeSha256"],
        "architectureFrontier":frontier,
        "architectureTarget":architecture_target,
        "metaPolicy":meta_policy,
        "symbioticCurriculum":symbiosis,
        "interactionGraph":interaction_graph,
        "negativeTransferPlan":negative_transfer,
        "coupledPortfolio":coupled_portfolio,
        "factorialExperiment":factorial,
        "recursiveScientificProgram":science,
        "improvementAlgebra":algebra,
        "methodGeneralization":method_learning,
        "experimentPolicy":experiment_policy,
        "metaOperatorActive":algebra_active,
        "metaRedesignTarget":meta_redesign_target,
        "metaRedesignActive":meta_redesign_active,
        "modelEvolutionPlan":model_evolution,
        "successorStage":successor_stage,
        "peerLineages":peers,
        "comparativeMode":comparative_mode,
        "decisionOwner":"rebuilt_descendant_binary",
        "improvementDirective":directive,
        "archiveSelection":archive,
        "inheritedProgram":inherited_program,
        "editPolicy":edit_policy
    }));
    Ok(())
}

