//! A successful RSI method can itself be optimized. This produces a new
//! executable procedure from verified execution receipts, not an edit claim.
//! Only a later independent trial can prove the optimized method is better.
use crate::{engine::{code_index,procedure,rsi,rsi_evolve},state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

fn valid_sha(value:&str)->bool {
    value.len()==64 && value.bytes().all(|c|c.is_ascii_hexdigit())
}

/// Source attribution must match the exact adopted file *postimage*, not
/// merely a tool call or an edit to another file in the same generation.
fn adopted_write(state:&AppState,method:&Value,receipt:&Value,
    ops:&[Value],generation:&Value)->bool {
    if generation["accepted"]!=true
        || generation["acceptedProof"]["status"]!="verified"
        || generation["acceptedProof"]["evaluation"]["metrics"]["eligible"]!=true
        || receipt["result"]["verified"]!=true
        || receipt["result"]["followed"]!=true {return false;}
    let Some(candidate)=generation["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{return false;};
    if candidate["status"]!="verified"
        || candidate["evaluation"]["metrics"]["eligible"]!=true {return false;}
    let Some(paths)=candidate["paths"].as_array() else{return false;};
    let Some(ids)=receipt["operationIds"].as_array() else{return false;};
    ops.iter().filter(|op|
        ids.contains(&op["id"])
        && op["origin"]=="procedure"
        && op["procedureStrategyId"]==method["id"]
        && op["procedureRevision"]==method["revision"]
        && matches!(op["tool"].as_str(),
            Some("workspace_replace"|"workspace_replace_range"|"workspace_write"))
        && op["status"]=="completed" && op["result"]["written"]==true
        && op["toolReceipt"]["verified"]==true)
    .any(|op|{
        let Some(path)=op["arguments"]["path"].as_str() else{return false;};
        if !paths.iter().any(|p|p.as_str()==Some(path)){return false;}
        let Some(adopted)=generation["acceptedSourceHashes"][path].as_str()
            .filter(|hash|valid_sha(hash)) else{return false;};
        let post=op["result"]["postSha256"].as_str().map(str::to_string)
            .or_else(||if op["tool"]=="workspace_write"{
                op["arguments"]["content"].as_str().map(|body|
                    format!("{:x}",Sha256::digest(body.as_bytes())))
            }else{None});
        post.as_deref().is_some_and(|hash|
            valid_sha(hash) && hash.eq_ignore_ascii_case(adopted))
    })
}

/// Transfer an independently adopted *operator*, never a donor's file path,
/// source text, or unmeasured quality claim.
fn donor(state:&AppState,job:&Value,domain:&str)->Option<Value>{
    let scope=rsi_evolve::rsi_training_partition(&job["input"]);
    let mut candidates=state.store.all("experience").into_iter()
        .filter(|record|record["status"]=="active"
            && record["partitionKey"]==scope)
        .flat_map(|record|record["executableMethods"].as_array()
            .cloned().unwrap_or_default())
        .filter(|method|
            matches!(method["synthesis"]["kind"].as_str(),
                Some("successful_read_ablation"|"transferred_read_ablation"))
            && method["domain"].as_str().is_some_and(|d|d!=domain)
            && method["status"]=="executable"
            && method["successfulApplications"].as_u64().unwrap_or(0)>0
            && method["synthesis"]["transferDepth"].as_u64().unwrap_or(0)<3
            && method["applications"].as_array().is_some_and(|rows|
                rows.iter().any(|row|row["independentGain"]==true)))
        .collect::<Vec<_>>();
    candidates.sort_by(|a,b|
        b["successfulApplications"].as_u64().unwrap_or(0).cmp(
            &a["successfulApplications"].as_u64().unwrap_or(0))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    candidates.into_iter().next().map(|method|json!({
        "methodId":method["id"],"revision":method["revision"],
        "originDomain":method["domain"],
        "transferDepth":method["synthesis"]["transferDepth"]
    }))
}

/// A pure structural rewrite. Never erase a read node with multiple
/// consumers, dynamic dependencies or required postconditions.
fn eliminate_single_read(graph:&Value,read_id:&str,edit_id:&str)->Option<Value>{
    let nodes=graph["nodes"].as_array()?;
    let edges=graph["edges"].as_array()?;
    if nodes.len()<2{return None;}
    let read=nodes.iter().find(|n|n["id"]==read_id && n["tool"]=="workspace_read_range")?;
    let edit=nodes.iter().find(|n|n["id"]==edit_id && n["tool"]=="workspace_replace")?;
    if edit["arguments"]["path"]!=read["arguments"]["path"]
        || edit["arguments"]["oldText"].as_str().is_some_and(|s|!s.is_empty())
        || edit["arguments"]["startLine"].as_u64().unwrap_or(0)==0
        || edit["arguments"]["endLine"].as_u64().unwrap_or(0)
            <edit["arguments"]["startLine"].as_u64().unwrap_or(1)
        || edit["arguments"]["expectedSha256"]!=
            format!("{}{}.sourceSha256",char::from(36),read_id){return None;}
    let binding=format!("{}{}.",char::from(36),read_id);
    let mut args=edit["arguments"].clone();
    args["expectedSha256"]=Value::Null;
    if args.to_string().contains(&binding)
        || edges.iter().any(|e|
            (e["from"]==read_id && e["to"]!=edit_id) || e["to"]==read_id)
        || !edges.iter().any(|e|e["from"]==read_id && e["to"]==edit_id)
        || nodes.iter().any(|n|n["id"]!=read_id && n["id"]!=edit_id
            && n["arguments"].to_string().contains(&binding))
        || graph["before"].as_array().into_iter().flatten()
            .any(|p|p.to_string().contains(read_id))
        || graph["after"].as_array().into_iter().flatten()
            .any(|p|p["tool"]=="workspace_read_range"
                || p.to_string().contains(&binding)) {return None;}
    let mut updated=graph.clone();
    updated["nodes"].as_array_mut()?.retain(|n|n["id"]!=read_id);
    let edited=updated["nodes"].as_array_mut()?.iter_mut()
        .find(|n|n["id"]==edit_id)?;
    edited["arguments"]["expectedSha256"]=Value::Null;
    updated["edges"].as_array_mut()?.retain(|edge|
        edge["from"]!=read_id && edge["to"]!=read_id);
    let normalized=procedure::normalize(&updated);
    (normalized["type"]=="procedure").then_some(normalized)
}

pub fn propose(state:&AppState,method:&Value,receipt:&Value,
    ops:&[Value],generation:&Value)->Option<Value>{
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || receipt["result"]["verified"]!=true
        || receipt["result"]["followed"]!=true
        || receipt["result"]["status"]=="violated" {return None;}
    let accepted=adopted_write(state,method,receipt,ops,generation);
    let job=state.store.get("job",generation["jobId"].as_str()?)?;
    let domain=method["domain"].as_str()?;
    let transferred=if accepted {None}else{donor(state,&job,domain)};
    if !accepted && transferred.is_none(){return None;}
    let index_id=job["input"]["rsiIndexId"].as_str()?;
    let index=state.store.get("code_index",index_id)?;
    if index["sourceDir"]!=job["input"]["rsiSourceDir"]{return None;}
    let nodes=method["verification"]["nodes"].as_array()?;
    let ids=receipt["operationIds"].as_array()?;
    for read in nodes.iter().filter(|n|n["tool"]=="workspace_read_range"){
        let read_id=read["id"].as_str()?;
        let path=read["arguments"]["path"].as_str()?;
        if !rsi::safe_path(path)
            || code_index::indexed_source_hash(state,index_id,path).is_none(){continue;}
        let edit_candidates=nodes.iter().filter(|n|
            n["tool"]=="workspace_replace"
            && n["arguments"]["path"]==path
            && n["arguments"]["expectedSha256"]==
                format!("{}{}.sourceSha256",char::from(36),read_id))
            .collect::<Vec<_>>();
        if edit_candidates.len()!=1{continue;}
        let edit_id=edit_candidates[0]["id"].as_str()?;
        let Some(program)=eliminate_single_read(&method["verification"],read_id,edit_id)
            else{continue;};
        let observed=ops.iter().filter(|op|
            ids.contains(&op["id"]) && op["origin"]=="procedure"
            && op["procedureStrategyId"]==method["id"]
            && op["procedureRevision"]==method["revision"]
            && op["procedureNodeId"]==read_id
            && op["tool"]=="workspace_read_range"
            && procedure::operation_succeeded(op))
            .filter_map(|op|op["result"]["sourceSha256"].as_str())
            .find(|sha|valid_sha(sha));
        let Some(sha)=observed else{continue;};
        let edited=ops.iter().any(|op|
            ids.contains(&op["id"]) && op["origin"]=="procedure"
            && op["procedureStrategyId"]==method["id"]
            && op["procedureRevision"]==method["revision"]
            && op["procedureNodeId"]==edit_id
            && op["tool"]=="workspace_replace"
            && procedure::operation_succeeded(op)
            && op["result"]["written"]==true
            && op["arguments"]["path"]==path
            && op["arguments"]["expectedSha256"].as_str()
                .is_some_and(|value|value.eq_ignore_ascii_case(sha)));
        if !edited{continue;}
        let revision=format!("{:x}",Sha256::digest(
            json!([domain,&program]).to_string().as_bytes()));
        if revision==method["revision"]{continue;}
        return Some(json!({"id":format!("rsi-method-{revision}"),
            "revision":revision,
            "name":format!("{} · success-ablation",
                method["name"].as_str().unwrap_or("Learned method")
                    .chars().take(65).collect::<String>()),
            "domain":domain,"verification":program,
            "status":"candidate","assistanceMode":"harness_assisted",
            "successfulApplications":0,
            "synthesis":{"kind":if transferred.is_some() {
                    "transferred_read_ablation"
                }else{"successful_read_ablation"},
                "parentMethodId":method["id"],"parentRevision":method["revision"],
                "sourceExecutionId":receipt["executionId"],
                "sourceGenerationJobId":generation["jobId"],
                "removedReadNode":read_id,"retainedEditNode":edit_id,
                "sourceIndexId":index_id,"sourcePath":path,
                "transferDonor":transferred,
                "transferDepth":transferred.as_ref().map(|x|
                    x["transferDepth"].as_u64().unwrap_or(0)+1).unwrap_or(0),
                "qualityGain":Value::Null,
                "causalProof":"requires_independent_descendant_trial"
            }}));
    }
    None
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn ablation_preserves_source_write_but_removes_redundant_read(){
        let graph=procedure::normalize(&json!({"type":"procedure",
            "appliesTo":{"requestContains":["memory"]},
            "nodes":[{"id":"read","tool":"workspace_read_range",
                "arguments":{"path":"src/engine/memory.rs","startLine":1,"endLine":1}},
                {"id":"edit","tool":"workspace_replace",
                    "arguments":{"path":"src/engine/memory.rs","startLine":1,
                        "endLine":1,"newText":"test",
                        "expectedSha256":"$read.sourceSha256"}}],
            "edges":[{"from":"read","to":"edit"}],
            "after":[{"kind":"file_contains","path":"src/engine/memory.rs","text":"test"}]}));
        let optimized=eliminate_single_read(&graph,"read","edit").unwrap();
        assert_eq!(optimized["nodes"].as_array().unwrap().len(),1);
        assert_eq!(optimized["nodes"][0]["arguments"]["expectedSha256"],Value::Null);
        assert_eq!(optimized["edges"],json!([]));
    }
    #[test]
    fn cannot_remove_read_required_by_another_consumer(){
        let graph=json!({"type":"procedure","nodes":[
            {"id":"read","tool":"workspace_read_range",
                "arguments":{"path":"src/a.rs"}},
            {"id":"edit","tool":"workspace_replace",
                "arguments":{"path":"src/a.rs","startLine":1,"endLine":1,
                    "expectedSha256":"$read.sourceSha256"}},
            {"id":"other","tool":"code_search","arguments":{}}],
            "edges":[{"from":"read","to":"edit"},{"from":"read","to":"other"}]});
        assert!(eliminate_single_read(&graph,"read","edit").is_none());
    }
}