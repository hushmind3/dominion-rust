//! Synthesize executable next-generation recovery DAGs from failed RSI
//! procedure operations. Nothing is promoted by synthesis alone.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

fn stale_read_rebase(method:&Value,op:&Value)->Option<Value> {
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || op["origin"]!="procedure" || op["tool"]!="workspace_replace"
        || op["status"]!="failed" || op["result"]["staleVersion"]!=true
        || op["procedureStrategyId"]!=method["id"]
        || op["procedureRevision"]!=method["revision"] {return None;}
    let path=op["arguments"]["path"].as_str()?;
    if !crate::engine::rsi::safe_path(path) {return None;}
    let node_id=op["procedureNodeId"].as_str()?;
    let nodes=method["verification"]["nodes"].as_array()?;
    if nodes.is_empty() || nodes.len()>=16 {return None;}
    let node=nodes.iter().find(|row|row["id"]==node_id
        && row["tool"]=="workspace_replace" && row["arguments"]["path"]==path)?;
    if !node["arguments"]["newText"].is_string()
        || node["arguments"]["expectedSha256"].as_str()
            .is_some_and(|v|v.starts_with(char::from(36))) {return None;}
    let text_mode=node["arguments"]["oldText"].as_str().is_some_and(|v|!v.is_empty());
    let (start,end)=if text_mode {
        if node["arguments"]["oldText"]!=op["arguments"]["oldText"] {return None;}
        (1,1)
    } else {
        let n=|v:&Value|v.as_u64().or_else(||v.as_str()?.parse::<u64>().ok());
        let start=n(&node["arguments"]["startLine"])?;
        let end=n(&node["arguments"]["endLine"])?;
        if start==0 || end<start || end-start>=120
            || n(&op["arguments"]["startLine"])?!=start
            || n(&op["arguments"]["endLine"])?!=end {return None;}
        (start,end)
    };
    let fingerprint=format!("{:x}",Sha256::digest(
        json!([method["revision"],node_id,"source-rebase-v1"]).to_string().as_bytes()));
    let read_id=format!("source_rebase_{}",&fingerprint[..12]);
    if nodes.iter().any(|node|node["id"]==read_id) {return None;}
    let mut plan=method["verification"].clone();
    plan["nodes"].as_array_mut()?.insert(0,json!({
        "id":read_id,"tool":"workspace_read_range",
        "arguments":{"path":path,"startLine":start,"endLine":end}
    }));
    let replace=plan["nodes"].as_array_mut()?.iter_mut()
        .find(|node|node["id"]==node_id)?;
    replace["arguments"]["expectedSha256"]=json!(format!(
        "{}{}.sourceSha256",char::from(36),read_id));
    if !plan["edges"].is_array(){plan["edges"]=json!([]);}
    plan["edges"].as_array_mut()?.push(json!({"from":read_id,"to":node_id}));
    let plan=crate::engine::procedure::normalize(&plan);
    if plan["type"]!="procedure"{return None;}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(json!([domain,&plan]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · source-rebase",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":plan,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"stale_source_rebase",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"changedNodeId":node_id,
            "newReadNodeId":read_id},
        "qualityProven":false}))
}

fn indexed_rebase(method:&Value,op:&Value)->Option<Value>{
    let _validated=stale_read_rebase(method,op)?;
    let node_id=op["procedureNodeId"].as_str()?;
    let node=method["verification"]["nodes"].as_array()?
        .iter().find(|node|node["id"]==node_id)?;
    if node["arguments"]["oldText"].as_str().is_some_and(|text|!text.is_empty()){
        return None;
    }
    let mut plan=method["verification"].clone();
    plan["nodes"].as_array_mut()?.iter_mut()
        .find(|node|node["id"]==node_id)?["arguments"]["expectedSha256"]=Value::Null;
    let plan=crate::engine::procedure::normalize(&plan);
    if plan["type"]!="procedure"{return None;}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(json!([domain,&plan]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · indexed-rebase",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":plan,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"indexed_source_rebase",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"changedNodeId":node_id},
        "qualityProven":false}))
}

fn existing_file_repair(method:&Value,op:&Value)->Option<Value>{
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || op["origin"]!="procedure" || op["tool"]!="workspace_write"
        || op["status"]!="failed"
        || op["procedureStrategyId"]!=method["id"]
        || op["procedureRevision"]!=method["revision"] {return None;}
    let error=op["result"]["error"].as_str()
        .or_else(||op["error"].as_str())?;
    if !error.contains("workspace_write") || !error.contains("새 파일 생성 전용") {
        return None;
    }
    let path=op["arguments"]["path"].as_str()?;
    if !crate::engine::rsi::safe_path(path){return None;}
    let node_id=op["procedureNodeId"].as_str()?;
    let nodes=method["verification"]["nodes"].as_array()?;
    if nodes.is_empty() || nodes.len()>=16 {return None;}
    let node=nodes.iter().find(|node|node["id"]==node_id
        && node["tool"]=="workspace_write"
        && node["arguments"]["path"]==path
        && node["arguments"]["content"]==op["arguments"]["content"])?;
    let content=node["arguments"]["content"].as_str()
        .filter(|text|!text.is_empty() && text.chars().count()<=4096)?;
    let fingerprint=format!("{:x}",Sha256::digest(
        json!([method["revision"],node_id,"existing-file-v1"]).to_string().as_bytes()));
    let read_id=format!("source_rebase_{}",&fingerprint[..12]);
    if nodes.iter().any(|node|node["id"]==read_id){return None;}
    let mut plan=method["verification"].clone();
    plan["nodes"].as_array_mut()?.insert(0,json!({
        "id":read_id,"tool":"workspace_read_range",
        "arguments":{"path":path,"startLine":1,"endLine":120}
    }));
    let target=plan["nodes"].as_array_mut()?.iter_mut()
        .find(|node|node["id"]==node_id)?;
    target["tool"]=json!("workspace_replace");
    target["arguments"]=json!({"path":path,"startLine":1,
        "endLine":format!("{}{}.totalLines",char::from(36),read_id),
        "expectedSha256":format!("{}{}.sourceSha256",char::from(36),read_id),
        "newText":content});
    for postcondition in plan["after"].as_array_mut()? {
        if postcondition["kind"]=="tool_called"
            && postcondition["tool"]=="workspace_write"
            && postcondition["path"]==path{
            postcondition["tool"]=json!("workspace_replace");
        }
    }
    if !plan["edges"].is_array(){plan["edges"]=json!([]);}
    plan["edges"].as_array_mut()?.push(json!({"from":read_id,"to":node_id}));
    let plan=crate::engine::procedure::normalize(&plan);
    if plan["type"]!="procedure"{return None;}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(json!([domain,&plan]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · versioned-edit",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":plan,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"existing_file_to_versioned_edit",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"changedNodeId":node_id,
            "newReadNodeId":read_id},
        "qualityProven":false}))
}

/// Failed procedure receipts can invent multiple rival recovery algorithms.
/// The ordinary experience store and independent postconditions choose them.
pub fn propose(method:&Value,op:&Value)->Vec<Value>{
    [stale_read_rebase(method,op),indexed_rebase(method,op),
        existing_file_repair(method,op)].into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cannot_synthesize_repair_from_a_model_claim(){
        let method=json!({"id":"m","revision":"r","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","nodes":[]}});
        let claim=json!({"tool":"workspace_replace","status":"completed",
            "result":{"staleVersion":true}});
        assert!(propose(&method,&claim).is_empty());
    }
}