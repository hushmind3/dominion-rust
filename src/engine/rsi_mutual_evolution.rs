//! Cross-component self-development programme of the compiled successor.
//! This is a source-editable hypothesis generator: observed continuation,
//! co-design, and an authored next-stage intention are different evidence.
//! An external paired evaluator alone decides whether any change improved.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

#[derive(Default,Clone)]
struct EdgeEvidence {
    authored_stages:u64,
    coupled_adoptions:u64,
    direct_successor_attempts:u64,
    direct_successor_adoptions:u64,
    return_stage_attempts:u64,
    return_stage_adoptions:u64,
}

fn roles_of(g:&Value)->Vec<String>{
    let design=&g["systemDesign"];
    if design["source"]!="model_authored_system_design"
        || design["generationJobId"]!=g["jobId"]
        || design["generatorCommit"]!=g["generatorCommit"] {
        return Vec::new();
    }
    let parts=if design["component"]=="system"{
        design["components"].as_array().cloned().unwrap_or_default()
    }else{vec![design["component"].clone()]};
    parts.iter().filter_map(Value::as_str)
        .filter(|name|!name.is_empty()&&name.len()<=64
            && name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
            && *name!="system")
        .map(str::to_string).collect::<BTreeSet<_>>().into_iter().collect()
}

fn accepted(g:&Value)->bool{
    if g["accepted"]!=true || g["pairedRevalidated"]!=true
        || g["evaluation"]["metrics"]["eligible"]!=true
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || g["acceptedCommit"].as_str().is_none_or(|s|s.len()!=40
            || !s.bytes().all(|c|c.is_ascii_hexdigit())){
        return false;
    }
    let paths=g["acceptedSourceHashes"].as_object().map(|m|
        m.keys().map(|path|json!(path)).collect::<Vec<_>>())
        .unwrap_or_default();
    crate::engine::rsi_system::design_implemented(&g["systemDesign"],&paths)
}

