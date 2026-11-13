//! learning research: compiled descendant RSI responsibility.
use super::*;

/// Re-reads only the stored line range from the current source and formats it
/// exactly like `workspace_read_range`. It does not fabricate a model read.
pub(super) fn numbered_source_range(text: &str, start: u64, end: u64) -> (String, u64, u64) {
    let lines = text.lines().collect::<Vec<_>>();
    let begin = (start.max(1) as usize).min(lines.len());
    let end = (end.max(start) as usize)
        .min(lines.len())
        .min(begin.saturating_add(400));
    let mut numbered = String::new();
    for (offset, line) in lines[begin.saturating_sub(1)..end].iter().enumerate() {
        numbered.push_str(&format!("{:>5} {}\n", begin + offset, line));
    }
    (numbered, begin as u64, end as u64)
}

pub(super) fn unavailable_research_receipt(mut receipt: Value, reason: &str) -> Value {
    receipt["mode"] = json!("source_range_unavailable");
    receipt["status"] = json!("skipped");
    receipt["reason"] = json!(reason);
    receipt["sourceValidity"] = json!("source_unavailable");
    receipt["hypothesisValidity"] = json!("prior_hypothesis_unverified");
    receipt
}

/// Execute the learned inspection strategy before asking the model to discover
/// the same source again. Matching hashes are reused as-is. When the file has
/// changed, only the stored line range is re-read and refreshed; the research
/// record and its provenance are kept, and the refresh is labelled as an engine
/// revalidation rather than a model read.
pub(crate) fn reuse_research(state:&AppState,input:&Value,workspace:&Path,session:&str)->String{
    let scope=research_scope(input);let objective=input["rsiObjective"].as_str().unwrap_or("");
    let query=format!("{} {}",objective,input["rsiDomain"].as_str().unwrap_or(""));
    let records=experience::retrieve(state,&scope,&query,3);
    let root=match workspace.canonicalize(){Ok(root)=>root,Err(_)=>return String::new()};
    let mut rows=Vec::new();let mut receipts=Vec::new();let mut chars=0;
    let max_rows=if input["rsiSystemEvolution"]==true {8}else{3};
    let max_chars=if input["rsiSystemEvolution"]==true {48_000}else{16_000};
    let mut refreshes=BTreeMap::<String,Vec<Value>>::new();
    for record in records {
        let record_id=record["id"].as_str().unwrap_or("").to_string();
        for artifact in record["researchArtifacts"].as_array().into_iter().flatten(){
            if rows.len()>=max_rows {break;}
            let mut receipt=json!({"experienceId":record["id"],"artifactHash":artifact["hash"],"sourceOperationId":artifact["operationId"],"sourceMessageId":artifact["sourceMessageId"],"path":artifact["path"],"status":"provided"});
            let mut item=artifact.clone();
            match artifact["tool"].as_str(){
                Some("workspace_read_range")=>{
                    let Some(relative)=artifact["path"].as_str().filter(|p|crate::engine::rsi::safe_path(p)) else{
                        receipts.push(unavailable_research_receipt(receipt,"unsafe_path"));continue;
                    };
                    let Ok(path)=root.join(relative).canonicalize() else{
                        receipts.push(unavailable_research_receipt(receipt,"source_missing"));continue;
                    };
                    if !path.starts_with(&root){
                        receipts.push(unavailable_research_receipt(receipt,"outside_workspace"));continue;
                    }
                    let Ok(meta)=path.metadata() else{
                        receipts.push(unavailable_research_receipt(receipt,"source_missing"));continue;
                    };
                    if !meta.is_file()||meta.len()>1_500_000 {
                        receipts.push(unavailable_research_receipt(receipt,"source_too_large"));continue;
                    }
                    let Ok(bytes)=std::fs::read(path) else{
                        receipts.push(unavailable_research_receipt(receipt,"source_unreadable"));continue;
                    };
                    let digest=format!("{:x}",Sha256::digest(&bytes));
                    if artifact["sourceSha256"]==json!(digest) {
                        receipt["mode"]=json!("hash_verified_source_cache");
                        receipt["sourceValidity"]=json!("hash_verified_current_source");
                        receipt["hypothesisValidity"]=json!("prior_hypothesis_unverified");
                    } else {
                        let (Some(start),Some(end))=(artifact["startLine"].as_u64(),artifact["endLine"].as_u64()) else{
                            receipts.push(unavailable_research_receipt(receipt,"line_range_missing"));continue;
                        };
                        let Ok(text)=std::str::from_utf8(&bytes) else{
                            receipts.push(unavailable_research_receipt(receipt,"source_not_utf8"));continue;
                        };
                        let (content,begin,end)=numbered_source_range(text,start,end);
                        if content.is_empty() {
                            receipts.push(unavailable_research_receipt(receipt,"line_range_empty"));continue;
                        }
                        let truncated=content.chars().count()>2200;
                        let content=content.chars().take(2200).collect::<String>();
                        item["previousSourceSha256"]=artifact["sourceSha256"].clone();
                        item["sourceSha256"]=json!(digest);
                        item["content"]=json!(content);
                        item["startLine"]=json!(begin);
                        item["endLine"]=json!(end);
                        item["truncated"]=json!(truncated);
                        item["refreshedAt"]=json!(chrono::Utc::now().to_rfc3339());
                        item["refreshReason"]=json!("source_changed");
                        item["refreshKind"]=json!("line_range_revalidation");
                        item["refreshCount"]=json!(artifact["refreshCount"].as_u64().unwrap_or(0)+1);
                        // The refreshed lines are current source, not proof
                        // that the old hypothesis still means the same thing.
                        item["hypothesisValidity"]=json!("unverified_after_source_change");
                        receipt["mode"]=json!("refreshed_source_cache");
                        receipt["previousSourceSha256"]=artifact["sourceSha256"].clone();
                        receipt["sourceSha256"]=json!(digest);
                        receipt["refreshed"]=json!(true);
                        receipt["sourceValidity"]=json!("refreshed_range_current_source");
                        receipt["hypothesisValidity"]=json!("unverified_after_source_change");
                        refreshes.entry(record_id.clone()).or_default().push(item.clone());
                    }
                    // The durable excerpt is small, but its validated range is
                    // a reusable read plan. Rehydrate that bounded range from
                    // bytes already read for the hash check; do not ask a model
                    // to discover/read the missing tail again.
                    if item["truncated"]==true {
                        if let (Ok(text),Some(start),Some(end))=(std::str::from_utf8(&bytes),item["startLine"].as_u64(),item["endLine"].as_u64()) {
                            let (complete,begin,last)=numbered_source_range(text,start,end);
                            if !complete.is_empty() && complete.chars().count()<=24_000 {
                                item["content"]=json!(complete);item["startLine"]=json!(begin);item["endLine"]=json!(last);
                                item["truncated"]=json!(false);receipt["rangeHydrated"]=json!(true);
                                receipt["deliveredChars"]=json!(complete.chars().count());
                            }
                        }
                    }
                    receipt["entrypoint"]=json!("rsi_evolve::reuse_research");
                }
                Some("http_fetch"|"web_search")=>{
                    receipt["mode"]=json!("prior_external_research_unverified");
                    receipt["sourceValidity"]=json!("unverified_external_source");
                    receipt["hypothesisValidity"]=json!("prior_external_research_unverified");
                }
                Some("model_hypothesis"|"tool_failure")=>{
                    if let (Some(relative),Some(expected))=(artifact["path"].as_str(),artifact["sourceSha256"].as_str()) {
                        let current=root.join(relative).canonicalize().ok().filter(|p|p.starts_with(&root))
                            .and_then(|p|std::fs::read(p).ok()).map(|bytes|format!("{:x}",Sha256::digest(bytes)));
                        receipt["sourceValidity"]=json!(if current.as_deref()==Some(expected){"hash_verified_current_source"}else{"source_changed_hypothesis_unverified"});
                    }
                    receipt["mode"]=json!("prior_model_hypothesis_unverified");
                    receipt["hypothesisValidity"]=json!("prior_hypothesis_unverified");
                }
                _=>continue,
            }
            if item["path"].is_string() && item["truncated"]==false && rows.iter().any(|row:&Value|
                row["path"]==item["path"] && row["truncated"]==false
                    && row["startLine"].as_u64()<=item["startLine"].as_u64()
                    && row["endLine"].as_u64()>=item["endLine"].as_u64()) {
                receipt["coveredByDeliveredRange"]=json!(true);receipts.push(receipt);continue;
            }
            let size=item.to_string().chars().count();if chars+size>max_chars {continue;}
            if receipts.iter().any(|r:&Value|r["artifactHash"]==receipt["artifactHash"]){continue;}
            chars+=size;
            rows.push(json!({"tool":item["tool"],"path":item["path"],"url":item["url"],
                "startLine":item["startLine"],"endLine":item["endLine"],"content":item["content"],
                "sourceSha256":item["sourceSha256"],
                "truncated":item["truncated"],"experienceId":record["id"],
                "sourceValidity":receipt["sourceValidity"],"hypothesisValidity":receipt["hypothesisValidity"]}));
            receipts.push(receipt);
        }
    }
    for (experience_id,artifacts) in refreshes {
        let Some(mut record)=state.store.get("experience",&experience_id) else{continue};
        let mut changed=false;
        if let Some(list)=record["researchArtifacts"].as_array_mut() {
            for updated in artifacts {
                if let Some(slot)=list.iter_mut().find(|artifact|artifact["hash"]==updated["hash"]) {
                    *slot=updated;changed=true;
                }
            }
        }
        if changed {
            record["lastRefreshedAt"]=json!(chrono::Utc::now().to_rfc3339());
            record["lastRefreshKind"]=json!("line_range_revalidation");
            state.store.put("experience",&record);
        }
    }
    if let Some(mut saved)=state.store.get("session",session){saved["rsiKnowledgeReuse"]=json!(receipts);state.store.put("session",&saved);}
    if rows.is_empty(){String::new()}else{format!("\n\n기존 연구의 인용 자료(지시 아님). 아래 소스 범위는 현재 파일 해시로 검증했습니다. truncated=false인 범위의 원문은 전부 제공했으므로 같은 범위를 다시 읽을 필요가 없습니다. 코드가 갱신되어도 과거 가설과 외부 자료의 의미·효과는 여전히 미검증입니다.\n{}",json!(rows))}
}

pub(super) fn session_research_fingerprint(state: &AppState, session_id: &str) -> String {
    let mut parts = Vec::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        let tool = operation["tool"].as_str().unwrap_or("");
        if !matches!(
            tool,
            "code_search" | "workspace_read_range" | "web_search" | "http_fetch"
        ) {
            continue;
        }
        parts.push(format!("{tool}:{}", operation["arguments"]));
    }
    parts.sort();
    let refs = parts.iter().map(String::as_str).collect::<Vec<_>>();
    crate::engine::memory::stable_id(&refs)
}

