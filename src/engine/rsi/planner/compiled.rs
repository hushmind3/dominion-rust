//! compiled: source-grounded compiled descendant planning responsibility.
use super::*;

// Allow the compiled model's *own* meta-method redesign to reach the
// executable source selector, without overriding an owner's explicit focus
// or an independently adopted predecessor programme.
pub(crate) fn evidence_based_meta_repair_focus(
    meta:&Value,successor_stage:&Value,explicit_focus:bool
)->bool{
    !explicit_focus && !successor_stage.is_object()
        && meta["metaRepairChosen"]==true
        && match meta["mode"].as_str() {
            Some("repair_own_prediction_mechanism")=>
                meta["target"]["path"]=="src/engine/rsi_meta_controller.rs",
            Some("repair_own_transfer_method")=>
                meta["target"]["path"]==
                    crate::engine::rsi_meta_controller::TRANSFER_METHOD_SOURCE,
            _=>false
        }
}

/// Compute the NEXT system architecture proposal inside the currently
/// compiled descendant. The resident coordinator has no authority to replace
/// this decision; it may validate a plan and then run a frozen evaluator.
pub(crate) fn compiled_system_generation_plan(
    state:&AppState,input:&Value,history:&[Value],commit:&str
)->Result<Value,String> {
    if input["rsiSystemEvolution"]!=true
        || input["rsiGeneratorCommit"]!=commit
        || !input["rsiSourceDir"].is_string() {
        return Err("컴파일된 후손 계획의 revision 또는 시스템 모드가 올바르지 않습니다.".into());
    }
    let root=Path::new(input["rsiSourceDir"].as_str().unwrap_or(""));
    // Derive the entire compiled successor plan from replayed immutable
    // experimental observations, not stored success/failure labels.
    let audited_history=crate::engine::rsi_system::revalidated_method_history(
        state,history);
    let history=audited_history.as_slice();
    let blueprint=crate::engine::rsi_system::blueprint(root,commit,history);
    if blueprint["sourceTreeSha256"].as_str().is_none_or(|s|s.len()!=64)
        || blueprint["architecture"].as_array().is_none_or(|a|a.len()<7||a.len()>64) {
        return Err("후손이 실제 전체 시스템 자기모형을 구축하지 못했습니다.".into());
    }
    let objective=selection_objective(input);
    // The accepted model's independently measured design is executable
    // successor policy. Failure/research history still drives the evolved
    // search algorithm when no adopted model-designed successor exists.
    let inherited=crate::engine::rsi_system::inherited_domain(history);
    // The compiled descendant searches its own source graph for whole-system
    // coverage gaps, independently of the coordinator's historical ranking.
    // A predecessor's verified design remains authoritative for its child.
    // An uncovered module is a search opportunity, NEVER a demonstrated gain.
    let frontier=crate::engine::rsi_system::architecture_frontier(
        state,&blueprint,history,input["rsiIndexId"].as_str().unwrap_or(""),root);
    // This editable module runs INSIDE the newly compiled descendant.
    // Its learned prediction dynamics select the NEXT architecture target.
    let explicit_focus=input["rsiAutoFocus"]!=true
        && input["rsiFocusFiles"].as_array().is_some_and(|files|!files.is_empty());
    // Source-grounded coverage, inherited design, prequential forecasts,
    // transferable methods and the next recursive plan share the same
    // independently revalidated history snapshot.
    let baseline_meta=crate::engine::rsi_meta_controller::synthesize(
        history,&frontier,DOMAINS,inherited.as_deref());
    // Portable method hypotheses require verified peer receipts and at
    // least one identical indexed source. Only the COMPILED child can
    // re-rank its own target; the frozen resident validates the raw receipts.
    // Explicit user-selected paths and adopted successor programs keep
    // their existing precedence.
    let peers=crate::engine::rsi_meta_controller::peer_lineages(
        state,input["rsiCampaignId"].as_str().unwrap_or(""),input,None);
    let meta=if explicit_focus{baseline_meta}else{
        crate::engine::rsi_meta_controller::evolve_from_peer_methods(
            baseline_meta,&peers,history)
    };
    // Learned laws are compiled into this actual successor, not a prompt-only
    // retrospective. A task-intelligence bottleneck can supersede another
    // purely meta-level tweak. The child may rewrite this scoring law itself.
    let principle=crate::engine::rsi_principle_lab::plan(history,&frontier,commit);
    // An actual adopted model-authored experiment graph supersedes the old
    // fixed phase scheduler; its interpreter is compiled into THIS child.
    let open_program=if explicit_focus{Value::Null}else{
        crate::engine::rsi_open_program::synthesize(history,&frontier,commit)
    };
    let mut architecture_target=if open_program["target"].is_object(){
        open_program["target"].clone()
    }else if principle["shouldProbeTaskMethod"]==true
        && principle["target"].is_object()
        && principle["methodTarget"].is_object(){
        principle["target"].clone()
    }else if meta["target"].is_object(){
        meta["target"].clone()
    }else{frontier.first().cloned().unwrap_or(Value::Null)};
    // A predecessor can AUTHOR the successor's task program. The compiled
    // descendant interprets it against its own newly built source graph and
    // silently drops stages whose path/role/hash is no longer current.
    let successor_stage=if open_program["stage"].is_object(){
        open_program["stage"].clone()
    }else{
        crate::engine::rsi_system::inherited_stage(history,&frontier)
            .or_else(||crate::engine::rsi_mutual_evolution::inherited_programme_stage(
                history,&frontier))
            .or_else(||crate::engine::rsi_mutual_evolution::reciprocal_return(
                history,&frontier))
            .unwrap_or(Value::Null)
    };
    // An independently observed meta-rule failure should reach the actual
    // compiled source target. Existing adopted successor programmes and
    // explicit owner focus retain their higher priority.
    let enforce_meta_repair=evidence_based_meta_repair_focus(
        &meta,&successor_stage,explicit_focus);
    if enforce_meta_repair{
        architecture_target=meta["target"].clone();
    }
    // Owner-constrained source targets must remain authoritative. The
    // autonomous factorial scheduler may not silently redirect an explicit
    // user target or forbid its ordinary source edits at the tool gateway.
    // A live, adopted predecessor's programme takes precedence. Otherwise
    // the compiled meta agent authors the next A->B->A source curriculum.
    let mutual=if successor_stage.is_object() || explicit_focus || enforce_meta_repair{
        Value::Null
    }else{
        crate::engine::rsi_mutual_evolution::curriculum(&frontier,&meta,DOMAINS)
    };
    // This experiment designer is compiled into the NEW child and can evolve.
    let raw_factorial=if successor_stage.is_object() || explicit_focus || enforce_meta_repair{
        Value::Null
    }else{
        crate::engine::rsi_causal_curriculum::design(
            history,&mutual,&frontier,commit)
    };
    let suggested=crate::engine::rsi_recursive_discovery::plan(
        history,&frontier,&blueprint["openTopology"],&meta,
        &raw_factorial,&mutual,commit);
    let phase=suggested["phase"].as_str().unwrap_or("").to_string();
    // Never interrupt an already accepted recursive invention lineage.
    // When the scientific programme is only inventing from no evidence,
    // let the descendant first create an independently testable task↔method
    // principle and measure whether it makes its NEXT descendant stronger.
    let principle_active=!explicit_focus && !enforce_meta_repair
        && !successor_stage.is_object()
        && principle["target"].is_object()
        && principle["methodTarget"].is_object()
        && principle["shouldProbeTaskMethod"]==true
        && matches!(phase.as_str(),"invent_new_intelligence"|"observe_factorial");
    let scientific_active=!explicit_focus && !enforce_meta_repair
        && !principle_active
        && !open_program["stage"].is_object()
        && suggested["target"].is_object()
        && (matches!(phase.as_str(),
            "develop_invented_capability"|"return_to_origin"|"rewrite_discovery_method"
                |"repair_discovery_method")
            || !successor_stage.is_object());
    // An adopted recursive-science continuation owns the ONE source focus:
    // no unrelated isolated factorial arm may block its actual write tools.
    let factorial=if principle_active
        || (scientific_active && phase!="invent_new_intelligence"){
        Value::Null
    }else{raw_factorial};
    let scientific=if scientific_active && phase!="invent_new_intelligence"{
        crate::engine::rsi_recursive_discovery::plan(
            history,&frontier,&blueprint["openTopology"],&meta,
            &factorial,&mutual,commit)
    }else{suggested};
    let empty_scientist=Value::Null;
    let scientific_target=if scientific_active{
        &scientific["target"]
    }else{&empty_scientist};
    let scientific_domain=scientific_target["domain"].as_str()
        .filter(|d|DOMAINS.contains(d)).map(str::to_string);
    let factorial_target=&factorial["selected"]["target"];
    let factorial_domain=factorial_target["domain"].as_str()
        .filter(|d|DOMAINS.contains(d)).map(str::to_string);
    let successor_domain=successor_stage["domain"].as_str()
        .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
    let architecture_domain=architecture_target["domain"].as_str()
        .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
    let continuation=if successor_domain.is_some()||inherited.is_some()
        ||architecture_domain.is_some(){None}else{research_continuation(history)};
    let selected=if enforce_meta_repair{
        architecture_domain.clone()
    }else{
        scientific_domain.or(successor_domain).or(factorial_domain)
            .or(inherited.clone()).or(architecture_domain)
            .or_else(||input["rsiSelectedDomain"].as_str()
                .filter(|domain|DOMAINS.contains(domain)).map(str::to_string))
    };
    let (domain,selection,strategy)=select_generation_strategy(
        state,history,&objective,continuation,
        None,selected,commit
    );
    let mut focus_input=input.clone();
    focus_input["rsiTargetDomain"]=json!(domain);
    let mut ranked=choose_focus_files(state,&focus_input,history,None);
    // Keep the indexed source evidence and SHA attached to the actual
    // descendant-selected target. An inherited model design can change the
    // domain, in which case use that domain's indexed focus instead.
    let preferred=if scientific_active && scientific_target["domain"]==domain {
        scientific_target["path"].as_str()
    }else if successor_stage["domain"]==domain {
        successor_stage["path"].as_str()
    }else if factorial_target["domain"]==domain {
        factorial_target["path"].as_str()
    }else if architecture_target["domain"]==domain {
        architecture_target["path"].as_str()
    }else{None};
    if let Some(path)=preferred {
        if root.join(path).is_file() {
            ranked.retain(|row|row["path"]!=path);
            let inherited_focus=successor_stage["path"]==path;
            let factorial_focus=factorial_target["path"]==path
                && factorial_target["domain"]==domain;
            let scientific_focus=scientific_active
                && scientific_target["path"]==path
                && scientific_target["domain"]==domain;
            let chosen=if scientific_focus{scientific_target}
                else if inherited_focus{&successor_stage}
                else if factorial_focus{factorial_target}
                else{&architecture_target};
            let mut prioritised=json!({
                "path":path,
                "sourceHash":chosen["sourceSha256"],
                "source":if scientific_focus{
                    "compiled_recursive_discovery_experiment"
                }else if inherited_focus {
                    "inherited_model_authored_successor_program"
                }else if factorial_focus{"compiled_factorial_source_intervention"}
                else{"compiled_architecture_frontier"},
                "role":chosen["role"],
                "domain":domain,
                "unmodified":if inherited_focus {
                    true
                }else{chosen["verifiedRoleImproved"]==false}
            });
            // Keep exact current-file index binding, so stale source graph
            // choices never become model-authorized write targets.
            prioritised["sourceIndexId"]=input["rsiIndexId"].clone();
            ranked.insert(0,prioritised);
        }
    }
    let original_paths=ranked.iter().filter_map(|row|row["path"].as_str())
        .filter(|path|crate::engine::rsi::safe_path(path)
            && root.join(path).is_file())
        .take(3).collect::<Vec<_>>();
    // Both source modules enter the NATIVE edit context, not merely the
    // prompt text. The B module may belong to a different domain than A.
    let mut paths=Vec::<String>::new();
    if enforce_meta_repair {
        // A meta-repair generation must edit the method-learning source,
        // not merely mention it alongside cheaper unrelated focus files.
        if let (Some(path),Some(sha))=(architecture_target["path"].as_str(),
            architecture_target["sourceSha256"].as_str()) {
            if crate::engine::rsi::safe_path(path) && root.join(path).is_file()
                && crate::engine::code_index::indexed_source_hash(
                    state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()==Some(sha) {
                paths.push(path.to_string());
            }
        }
    }else if principle_active {
        // Source-ground both ends of the proposed learning law. This is an
        // actual two-file edit target, not just an abstract method label.
        for key in ["target","methodTarget"] {
            let item=&principle[key];
            if let (Some(path),Some(sha))=(
                item["path"].as_str(),item["sourceSha256"].as_str()) {
                if crate::engine::rsi::safe_path(path)
                    && root.join(path).is_file()
                    && crate::engine::code_index::indexed_source_hash(
                        state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                        .as_deref()==Some(sha)
                    && !paths.iter().any(|p|p==path) {
                    paths.push(path.to_owned());
                }
            }
        }
    }else if scientific_active {
        if let Some(path)=scientific_target["path"].as_str()
            .filter(|p|crate::engine::rsi::safe_path(p)){
            paths.push(path.to_string());
        }
        if phase=="invent_new_intelligence" {
            for candidate in ["src/engine/rsi_evolve.rs","src/engine/mod.rs"] {
                if paths.len()>=3{break;}
                if !paths.iter().any(|p|p==candidate)
                    && crate::engine::code_index::indexed_source_hash(
                        state,input["rsiIndexId"].as_str().unwrap_or(""),candidate)
                        .is_some(){paths.push(candidate.into());}
            }
        }
    }else if factorial["selected"]["isolationRequired"]==true {
        if let (Some(path),Some(sha))=(
            factorial_target["path"].as_str(),factorial_target["sourceSha256"].as_str()) {
            if crate::engine::rsi::safe_path(path)
                && crate::engine::code_index::indexed_source_hash(
                    state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()==Some(sha){paths.push(path.to_string());}
        }
    }else if mutual["pair"].is_object()
        && architecture_target["domain"]==domain {
        for (key,sha_key) in [("fromPath","fromSourceSha256"),
            ("toPath","toSourceSha256")] {
            if let (Some(path),Some(sha))=(mutual["pair"][key].as_str(),
                mutual["pair"][sha_key].as_str()) {
                if crate::engine::rsi::safe_path(path) && root.join(path).is_file()
                    && crate::engine::code_index::indexed_source_hash(
                        state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                        .as_deref()==Some(sha) && !paths.iter().any(|p|p==path) {
                    paths.push(path.to_owned());
                }
            }
        }
    }
    if enforce_meta_repair {
        // Preserve only the observed self-repair source for this experiment.
    }else if scientific_active {
        // Invention may need a new module plus existing consumers; leave the
        // execution target no broader than its SOURCE-GROUNDED programme.
    }else if principle_active {
        // Keep both actual intelligence and method-learning sources as the
        // executable model context. Generic ranked files must not evict them.
    }else if factorial["selected"]["isolationRequired"]!=true{
        for path in original_paths{
            if paths.len()>=3{break;}
            if !paths.iter().any(|p|p==path){paths.push(path.to_owned());}
        }
    }
    let directive=crate::engine::rsi_system::improvement_directive(history);
    // Choose an independently replayed persistent stepping stone INSIDE the
    // rebuilt descendant. Frozen evidence is shared; model-authored and
    // executable selection methods may evolve with the child binary.
    let archive=if !explicit_focus && input["rsiAutoFocus"]==true
        && input["rsiFailedPatchRecovery"]["candidateId"].is_null() {
        crate::engine::rsi_archive::select_bound(
            state,history,commit,&input["rsiFrozenCases"],
            input["rsiSourceBranch"].as_str().unwrap_or(""))
    }else{Value::Null};
    // The NEWLY COMPILED child, not the old campaign supervisor, learns
    // which real configured API/local model will author this generation.
    // Updating this source or rsi_model_ecology.rs changes future routing.
    let model_route=crate::engine::rsi_model_ecology::choose(
        state,input,history,commit,&domain,
        &crate::engine::memory::stable_id(&[
            input["rsiObjective"].as_str().unwrap_or("")
        ]));
    let edit_policy=inherited_edit_policy(state,input,history,
        &model_route["selected"],&domain);
    Ok(json!({
        "source":"compiled_descendant_system_controller_v3",
        "generatorCommit":commit,
        "historyCount":history.len(),
        "sourceTreeSha256":blueprint["sourceTreeSha256"],
        "objectiveId":crate::engine::memory::stable_id(&[&objective]),
        "indexId":input["rsiIndexId"],
        "domain":domain,
        "selection":selection,
        "strategy":strategy,
        "targetPlan":generation_target_plan(&domain,selection,&strategy,commit),
        "focusFiles":paths,
        "ranked":ranked,
        "systemBlueprint":blueprint,
        "architectureFrontier":frontier,
        "architectureTarget":architecture_target,
        "metaPolicy":meta,
        "metaActionPolicy":meta["controlPolicy"],
        "peerLineages":peers,
        "mutualCurriculum":mutual,
        "factorialCurriculum":factorial,
        "recursiveDiscovery":scientific,
        "recursiveScienceActive":scientific_active,
        "principleProgramme":principle,
        "principleActive":principle_active,
        "openProgramPlan":open_program,
        "archiveSelection":archive,
        "editPolicy":edit_policy,
        "modelRoute":model_route,
        "successorStage":successor_stage,
        "improvementDirective":directive,
        "decisionOwner":"rebuilt_descendant_binary",
        "frozenAuthority":"resident_independent_evaluator"
    }))
}