/// Portfolio of potential mutual improvements, grounded in source revisions
/// and independent adoption receipts. Edge rank is for exploration only.
/// A predecessor->child observed gain is not claimed to be causal attribution.
pub(crate) fn graph(history:&[Value],frontier:&[Value])->Value{
    let roles=frontier.iter().filter_map(|row|row["role"].as_str())
        .filter(|role|!role.is_empty()&&role.len()<=64)
        .map(str::to_string).collect::<BTreeSet<_>>();
    let recent=history.iter().rev().take(96).collect::<Vec<_>>();
    let mut attempts=BTreeMap::<String,u64>::new();
    let mut proven=BTreeMap::<String,u64>::new();
    let mut evidence=BTreeMap::<(String,String),EdgeEvidence>::new();
    for g in &recent {
        let attributed=roles_of(g);
        if attributed.is_empty(){continue;}
        for role in &attributed{
            if roles.contains(role){
                *attempts.entry(role.clone()).or_default()+=1;
                if accepted(g){*proven.entry(role.clone()).or_default()+=1;}
            }
        }
        if !accepted(g){continue;}
        for from in &attributed{
            for stage in g["systemDesign"]["successorStages"]
                .as_array().into_iter().flatten(){
                let Some(to)=stage["role"].as_str() else{continue};
                if from!=to && roles.contains(from) && roles.contains(to){
                    evidence.entry((from.clone(),to.into())).or_default()
                        .authored_stages+=1;
                }
            }
            for to in &attributed{
                if from!=to && roles.contains(from)&&roles.contains(to){
                    evidence.entry((from.clone(),to.clone())).or_default()
                        .coupled_adoptions+=1;
                }
            }
        }
        let Some(accepted_commit)=g["acceptedCommit"].as_str() else{continue};
        for later in &recent{
            if later["generatorCommit"].as_str()!=Some(accepted_commit){continue;}
            // A compiled, source-focused successor may fail BEFORE it authors
            // another model design. Count that as an ATTEMPT, not as a gain.
            let mut attempted=roles_of(later).into_iter()
                .collect::<BTreeSet<_>>();
            let chosen=&later["successorStage"];
            let grounded=later["metaPolicyExecuted"]==true
                && chosen["predecessorCommit"]==g["acceptedCommit"]
                && chosen["sourceDesignId"]==g["systemDesign"]["id"]
                && chosen["path"].as_str().is_some_and(|path|
                    later["focusFiles"].as_array().is_some_and(|files|
                        files.iter().any(|file|file.as_str()==Some(path))));
            if grounded{
                if let Some(role)=chosen["role"].as_str(){
                    attempted.insert(role.to_string());
                }
            }
            for from in &attributed{
                for to in &attempted {
                    if *from==*to || !roles.contains(from)||!roles.contains(to){continue;}
                    let edge=evidence.entry((from.clone(),to.clone())).or_default();
                    edge.direct_successor_attempts+=1;
                    let adopted=accepted(later)
                        && roles_of(later).iter().any(|role|role==to);
                    if adopted{edge.direct_successor_adoptions+=1;}
                    if grounded && chosen["phase"]=="return_improvement_to_primary" {
                        edge.return_stage_attempts+=1;
                        if adopted{edge.return_stage_adoptions+=1;}
                    }
                }
            }
        }
    }
    let mut candidates=Vec::<(i64,Value)>::new();
    for from in &roles{
        for to in &roles{
            if from==to{continue;}
            let count=evidence.get(&(from.clone(),to.clone()))
                .cloned().unwrap_or_default();
            let origin=proven.get(from).copied().unwrap_or(0);
            let destination=proven.get(to).copied().unwrap_or(0);
            let tries=attempts.get(to).copied().unwrap_or(0);
            let failed_followups=count.direct_successor_attempts
                .saturating_sub(count.direct_successor_adoptions);
            // An open target reached from an already improved source is a
            // strong exploration opportunity, not a measured transfer.
            let score=45*i64::from(origin>0)
                +35*i64::from(destination==0)
                +25*(count.direct_successor_adoptions.min(3) as i64)
                +30*(count.return_stage_adoptions.min(3) as i64)
                +12*(count.authored_stages.min(3) as i64)
                +8*(count.coupled_adoptions.min(3) as i64)
                -8*(failed_followups.min(4) as i64)
                -(tries.min(6) as i64)*3;
            let focus=frontier.iter().find(|row|row["role"]==to);
            candidates.push((score,json!({
                "from":from,"to":to,"targetDomain":focus.map(|r|&r["domain"]),
                "targetPath":focus.map(|r|&r["path"]),
                "targetSourceSha256":focus.map(|r|&r["sourceSha256"]),
                "sourceAcceptedGenerations":origin,
                "targetAcceptedGenerations":destination,
                "targetAttemptedGenerations":tries,
                "authoredSuccessorStages":count.authored_stages,
                "coupledAdoptions":count.coupled_adoptions,
                "observedDirectSuccessorAttempts":count.direct_successor_attempts,
                "observedDirectSuccessorAdoptions":count.direct_successor_adoptions,
                "observedReturnStageAttempts":count.return_stage_attempts,
                "observedReturnStageAdoptions":count.return_stage_adoptions,
                "priority":score,
                "causalAttributionProven":false,
                "qualityProvenForThisDesign":false
            })));
        }
    }
    candidates.sort_by(|(a,left),(b,right)|b.cmp(a)
        .then_with(||left["to"].as_str().cmp(&right["to"].as_str()))
        .then_with(||left["from"].as_str().cmp(&right["from"].as_str())));
    json!({
        "source":"compiled_mutual_intelligence_development_v1",
        "roleCount":roles.len(),
        "observedGenerations":recent.len(),
        "edges":candidates.into_iter().take(96).map(|(_,row)|row)
            .collect::<Vec<_>>(),
        "interpretation":"source-bound cross-role exploration, not causal proof",
        "qualityProven":false
    })
}


