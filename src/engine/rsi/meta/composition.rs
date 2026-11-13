//! One typed ownership boundary for the compiled descendant's meta-system.
//!
//! Selects and composes experiment design, model-authored open programs,
//! recursive protocols, cross-role influence, meta-learning and method reuse.
//! The planner consumes the resulting decision; the supervisor validates the
//! original program against source snapshots instead of recomputing ranking.
use super::*;

pub(super) struct MetaComposition {
    pub(super) method_transfer: Value,
    pub(super) feedback_repair: bool,
    pub(super) influence: Value,
    pub(super) principle_learning: Value,
    pub(super) api_route_policy: Value,
    pub(super) factorial: Value,
    pub(super) evolution_program: Value,
    pub(super) peers: Value,
    pub(super) mutual_program: Value,
    pub(super) reflexive_program: Value,
    pub(super) open_program: Value,
    pub(super) meta_policy: Value,
    pub(super) architecture_target: Value,
    pub(super) co_design: Value,
    pub(super) successor_stage: Value,
}

pub(super) fn compose(
    state:&AppState,input:&Value,history:&[Value],frontier:&[Value],
    allowed:&[&str],commit:&str,campaign:&str,lineage:&Value,
    inherited:Option<&str>,inherited_stage:Option<&Value>,explicit:bool,
)->Result<MetaComposition,String>{
    let inputs=super::meta_inputs::prepare(state,input,history,campaign,explicit)?;
    Ok(compose_pure(input,history,frontier,allowed,commit,lineage,
        inherited,inherited_stage,explicit,inputs))
}

