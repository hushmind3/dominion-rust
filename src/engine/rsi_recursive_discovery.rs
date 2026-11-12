//! Self-editable recursive intelligence discovery and improvement scientist.
//!
//! Unlike an A->B pair scheduler, this program uses independently ADOPTED
//! descendants to close an invention -> novel capability -> original system
//! -> invention method cycle. Invention and adoption are not synonymous.
//! The frozen paired evaluator and promotion boundary are outside this module.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn hash(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn sha40(v:&Value)->bool{
    v.as_str().is_some_and(|s|s.len()==40 &&
        s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn sha64(v:&Value)->bool{
    v.as_str().is_some_and(|s|s.len()==64 &&
        s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn adopted(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true
        && sha40(&g["acceptedCommit"]) && sha40(&g["generatorCommit"])
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
        && g["metaPolicyExecuted"]==true
        && crate::engine::rsi_system::design_implemented(
            &g["systemDesign"],
            &g["acceptedSourceHashes"].as_object()
                .map(|obj|obj.keys().map(|p|json!(p)).collect::<Vec<_>>())
                .unwrap_or_default())
}
fn current<'a>(frontier:&'a [Value],role:&str,path:&str)->Option<&'a Value>{
    frontier.iter().find(|f|f["role"]==role && f["path"]==path
        && sha64(&f["sourceSha256"])
        && f["path"].as_str().is_some_and(crate::engine::rsi::safe_path))
}
fn role_changed(g:&Value,role:&str,path:&str,phase:&str)->bool{
    if !adopted(g) || g["recursiveScienceExecuted"]!=true
        || g["recursiveDiscovery"]["phase"]!=phase
        || g["recursiveDiscovery"]["generatorCommit"]!=g["generatorCommit"]
        || g["recursiveDiscovery"]["target"]["role"]!=role
        || g["recursiveDiscovery"]["target"]["path"]!=path
        || !sha64(&g["acceptedSourceHashes"][path]) {return false;}
    let design=&g["systemDesign"];
    let files=design["componentFiles"][role].as_array()
        .or_else(||(design["component"]==role)
            .then(||design["targetFiles"].as_array()).flatten());
    files.is_some_and(|files|files.iter().any(|p|p.as_str()==Some(path)))
}

/// Follow the accepted *commit chain*, including independent unrelated
/// intermediate accepted descendants. A phase completes only upon its own
/// adopted source edit, not a written claim or an earlier-generation proposal.
fn adopted_step<'a>(history:&'a [Value],start:&str,
    role:&str,path:&str,phase:&str)->Option<&'a Value>{
    let mut head=start.to_string();
    let mut visited=BTreeSet::new();
    for _ in 0..history.len(){
        if !visited.insert(head.clone()){return None;}
        let child=history.iter().find(|g|
            adopted(g) && g["generatorCommit"].as_str()==Some(&head))?;
        if role_changed(child,role,path,phase) {return Some(child);}
        head=child["acceptedCommit"].as_str()?.to_string();
    }
    None
}
fn accepted_birth(g:&Value,module:&Value)->bool{
    adopted(g) && g["acceptedCommit"]==module["introductionCommit"]
        && g["systemDesign"]["id"]==module["introductionDesignId"]
        && g["systemDesign"]["moduleIntroductions"].as_array().is_some_and(|rows|
            rows.iter().any(|candidate|
                candidate["role"]==module["role"]
                && candidate["newFiles"]==module["newFiles"]
                && candidate["integrationFiles"]==module["integrationFiles"]))
}