/// Turn a statistical cross-role graph into a CURRENT, SOURCE-GROUNDED
/// three-phase programme. A phase is an experiment, never a proof of gain.
/// This routine belongs to the compiled descendant and can evolve with it.
pub(crate) fn curriculum(frontier:&[Value],meta:&Value,allowed:&[&str])->Value{
    let main=&meta["target"];
    let graph=&meta["mutualDevelopmentGraph"];
    let primary=frontier.iter().find(|row|row["role"]==main["role"]
        && row["path"]==main["path"]
        && row["sourceSha256"]==main["sourceSha256"])
        .cloned().unwrap_or(Value::Null);
    let mut pairs=Vec::<(f64,Value)>::new();
    if primary.is_object(){
        for partner in frontier.iter().filter(|row|
            row["role"]!=primary["role"] && row["path"]!=primary["path"]
                && row["sourceSha256"].as_str().is_some_and(|sha|sha.len()==64)
                && row["domain"].as_str().is_some_and(|domain|allowed.contains(&domain))) {
            let a=primary["role"].as_str().unwrap_or("");
            let b=partner["role"].as_str().unwrap_or("");
            let evidence=graph["edges"].as_array();
            let forward=evidence.into_iter().flatten().find(|e|
                e["from"]==a && e["to"]==b);
            let backward=evidence.into_iter().flatten().find(|e|
                e["from"]==b && e["to"]==a);
            let known=forward.and_then(|e|e["observedDirectSuccessorAdoptions"].as_u64())
                .unwrap_or(0).min(4)
                +backward.and_then(|e|e["observedDirectSuccessorAdoptions"].as_u64())
                    .unwrap_or(0).min(4)
                +backward.and_then(|e|e["observedReturnStageAdoptions"].as_u64())
                    .unwrap_or(0).min(4);
            let attempts=forward.and_then(|e|e["observedDirectSuccessorAttempts"].as_u64())
                .unwrap_or(0)
                +backward.and_then(|e|e["observedDirectSuccessorAttempts"].as_u64())
                    .unwrap_or(0);
            let meta_partner=a=="improvement_methods" || b=="improvement_methods";
            let untouched=partner["verifiedRoleImproved"]!=true;
            let reciprocal=(known as f64+1.0)/(attempts as f64+2.0);
            let score=0.35*reciprocal
                +if meta_partner{0.24}else{0.07}
                +if untouched{0.20}else{0.0}
                +0.10/(1.0+attempts as f64)
                -((partner["sourceBytes"].as_u64().unwrap_or(0) as f64)/200_000.0)
                    .min(0.10);
            pairs.push((score,json!({
                "fromRole":a,"toRole":b,
                "fromPath":primary["path"],"toPath":partner["path"],
                "fromSourceSha256":primary["sourceSha256"],
                "toSourceSha256":partner["sourceSha256"],
                "fromDomain":primary["domain"],"toDomain":partner["domain"],
                "score":score,"observedBidirectionalSuccessors":known,
                "observedAttempts":attempts,"causalityProven":false
            })));
        }
    }
    pairs.sort_by(|(a,first),(b,second)|b.total_cmp(a)
        .then_with(||first["toRole"].as_str().cmp(&second["toRole"].as_str()))
        .then_with(||first["toPath"].as_str().cmp(&second["toPath"].as_str())));
    let pair=pairs.first().map(|(_,v)|v.clone()).unwrap_or(Value::Null);
    let phases=if pair.is_object(){
        json!([{
            "phase":"redesign_primary",
            "role":pair["fromRole"],"path":pair["fromPath"],
            "sourceSha256":pair["fromSourceSha256"]
        },{
            "phase":"redesign_partner_to_improve_primary",
            "role":pair["toRole"],"path":pair["toPath"],
            "sourceSha256":pair["toSourceSha256"]
        },{
            "phase":"return_improvement_to_primary",
            "role":pair["fromRole"],"path":pair["fromPath"],
            "sourceRebindRequired":true
        }])
    }else{json!([])};
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([&primary,&pair,&phases,graph["source"]]))
            .unwrap_or_default()));
    json!({
        "source":"compiled_reciprocal_intelligence_curriculum_v1",
        "revision":revision,"primary":primary,"pair":pair,"phases":phases,
        "candidatePartners":pairs.len(),
        "bundleRequiresBothWrittenSources":true,
        "returnStageRequiresAcceptedParentAndFreshSource":true,
        "feedbackTarget":"improve_future_ability_to_improve_other_components",
        "qualityProven":false,"causalityProven":false
    })
}

pub(crate) fn instruction(curriculum:&Value)->String {
    let pair=&curriculum["pair"];
    if !pair.is_object(){return String::new();}
    format!(
        "[RECIPROCAL INTELLIGENCE REDESIGN revision={}]\n\
        Current indexed primary {} at {} (sourceSha256={}) and partner {} \
        at {} (sourceSha256={}) form the planned loop {}.\n\
        You must AUTHOR the causal design yourself. To claim a coupled \
        design, use rsi_system_plan(component=system, components, \
        componentFiles, interactionEdges, successorStages), then actually \
        edit BOTH source files; source receipts and frozen evaluation decide \
        adoption. interactionEdges must bind the actual two source files \
        and the hypothesized mechanism. Set the next descendant's stage \
        toward the complementary system; propose the return stage only after \
        the observed descendant can continue. A source model and policy \
        declaration alone are not improvements.\n",
        curriculum["revision"],pair["fromRole"],pair["fromPath"],
        pair["fromSourceSha256"],pair["toRole"],pair["toPath"],
        pair["toSourceSha256"],curriculum["phases"]
    )
}


