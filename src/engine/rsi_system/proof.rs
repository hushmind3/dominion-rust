//! Independent whole-system descendant capability proof.
use super::*;

/// Recompute whole-system recursion from model-produced source changes,
/// actual compiled planner ownership, independent paired receipts and a
/// cumulative set of redesigned intelligence components. Speeding up one
/// memorized benchmark three times is never whole-system evolution.
pub(crate) fn system_capability_proven(state:&crate::state::AppState, history:&Value)->bool {
    let Some(rows)=history.as_array() else {return false;};
    let mut chain=0usize;
    let mut prior_commit=String::new();
    let mut prior_binary=String::new();
    let mut covered=std::collections::BTreeSet::new();
    let mut independently_planned=0usize;
    let mut proposed_model_methods=std::collections::BTreeSet::new();
    let mut witnessed_inherited_method=false;
    for generation in rows {
        if generation["accepted"]!=true {chain=0;prior_commit.clear();covered.clear();
            proposed_model_methods.clear();witnessed_inherited_method=false;
            independently_planned=0;continue;}
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else {
            chain=0;prior_commit.clear();covered.clear();
            proposed_model_methods.clear();witnessed_inherited_method=false;
            independently_planned=0;continue;
        };
        let Some(generator)=generation["generatorCommit"].as_str() else {return false;};
        let Some(accepted)=generation["acceptedCommit"].as_str() else {return false;};
        let Some(binary)=generation["generatorBinarySha256"].as_str() else {return false;};
        if chain>0 && (generator!=prior_commit || binary==prior_binary) {
            chain=0;covered.clear();independently_planned=0;
            proposed_model_methods.clear();witnessed_inherited_method=false;
        }
        let model=&generation["systemBlueprint"];
        let design=&generation["systemDesign"];
        let plan=&generation["descendantPlan"];
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        let independently_verified=
            generation["pairedRevalidated"]==true
            && crate::engine::rsi::verified_pair_receipt(state,&candidate)
            && candidate["status"]=="verified"
            && candidate["origin"]=="model_driven_detached_worktree"
            && candidate["experimentAdoption"]["commit"]==accepted
            && candidate["baseCommit"]==generator
            && generation["candidateOrigin"]==candidate["origin"]
            && generation["systemEvolution"]==true
            && generation["systemBlueprintVerified"]==true
            && model["generatorCommit"]==generator
            && model["architecture"].as_array().is_some_and(|a|
                (7..=64).contains(&a.len()))
            && model["sourceTreeSha256"].as_str().is_some_and(|h|
                h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
            && plan["source"]=="compiled_descendant_system_controller_v3"
            && plan["generatorCommit"]==generator
            && plan["systemBlueprint"]==*model
            && design["source"]=="model_authored_system_design"
            && design["generatorCommit"]==generator
            && design["generationJobId"]==generation["jobId"]
            && design_implemented(design,&changed)
            && design["targetFiles"].as_array().is_some_and(|targets|
                targets.iter().all(|target|target.as_str().is_some_and(|path|
                    generation["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|hash|hash.len()==64
                            && hash.bytes().all(|b|b.is_ascii_hexdigit())))))
            && generation["evaluation"]["metrics"]["eligible"]==true
            && generation["evaluation"]["metrics"]["regressions"]==0;
        if !independently_verified {
            chain=0;prior_commit.clear();covered.clear();independently_planned=0;
            continue;
        }
        // A new executable improvement method must be authored by an
        // independently ADOPTED model generation, then ACTUALLY run by a
        // later compiled descendant with witnessed verified postconditions.
        // Merely registering a procedure or replaying an engine's fixed
        // repair recipe cannot earn the claimed method-learning transfer.
        for execution in generation["methodExecutions"].as_array()
            .into_iter().flatten() {
            let observed=execution["result"]["verified"]==true
                && execution["result"]["followed"]==true
                && execution["result"]["status"]=="followed"
                && execution["operationIds"].as_array()
                    .is_some_and(|actions|!actions.is_empty());
            if observed {
                if let (Some(id),Some(revision))=(
                    execution["methodId"].as_str(),execution["revision"].as_str()) {
                    if proposed_model_methods.contains(&(id.to_string(),revision.to_string()))
                        // The existing method execution scorer binds a
                        // procedure's actual source-write operation SHA to
                        // the independently adopted candidate. Read that
                        // witnessed causal receipt, not a model's report.
                        && state.store.all("experience").iter().any(|record|
                            record["executableMethods"].as_array().is_some_and(|methods|
                                methods.iter().any(|method|
                                    method["id"]==id && method["revision"]==revision
                                    && method["applications"].as_array().is_some_and(|apps|
                                        apps.iter().any(|episode|
                                            episode["jobId"]==generation["jobId"]
                                            && episode["methodWroteAcceptedCandidate"]==true
                                            && episode["verificationVerdict"]=="passed"))))) {
                        witnessed_inherited_method=true;
                    }
                }
            }
        }
        for proposal in generation["methodProposals"].as_array()
            .into_iter().flatten() {
            if proposal["sourceGenerationJobId"]==generation["jobId"]
                && proposal["synthesis"].is_null()
                && proposal["verification"]["type"]=="procedure"
                && proposal["verification"]["nodes"].as_array()
                    .is_some_and(|nodes|!nodes.is_empty())
                && proposal["revision"].as_str().is_some_and(|hash|
                    hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit())) {
                if let (Some(id),Some(revision))=(
                    proposal["id"].as_str(),proposal["revision"].as_str()) {
                    proposed_model_methods.insert((id.to_string(),revision.to_string()));
                }
            }
        }
        // Source coverage follows the MODEL'S validated, completely
        // implemented design programme, not a hard-coded file-name lookup.
        // A changed file only counts for its explicitly assigned role;
        // invented roles remain evolvable without faking the six core ones.
        for role in design_roles(design) {
            if design_role_files(design,&role).is_some_and(|files|
                files.iter().all(|file|file.as_str().is_some_and(|path|
                    changed.iter().any(|entry|entry==path)
                    && generation["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|sha|sha.len()==64
                            && sha.bytes().all(|b|b.is_ascii_hexdigit()))))) {
                covered.insert(role);
            }
        }
        if chain>0 {independently_planned+=1;}
        chain+=1;
        prior_commit=accepted.to_string();
        prior_binary=binary.to_string();
        // The whole intelligence stack includes the model adapter/training
        // architecture, not only the harness and improver. A novel optional
        // role may be introduced without pretending it replaces a core one.
        if chain>=3 && independently_planned>=2
            && witnessed_inherited_method
            // Finite full-system credit additionally needs a witnessed
            // improvement principle that makes its rebuilt descendant
            // independently improve a DISTINCT intelligence subsystem.
            && crate::engine::rsi_principle_lab::established_transfer(rows)
            && ["algorithms","harness","memory","learning","tools",
                "models","improvement_methods"].iter().all(|role|covered.contains(*role)) {
            return true;
        }
    }
    false
}