fn unfinished(history:&[Value],frontier:&[Value],topology:&Value)->Option<Value>{
    let modules=topology["inventedComponents"].as_array()?;
    for invention in modules.iter().rev(){
        let Some(role)=invention["role"].as_str()else{continue};
        let Some(module)=invention["newFiles"].as_array()
            .and_then(|files|files.first()).and_then(Value::as_str)else{continue};
        let Some(g)=history.iter().rev()
            .find(|g|accepted_birth(g,invention))else{continue};
        let Some(module_source)=current(frontier,role,module)else{continue};
        let Some(origin)=invention["integrationFiles"].as_array()
            .into_iter().flatten().filter_map(Value::as_str)
            .find_map(|path|frontier.iter().find(|f|
                f["path"]==path && sha64(&f["sourceSha256"])))else{continue};
        let Some(origin_role)=origin["role"].as_str()else{continue};
        let Some(birth)=g["acceptedCommit"].as_str()else{continue};
        let developed=adopted_step(history,birth,role,module,
            "develop_invented_capability");
        if developed.is_none(){
            return Some(json!({
                "phase":"develop_invented_capability",
                "target":module_source,
                "birthGenerationId":g["id"],
                "birthCommit":g["acceptedCommit"],
                "inventedRole":role,"inventedPath":module,
                "originRole":origin_role,"originPath":origin["path"],
                "requiredPreviousCommit":g["acceptedCommit"],
                "developedCommit":null,"returnedCommit":null
            }));
        }
        let developed=developed?;
        let Some(first)=developed["acceptedCommit"].as_str()else{continue};
        let returned=adopted_step(history,first,origin_role,
            origin["path"].as_str().unwrap_or(""),"return_to_origin");
        if returned.is_none(){
            return Some(json!({
                "phase":"return_to_origin","target":origin,
                "birthGenerationId":g["id"],"birthCommit":g["acceptedCommit"],
                "inventedRole":role,"inventedPath":module,
                "originRole":origin_role,"originPath":origin["path"],
                "requiredPreviousCommit":developed["acceptedCommit"],
                "developedCommit":developed["acceptedCommit"],
                "returnedCommit":null
            }));
        }
        let returned=returned?;
        let Some(second)=returned["acceptedCommit"].as_str()else{continue};
        // A descendant must eventually rewrite the actual algorithm that
        // designs new experiments, rather than claiming task-agent gains
        // prove that the self-improvement method has evolved.
        let scientific=frontier.iter().find(|f|
            f["path"]=="src/engine/rsi_recursive_discovery.rs"
            && f["role"]=="improvement_methods")
            .or_else(||frontier.iter().find(|f|
                f["path"]=="src/engine/rsi_causal_curriculum.rs"
                && f["role"]=="improvement_methods"))
            .or_else(||frontier.iter().find(|f|
                f["path"]=="src/engine/rsi_meta_controller.rs"
                && f["role"]=="improvement_methods"));
        let Some(scientific)=scientific else{continue};
        let Some(method_path)=scientific["path"].as_str()else{continue};
        if adopted_step(history,second,"improvement_methods",method_path,
            "rewrite_discovery_method").is_none(){
            return Some(json!({
                "phase":"rewrite_discovery_method","target":scientific,
                "birthGenerationId":g["id"],"birthCommit":g["acceptedCommit"],
                "inventedRole":role,"inventedPath":module,
                "originRole":origin_role,"originPath":origin["path"],
                "requiredPreviousCommit":returned["acceptedCommit"],
                "developedCommit":developed["acceptedCommit"],
                "returnedCommit":returned["acceptedCommit"],
                "methodPath":scientific["path"]
            }));
        }
    }
    None
}