/// If an independently adopted A->B source redesign was followed by a
/// genuinely adopted B-generation source change, the next compiled child may
/// rebind A to CURRENT source and run the third, returning improvement stage.
/// This is derived from source receipts, not merely an announced loop.
pub(crate) fn reciprocal_return(history:&[Value],frontier:&[Value])->Option<Value>{
    let child=history.iter().rev().find(|g|accepted(g))?;
    let parent=history.iter().rev().find(|g|accepted(g)
        && g["acceptedCommit"]==child["generatorCommit"]
        && g["jobId"]!=child["jobId"])?;
    if child["metaPolicyExecuted"]!=true && child["compiledPlanExecuted"]["source"]
        !="running_compiled_descendant_binary" {return None;}
    for edge in parent["systemDesign"]["interactionEdges"]
        .as_array().into_iter().flatten(){
        let (Some(from),Some(to),Some(path),Some(to_path))=(
            edge["fromRole"].as_str(),edge["toRole"].as_str(),
            edge["fromPath"].as_str(),edge["toPath"].as_str())else{continue};
        let planned=parent["systemDesign"]["successorStages"].as_array()
            .is_some_and(|stages|stages.iter().any(|stage|
                stage["role"]==to && stage["path"]==to_path));
        if !planned || !roles_of(child).iter().any(|r|r==to)
            || child["successorStage"]["sourceDesignId"]!=parent["systemDesign"]["id"]
            || child["successorStage"]["role"]!=to
            || child["successorStage"]["path"]!=to_path
            || child["acceptedSourceHashes"][to_path].as_str()
                .is_none_or(|h|h.len()!=64)
            || parent["acceptedSourceHashes"][path].as_str()
                .is_none_or(|h|h.len()!=64){continue;}
        let Some(current)=frontier.iter().find(|row|
            row["role"]==from && row["path"]==path)else{continue};
        let objective=format!(
            "Apply the independently adopted improvement to {} from {} to redesign the \
             {} subsystem's future improvement mechanism; originating hypothesis: {}",
            to,child["acceptedCommit"],from,edge["mechanism"].as_str()
                .unwrap_or("cross-subsystem learning"));
        return Some(json!({
            "phase":"return_improvement_to_primary",
            "role":from,"domain":current["domain"],"path":path,
            "sourceSha256":current["sourceSha256"],
            "objective":objective.chars().take(1200).collect::<String>(),
            "sourceDesignId":child["systemDesign"]["id"],
            "predecessorCommit":child["acceptedCommit"],
            "interactionOriginCommit":parent["acceptedCommit"],
            "interactionOriginalDesignId":parent["systemDesign"]["id"],
            "interactionFromRole":from,"interactionToRole":to,
            "learningPolicy":"avoid_failed","toolPolicy":"allow_new_modules",
            "executionPolicy":"explore_then_edit",
            "qualityProven":false,
            "actualTransferGainProven":false
        }));
    }
    None
}

/// The external runner validates the immutable ancestor/descendant relationship
/// without imposing the old descendant's particular scheduling equation.
pub(crate) fn valid_return_lineage(history:&[Value],stage:&Value)->bool{
    if stage["phase"]!="return_improvement_to_primary" {return false;}
    history.iter().any(|child|accepted(child)
        && child["systemDesign"]["id"]==stage["sourceDesignId"]
        && child["acceptedCommit"]==stage["predecessorCommit"]
        && (child["metaPolicyExecuted"]==true
            || child["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary")
        && history.iter().any(|parent|accepted(parent)
            && parent["acceptedCommit"]==child["generatorCommit"]
            && parent["acceptedCommit"]==stage["interactionOriginCommit"]
            && parent["systemDesign"]["id"]==stage["interactionOriginalDesignId"]
            && parent["systemDesign"]["interactionEdges"].as_array()
                .is_some_and(|edges|edges.iter().any(|edge|
                    edge["fromRole"]==stage["role"]
                    && edge["fromRole"]==stage["interactionFromRole"]
                    && edge["toRole"]==stage["interactionToRole"]
                    && edge["fromPath"]==stage["path"]
                    && parent["acceptedSourceHashes"][edge["fromPath"]
                        .as_str().unwrap_or("")].is_string()
                    && child["acceptedSourceHashes"][edge["toPath"]
                        .as_str().unwrap_or("")].is_string()
                    && child["successorStage"]["sourceDesignId"]==
                        parent["systemDesign"]["id"]
                    && child["successorStage"]["role"]==edge["toRole"]
                    && child["successorStage"]["path"]==edge["toPath"]
                    && roles_of(child).iter().any(|role|
                        Some(role.as_str())==edge["toRole"].as_str())
                    && parent["systemDesign"]["successorStages"].as_array()
                        .is_some_and(|stages|stages.iter().any(|s|
                            s["role"]==edge["toRole"]
                            && s["path"]==edge["toPath"]))
                )))
    )
}
