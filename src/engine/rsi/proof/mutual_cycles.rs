//! Protected evidence replay and lineage attestation, separate from editable
//! improvement targeting and learned program synthesis.
use super::*;

pub(crate) fn completed_cycles(history:&[Value])->Value {
    let mut cycles=Vec::new();
    let mut seen=BTreeSet::new();
    for returned in history.iter().filter(|g|
        adopted_source(g) && g["mutualProgram"]["phase"]=="return") {
        let program=&returned["mutualProgram"];
        let Some(root_id)=program["rootGenerationId"].as_str()
            .filter(|s|!s.is_empty()) else{continue};
        let Some(root)=history.iter().find(|g|g["id"]==root_id
            && adopted_source(g)
            && g["mutualProgram"]["phase"]=="seed") else{continue};
        let (Some(origin),Some(partner))=(
            program["origin"]["role"].as_str(),
            program["partner"]["role"].as_str()) else{continue};
        if origin==partner || !implemented_roles(root).iter().any(|r|r==origin)
            || !implemented_roles(returned).iter().any(|r|r==origin) {continue;}
        let Some(middle)=history.iter().find(|g|
            adopted_source(g)
            && g["generatorCommit"]==root["acceptedCommit"]
            && g["acceptedCommit"]==returned["generatorCommit"]
            && g["mutualProgram"]["phase"]=="complement"
            && g["mutualProgram"]["rootGenerationId"]==root_id
            && implemented_roles(g).iter().any(|r|r==partner))
        else{continue};
        if program["generatorCommit"]!=returned["generatorCommit"]
            || middle["mutualProgram"]["origin"]["role"]!=origin
            || middle["mutualProgram"]["partner"]["role"]!=partner {
            continue;
        }
        let Some(returned_id)=returned["id"].as_str() else{continue};
        if !seen.insert(returned_id.to_owned()){continue;}
        cycles.push(json!({
            "originGenerationId":root_id,"complementGenerationId":middle["id"],
            "returnGenerationId":returned["id"],
            "sourceRole":origin,"complementRole":partner,
            "sourceCommit":root["acceptedCommit"],
            "complementCommit":middle["acceptedCommit"],
            "returnCommit":returned["acceptedCommit"],
            "allThreeIndependentAdoptions":true,
            "causalityProven":false
        }));
    }
    let mut result=json!({"source":"independently_adopted_reciprocal_lineage_v1",
        "completedCycles":cycles.len(),"cycles":cycles,
        "claim":"three_real_sequential_adoptions_not_proof_of_causal_effect"});
    result["sha256"]=json!(sha(&result));
    result
}