/// This is a lineage audit, not an equation claiming causality or infinite
/// recursion. A cycle is recognized only when FOUR independent adopted
/// source revisions occur in the correct rebuilt-child ancestry:
/// invention -> module growth -> origin improvement -> method rewrite.
/// Unrelated generations may occur BETWEEN accepted stages.
pub(crate) fn completed_cycles(history:&[Value])->Value {
    let mut cycles=Vec::new();
    let mut origins=BTreeSet::<String>::new();
    for birth in history.iter().filter(|g|adopted(g)){
        for module in birth["systemDesign"]["moduleIntroductions"]
            .as_array().into_iter().flatten(){
            let (Some(role),Some(path),Some(origin))=(
                module["role"].as_str(),
                module["newFiles"].as_array().and_then(|v|v.first())
                    .and_then(Value::as_str),
                module["integrationFiles"].as_array().and_then(|v|v.first())
                    .and_then(Value::as_str))else{continue};
            if !birth["acceptedSourceHashes"][path].is_string()
                || !birth["acceptedSourceHashes"][origin].is_string(){continue;}
            let Some(head)=birth["acceptedCommit"].as_str()else{continue};
            if !origins.insert(head.to_string()){continue;}
            let Some(grown)=adopted_step(history,head,role,path,
                "develop_invented_capability")else{continue};
            let Some(next)=grown["acceptedCommit"].as_str()else{continue};
            let Some(back)=adopted_step(history,next,
                "improvement_methods",origin,"return_to_origin")
                else{continue};
            let Some(next)=back["acceptedCommit"].as_str()else{continue};
            let method=adopted_step(history,next,"improvement_methods",
                "src/engine/rsi_recursive_discovery.rs",
                "rewrite_discovery_method")
                .or_else(||adopted_step(history,next,"improvement_methods",
                    "src/engine/rsi_causal_curriculum.rs",
                    "rewrite_discovery_method"))
                .or_else(||adopted_step(history,next,"improvement_methods",
                    "src/engine/rsi_meta_controller.rs",
                    "rewrite_discovery_method"));
            let Some(method)=method else{continue};
            cycles.push(json!({
                "introducedRole":role,
                "introducedSource":path,
                "originSource":origin,
                "inventedSourceCommit":birth["acceptedCommit"],
                "developedSourceCommit":grown["acceptedCommit"],
                "returnedSourceCommit":back["acceptedCommit"],
                "rewrittenImproverCommit":method["acceptedCommit"],
                "adoptionCount":4,
                "newCompiledPolicyInherited":true,
                "endToEndGainProven":false,
                "unboundedSelfImprovementProven":false
            }));
        }
    }
    let mut out=json!({
        "source":"four_independently_adopted_recursive_discovery_steps_v1",
        "completedCycles":cycles.len(),
        "cycles":cycles.into_iter().take(16).collect::<Vec<_>>(),
        "allStepsMustBeAcceptedInRealCommitChain":true,
        "qualityOrCausalityProven":false
    });
    out["revision"]=json!(hash(&out));
    out
}