/// This stage has no AppState, job store, filesystem access or evaluator.
/// Descendants can replace its strategy and experiment scheduling without
/// acquiring mutation authority over source adoption or frozen profiles.
fn compose_pure(
    input:&Value,history:&[Value],frontier:&[Value],allowed:&[&str],
    commit:&str,lineage:&Value,inherited:Option<&str>,
    inherited_stage:Option<&Value>,explicit:bool,
    inputs:super::meta_inputs::PreparedMetaInputs,
)->MetaComposition {
    let method_transfer=inputs.method_transfer;
    let feedback_repair=history.iter().rev().find(|g|g["accepted"]==true)
        .is_some_and(|last|
        input["rsiMethodAblation"]==true && input["rsiAutonomous"]==true
        && !explicit
        && last["accepted"]==true && last["domain"]!="improver"
        && last["methodAblation"]["methodAssociationObserved"]==true
        && last["methodAblation"]["qualityProven"]==true);
    let influence=crate::engine::rsi_interaction::influence_graph(&history,&frontier);
    // Independent task-quality and descendant capability measurements.
    let principle_learning=crate::engine::rsi_principle_learning::study(&history);
    // The newly compiled descendant chooses between owner-authorized API
    // execution models. DeepSeek weights remain external and immutable.
    let api_route_policy=if input["rsiAdaptiveApiRouting"]==true {
        crate::engine::rsi_api_route_learning::decide(
            &history,&input["rsiApprovedApiRoutes"],
            &input["rsiOriginalApiRoute"],commit)
    }else{Value::Null};
    // Derived inside the REBUILT descendant from real paired source changes.
    // It identifies which frozen A, B, A+B variant is not yet measured.
    let factorial=crate::engine::rsi_factorial::design(
        &history,&frontier,&influence,&allowed,commit);
    // Real model-authored meta operators, free of a fixed A/B/A/B stage list.
    // An old supervisor's post-adoption cost target is a suggestion,
    // NOT an owner constraint. The genuinely adopted successor program
    // may supersede it while preserving the frozen evaluation profile.
    let evolution_program=if explicit || input["rsiAutonomous"]!=true {Value::Null}
        else {crate::engine::rsi_evolve::meta_evolution::compile(
            &history,&frontier,&allowed,commit)};
    // A later independently adopted, freely authored protocol may supersede
    // the older fixed-order method curriculum. The two programs must not
    // assign mutually exclusive edit targets to one worker.
    // The new graph normally supersedes the legacy fixed curriculum.
    // An explicit method_control operator is the exception: its experiment
    // must execute the real inherited method and a separate disabled arm.
    let uses_inherited_control=evolution_program["operator"]["experiment"]=="method_control"
        && evolution_program["target"]["domain"]==method_transfer["domain"]
        && method_transfer["methodId"].is_string();
    let method_transfer=if evolution_program.is_object() && !uses_inherited_control {
        Value::Null
    }else{method_transfer};
    let feedback_repair=feedback_repair && evolution_program.is_null();
    let peers=inputs.peers;
    // Crucial: the *compiled descendant*, rather than the resident server,
    // scores possible meta-improvement targets and proposes its next experiment.
    let unsteered=crate::engine::rsi_meta_controller::synthesize(
        &history,&frontier,&allowed,inherited.as_deref(),&lineage,&peers,&influence);
    let factorial_steered=if explicit || paired_descendant_target(input) {
        unsteered
    }else{
        crate::engine::rsi_meta_controller::steer_factorial(unsteered,&factorial)
    };
    // Manual source assignments and sealed downstream A/B episodes are not
    // autonomous reciprocal research. They must never mint an A->B->A stage.
    let mutual_program=if explicit || paired_descendant_target(input)
        || evolution_program.is_object() {
        Value::Null
    }else{
        crate::engine::rsi_mutual_program::propose(
            &history,&frontier,commit,&factorial_steered["target"],&influence,&allowed)
    };
    let prior_meta=crate::engine::rsi_meta_controller::steer_reciprocal(
        factorial_steered,&mutual_program);
    // A model-authored graph can invent NEW causal improvement sequences,
    // branches and self-rewrites. It is executable only after its source
    // revision is independently adopted and recompiled.
    // A more recently adopted, model-authored open program may supersede
    // a legacy fixed-stage continuation; otherwise the old reciprocal
    // continuation remains the fallback. Source adoption, not stage rank,
    // determines whether a newly invented improvement policy can take over.
    let open_program=if explicit || paired_descendant_target(input)
        || evolution_program.is_object() || input["rsiAutonomous"]!=true {
        Value::Null
    }else{
        crate::engine::rsi_program_graph::synthesize(
            &history,&frontier,&allowed,commit)
    };
    // Learning failure is evidence of an inadequate improvement ALGORITHM,
    // so it must be able to interrupt a speculative fixed seed. Previously
    // accepted, executing reciprocal complement/return work stays intact.
    let learner_repair=principle_learning["predictiveCalibration"]
        ["needsLearnerSourceRevision"]==true
        && evolution_program.is_null() && open_program.is_null()
        && input["rsiAutonomous"]==true && !explicit
        && !paired_descendant_target(input)
        && !matches!(mutual_program["phase"].as_str(),Some("complement"|"return"));
    // The CURRENT compiled descendant can schedule a repair of its own
    // prequential task-quality predictor. This is a source-edit objective;
    // the independent evaluator and user-assigned work remain authoritative.
    let quality_repair=prior_meta["qualityPredictorCalibration"]
        ["needsPredictorSourceRevision"]==true
        && !learner_repair && evolution_program.is_null() && open_program.is_null()
        && input["rsiAutonomous"]==true && !explicit
        && !paired_descendant_target(input)
        && allowed.contains(&"improver")
        && !matches!(mutual_program["phase"].as_str(),Some("complement"|"return"));
    // The earlier fixed reflexive curriculum is a fallback, not a ceiling
    // over dynamically authored successor improvement programs.
    let reflexive_program=if learner_repair || quality_repair || evolution_program.is_object()
        || open_program.is_object()
        || explicit || paired_descendant_target(input)
        || input["rsiAutonomous"]!=true
        || matches!(mutual_program["phase"].as_str(),Some("complement"|"return")) {
        Value::Null
    }else{
        crate::engine::rsi_reflexive::propose(
            &history,&frontier,commit,&allowed,&prior_meta["target"],&factorial)
    };
    let initial_policy=crate::engine::rsi_meta_controller::steer_open_program(
        crate::engine::rsi_meta_controller::steer_reflexive(
            prior_meta,&reflexive_program),&open_program);
    let meta_policy=if learner_repair {
        crate::engine::rsi_meta_controller::steer_learner_repair(
            initial_policy,&principle_learning,&frontier,&allowed)
    }else if quality_repair {
        crate::engine::rsi_meta_controller::steer_quality_predictor_repair(
            initial_policy,&history,&frontier,&allowed)
    }else{crate::engine::rsi_evolve::meta_dispatch::steer(
        initial_policy,&evolution_program)};
    // If a reflexive source experiment has priority, do not also advertise
    // a conflicting speculative mutual-improvement stage to the worker.
    let mutual_program=if reflexive_program.is_object() || open_program.is_object()
        || evolution_program.is_object() {
        Value::Null
    }else{mutual_program};
    let architecture_target=if explicit
        || (paired_descendant_target(input) && evolution_program.is_null()) {
        Value::Null
    }else if meta_policy["target"].is_object() {
        meta_policy["target"].clone()
    }else{
        frontier.iter().find(|row|row["domain"].as_str().is_some_and(|domain|
            allowed.contains(&domain) && inherited.as_deref().is_none_or(|pinned|pinned==domain)))
            .cloned().unwrap_or(Value::Null)
    };
    // A whole-system successor can design interlocking improvements in
    // memory, harness, learning and meta-search in the SAME generation.
    // Explicit owner-assigned targets remain a higher-priority constraint.
    let coupled_domains=if input["rsiAutonomous"]==true && !explicit
        && !paired_descendant_target(input) {
        DOMAINS.to_vec()
    }else{allowed.to_vec()};
    let co_design=if architecture_target.is_object() {
        crate::engine::rsi_interaction::coupled_portfolio(
            &architecture_target,&frontier,&influence,&coupled_domains)
    }else{Value::Null};
    let successor_stage=if architecture_target["inheritedProgramStage"]==true
        && inherited_stage.as_ref().is_some_and(|stage|
            stage["role"]==architecture_target["role"]
            && stage["path"]==architecture_target["path"]
            && stage["sourceSha256"]==architecture_target["sourceSha256"]) {
        inherited_stage.cloned().unwrap_or(Value::Null)
    }else{Value::Null};
    MetaComposition {
        method_transfer,
        feedback_repair,
        influence,
        principle_learning,
        api_route_policy,
        factorial,
        evolution_program,
        peers,
        mutual_program,
        reflexive_program,
        open_program,
        meta_policy,
        architecture_target,
        co_design,
        successor_stage,
    }
}
