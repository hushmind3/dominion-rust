//! reciprocal stage of the compiled, revisable meta-improvement controller.
//! These observations propose experiments; only frozen independent evaluation may adopt them.
use super::*;

/// Construct a *two-way* source-edit experiment from independently adopted
/// parent -> rebuilt-child role transitions. A joint edit is not automatically
/// a causal gain: both directions remain hypotheses until separately tested.
/// The role graph, traversal, and scoring are part of the editable descendant,
/// never part of the frozen external evaluator.
pub(super) fn reciprocal_evolution_contract(
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
            + (if reversal_prior{24.0}else{0.0})
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
pub(super) fn adopted_reciprocal_reversal(history:&[Value],frontier:&[Value])
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