/// The currently COMPILED descendant owns source focus and whether to invent
/// a new topology or continue a verified earlier invention. No resident
/// predecessor can recompute this ranking with an obsolete meta-algorithm.
pub(crate) fn plan(history:&[Value],frontier:&[Value],topology:&Value,
    meta:&Value,factorial:&Value,mutual:&Value,commit:&str)->Value{
    let pending=unfinished(history,frontier,topology);
    let mut phase="observe_factorial".to_string();
    let mut target=Value::Null;
    let mut ancestry=Value::Null;
    let mut invention=Value::Null;
    if let Some(stage)=pending{
        phase=stage["phase"].as_str().unwrap_or("observe_factorial").into();
        target=stage["target"].clone();
        ancestry=stage;
    }else{
        let observations=&factorial["selected"]["observations"];
        let interaction=observations["meanInteractionContrast"].as_f64();
        let matched=observations["matchedFrozenCases"].as_u64().unwrap_or(0);
        let unanswered=factorial["selected"]["phase"]!="analyze_interaction"
            && factorial["selected"]["phase"]!=""
            && factorial["selected"]["variantIndex"].is_number();
        // The missing factorial arms are a higher-priority actual scientific
        // action; once they are complete or stall, it may invent a new
        // causal-transfer mechanism. Invention is NEVER counted as a gain.
        if !unanswered {
            let role_set=frontier.iter().filter_map(|r|r["role"].as_str())
                .collect::<BTreeSet<_>>();
            let origin=if matched>0 && interaction.is_some_and(|n|n<=0.0){
                &factorial["selected"]["primary"]
            }else{&meta["target"]};
            let candidate=frontier.iter().find(|f|*f==origin)
                .or_else(||frontier.iter().find(|f|
                    f["role"]=="improvement_methods"
                    && f["path"]=="src/engine/rsi_evolve.rs"))
                .or_else(||frontier.first());
            if let Some(f)=candidate{
                // Stable experimental identity across failed generations:
                // changing the arbitrary name every time would prevent
                // learning the true failure rate of one causal hypothesis.
                let material=json!([commit,f["role"],f["sourceSha256"],
                    mutual["pair"]["fromRole"],mutual["pair"]["toRole"],
                    interaction]);
                let seed=hash(&material);
                let suffix=&seed[..12];
                let role=format!("causal_synthesis_{suffix}");
                if !role_set.contains(role.as_str()){
                    phase="invent_new_intelligence".into();
                    target=f.clone();
                    invention=json!({
                        "role":role,"domain":"improver",
                        "newFile":format!("src/engine/rsi_{role}.rs"),
                        "integrationFiles":["src/engine/rsi_evolve.rs"],
                        "moduleRegistrar":"src/engine/mod.rs",
                        "integrationSymbol":format!("synthesis_{suffix}"),
                        "modelDesignField":"moduleIntroductions",
                        "sourceOfHypothesis":if matched>0{
                            "observed_factorial_interaction"
                        }else{"unexplored_cross_domain_recombination"},
                        "observedInteractionContrast":interaction,
                        "matchedCaseCount":matched,
                        "mustActuallyWireExistingCallSite":true,
                        "requiresSuccessorStage":true,
                        "qualityProven":false
                    });
                }
            }
        }
    }
    let mut method_trials=Vec::new();
    for method in ["invent_new_intelligence","develop_invented_capability",
        "return_to_origin","rewrite_discovery_method"]{
        let tries=history.iter().filter(|g|
            g["recursiveDiscovery"]["phase"]==method
            && g["recursiveScienceExecuted"]==true
            && g["candidateId"].is_string()).count();
        let adopted_count=history.iter().filter(|g|
            g["recursiveDiscovery"]["phase"]==method
            && g["recursiveScienceExecuted"]==true && adopted(g)).count();
        method_trials.push(json!({
            "method":method,"observedTrials":tries,
            "independentlyAdopted":adopted_count,
            "posterior":(adopted_count as f64+1.0)/(tries as f64+2.0)
        }));
    }
    // Retries are bounded only for NEW inventions. A verified inherited
    // chain may continue after non-adopted attempts, without falsely
    // advancing any stage.
    if phase=="invent_new_intelligence"{
        let same=history.iter().rev().take(8).filter(|g|
            g["recursiveDiscovery"]["phase"]=="invent_new_intelligence"
            && g["recursiveDiscovery"]["invention"]["role"]==invention["role"]
            && g["candidateId"].is_string()
            && g["accepted"]!=true).count();
        if same>=2 {
            if let Some(method)=frontier.iter().find(|f|
                f["path"]=="src/engine/rsi_recursive_discovery.rs"
                    && f["role"]=="improvement_methods")
                .or_else(||frontier.iter().find(|f|
                    f["path"]=="src/engine/rsi_meta_controller.rs"
                    && f["role"]=="improvement_methods")){
                // Evolve the experiment-invention algorithm instead of
                // rolling a new meaningless role ID after repeated failure.
                phase="repair_discovery_method".into();
                target=method.clone();
                invention=Value::Null;
            }else{
                phase="observe_factorial".into();
                target=Value::Null;invention=Value::Null;
            }
        }
    }
    let core=json!({
        "source":"compiled_recursive_discovery_program_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "phase":phase,"target":target,"invention":invention,
        "ancestry":ancestry,"scientificMethodLearning":method_trials,
        "factorialEvidenceSha256":factorial["sha256"],
        "mutualCurriculumRevision":mutual["revision"],
        "openTopologySha256":topology["sha256"],
        "editableDiscoverySource":"src/engine/rsi_recursive_discovery.rs",
        "independentEvaluationOwner":"frozen_paired_supervisor",
        "sourceChangedBeforeEvidenceRequired":true,
        "qualityProven":false,"causalBenefitProven":false
    });
    let mut result=core;
    result["revision"]=json!(hash(&result));
    result
}
pub(crate) fn validate(plan:&Value,frontier:&[Value],commit:&str,
    history_len:usize,topology:&Value,factorial:&Value,mutual:&Value)->bool{
    if !plan.is_object(){return false;}
    let mut unsigned=plan.clone();
    if let Some(obj)=unsigned.as_object_mut(){obj.remove("revision");}
    if plan["source"]!="compiled_recursive_discovery_program_v1"
        || plan["revision"]!=hash(&unsigned)
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history_len as u64)
        || plan["openTopologySha256"]!=topology["sha256"]
        || plan["factorialEvidenceSha256"]!=factorial["sha256"]
        || plan["mutualCurriculumRevision"]!=mutual["revision"]
        || plan["independentEvaluationOwner"]!="frozen_paired_supervisor"
        || plan["qualityProven"]!=false
        || plan["causalBenefitProven"]!=false{return false;}
    if plan["phase"]=="observe_factorial"{
        return plan["target"].is_null();
    }
    if !matches!(plan["phase"].as_str(),
        Some("invent_new_intelligence"|"develop_invented_capability"|
            "return_to_origin"|"rewrite_discovery_method"|
            "repair_discovery_method"))
        || !frontier.iter().any(|f|f==&plan["target"]){return false;}
    if plan["phase"]=="invent_new_intelligence"{
        let source=plan["invention"]["newFile"].as_str().unwrap_or("");
        return crate::engine::rsi::safe_path(source)
            && source.starts_with("src/engine/rsi_")
            && source.ends_with(".rs")
            && plan["invention"]["role"].as_str().is_some_and(|r|
                r.starts_with("causal_synthesis_")
                && r.len()<=64)
            && plan["invention"]["mustActuallyWireExistingCallSite"]==true
            && plan["invention"]["integrationFiles"].as_array().is_some_and(
                |files|files.len()==1 && files[0]=="src/engine/rsi_evolve.rs");
    }
    if plan["phase"]=="repair_discovery_method"{
        return plan["target"]["role"]=="improvement_methods"
            && plan["target"]["path"].as_str().is_some_and(|p|
                matches!(p,"src/engine/rsi_recursive_discovery.rs"
                    |"src/engine/rsi_meta_controller.rs"));
    }
    let ancestor=&plan["ancestry"];
    sha40(&ancestor["birthCommit"])
        && sha40(&ancestor["requiredPreviousCommit"])
        && ancestor["target"]==plan["target"]
        && ancestor["phase"]==plan["phase"]
        && ancestor["birthGenerationId"].is_string()
}
pub(crate) fn instruction(plan:&Value)->String{
    let phase=plan["phase"].as_str().unwrap_or("");
    if phase=="observe_factorial"{return String::new();}
    let source=&plan["target"];
    let change=match phase{
        "invent_new_intelligence"=>
            "INVENT a genuinely NEW executable intelligence ability, not merely a renamed helper. Create the proposed Rust module, register it in src/engine/mod.rs and call its new method from the REAL compiled src/engine/rsi_evolve.rs. Call rsi_system_plan(component=system, components=[new role, improvement_methods], componentFiles, moduleIntroductions, interactionEdges, successorStages); include the same integrationSymbol in BOTH the new implementation and the changed rsi_evolve.rs execution consumer. Include the exact new module in moduleIntroductions.newFiles and rsi_evolve.rs in moduleIntroductions.integrationFiles. A subsequent accepted descendant must improve the new capability, then return to improve the original subsystem and rewrite THIS experiment-discovery algorithm.",
        "develop_invented_capability"=>
            "An independently adopted descendant has already introduced a source-integrated new intelligence. Actually change its CURRENT module's implementation and measure independently. This stage is not complete until a rebuilt accepted child has rewritten the new component.",
        "return_to_origin"=>
            "A compiled child has independently improved the invented ability. Use that improved implementation to REWRITE the original agent component's source code. Source-level adoption, not a message claiming transfer, is required.",
        "rewrite_discovery_method"=>
            "A descendant has independently improved the original agent component using the invented ability. NOW rewrite THIS program's method of generating new intelligence experiments, or another genuine meta experiment source. The next rebuilt descendant must inherit the changed meta algorithm.",
        "repair_discovery_method"=>
            "Multiple independently evaluated candidate inventions from the same stable source-hypothesis failed. Change the SOURCE of the discovery method; invent a better experiment selection and failure model, do not merely rename or replay the candidate.",
        _=>"",
    };
    format!(
        "[SELF-EVOLVING RECURSIVE SCIENTIST] phase={}; target={}; \
        invention={}; verifiedAncestry={}; methodEvidence={}; \
        programmeRevision={}. {} This is a SOURCE-CHANGING hypothesis. \
        Do not claim improvement before the independent paired evaluator adopts \
        an actual patch. The next compiled descendant owns the successor stage.",
        phase,source,plan["invention"],plan["ancestry"],
        plan["scientificMethodLearning"],plan["revision"],change
    )
}
