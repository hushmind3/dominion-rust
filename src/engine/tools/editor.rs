//! Native indexed code search and deterministic source-edit gateways.
//! Checkpoint and preflight policy remain part of the editing contract.
use super::*;

pub(super) fn search_code_index(
    ctx: &ToolContext<'_>,
    query: &str,
    limit: usize,
) -> Result<Value, String> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Err("code_search query는 비어 있을 수 없습니다.".into());
    }
    let session=ctx.store.get("session",ctx.session_id).unwrap_or(Value::Null);
    let bound=session["rsiCodeIndexId"].as_str()
        .filter(|_|session["rsiRun"]==true)
        .and_then(|id|ctx.store.get("code_index",id))
        .filter(|meta|meta["sourceDir"].is_string() && meta["sourceDir"]==session["rsiSourceDir"]);
    // Never search whichever unrelated repository happened to be indexed last.
    let meta=match bound.or_else(||crate::engine::code_index::get(ctx.state,ctx.workspace)) {
        Some(meta)=>meta,
        None=>crate::engine::code_index::refresh(ctx.state,ctx.workspace)?,
    };
    let index_id = meta["id"].as_str().unwrap_or("");
    let terms=needle.split_whitespace().map(|term|term.trim_matches(['`','\"','\'','(',')',',']))
        .filter(|term|!term.is_empty() && !matches!(*term,"fn"|"pub"|"async"|"struct"|"enum"))
        .collect::<Vec<_>>();
    let score=|value:&str|{
        let value=value.to_lowercase();
        if value==needle {1000} else {
            u64::from(value.contains(&needle))*200 + terms.iter().map(|term|
                if value==*term {100} else {u64::from(value.contains(term))*20}).sum::<u64>()
        }
    };
    let mut matches=Vec::new();
    for file in ctx.store.all("code_index_file") {
        if file["indexId"].as_str()!=Some(index_id) {continue;}
        let path=file["path"].as_str().unwrap_or("");
        if session["rsiRun"]==true && crate::engine::rsi::sealed_measurement_path(path) {
            continue;
        }
        let rank=score(path);
        if rank>0 {matches.push((rank,json!({"path":path,"kind":"file","name":path,"line":1})));}
        for symbol in file["symbols"].as_array().into_iter().flatten() {
            let name=symbol["name"].as_str().unwrap_or("");let rank=score(name);
            if rank==0 {continue;}
            matches.push((rank,json!({"path":path,"kind":symbol["kind"],"name":name,"line":symbol["line"]})));
        }
    }
    matches.sort_by(|(a,left),(b,right)|b.cmp(a)
        .then_with(||left["path"].as_str().cmp(&right["path"].as_str()))
        .then_with(||left["line"].as_u64().cmp(&right["line"].as_u64())));
    matches.dedup_by(|(_,a),(_,b)|a==b);
    matches.truncate(limit);
    let matches=matches.into_iter().map(|(_,row)|row).collect::<Vec<_>>();
    Ok(json!({"query":query,"indexId":index_id,"indexedFiles":meta["files"],"matchCount":matches.len(),"matches":matches}))
}

pub(super) async fn replace_text(
    ctx: &ToolContext<'_>,
    path: &Path,
    old_text: &str,
    new_text: &str,
    expected_sha256: Option<&str>,
    run_id: &str,
) -> Result<Value, String> {
    if old_text.is_empty() {
        return Err("oldText는 비어 있을 수 없습니다.".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let occurrences = text.matches(old_text).count();
    if occurrences != 1 {
        return Ok(json!({
            "error": "oldText가 파일 안에서 정확히 한 번만 나타나야 합니다.",
            "occurrences": occurrences,
            "path": path.to_string_lossy(),
        }));
    }
    let updated = text.replacen(old_text, new_text, 1);
    if updated==text {
        return Ok(json!({"error":"코드를 실제로 변경하지 않았습니다.",
            "noChange":true,"written":false,"replaced":false}));
    }
    let mut result = checked_write(ctx, path, &updated, run_id, true, expected_sha256).await?;
    // A blocked preflight returns Ok({error, preflightBlocked}) instead of
    // throwing. Do not label that result a completed replacement: the RSI
    // coordinator must distinguish an attempted edit from bytes on disk.
    result["replaced"] = json!(result["written"] == true);
    if result["written"] == true {
        result["replacedChars"] = json!(old_text.chars().count());
        // Expected post-image digest; effect::seal re-reads independent bytes
        // before claiming the replacement is a verified source change.
        result["postSha256"] = json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    }
    Ok(result)
}

/// Source-range editing uses the same workspace_replace permission and audit
/// path as exact-text replacement. The small model can provide the numbered
/// source span rather than reconstructing 50KB of source or perfectly echoing
/// a long oldText block. It must prove which file revision it observed.
/// A model can choose the right source file and author Rust without
/// serializing every coordinate of a source editing tool perfectly.
/// Normalize only edits whose target is uniquely determined by the authored
/// Rust text or by an explicitly supplied line. This is NOT a built-in
/// improvement operator; actual bytes still need CAS, compilation and the
/// unchanged independent paired evaluator.
#[derive(Debug,PartialEq,Eq)]
pub(super) enum RsiEditIntent {
    CompleteFunction(String),
    SingleLine(u64),
}

pub(super) fn rsi_unambiguous_edit_intent(args:&Value,new_text:&str)->Option<RsiEditIntent>{
    let line=|key:&str|args[key].as_u64().or_else(||
        args[key].as_str()?.parse::<u64>().ok());
    if line("endLine").is_some_and(|n|n>0) {return None;}
    if new_text.len()<=16_000 && new_text.trim_end().ends_with('}') {
        let functions=rsi_top_level_functions(new_text);
        let first=functions.first()?;
        if functions.len()==1 && new_text.trim_start()
            .starts_with(first.1.trim_start()) {
            return Some(RsiEditIntent::CompleteFunction(first.0.clone()));
        }
    }
    let start=line("startLine").filter(|line|*line>0)?;
    let single=new_text.trim_end_matches(['\n','\r']);
    (!single.contains('\n') && !single.is_empty())
        .then_some(RsiEditIntent::SingleLine(start))
}

pub(super) fn rsi_pinned_source_sha(
    ctx:&ToolContext<'_>,path:&Path,args:&Value
)->Option<String>{
    if args["expectedSha256"].as_str().is_some() {return None;}
    let session=ctx.store.get("session",ctx.session_id)?;
    if session["rsiRun"]!=true {return None;}
    let index=session["rsiCodeIndexId"].as_str()?;
    let relative=path.strip_prefix(ctx.workspace).ok()?.to_str()?;
    crate::engine::code_index::indexed_source_hash(ctx.state,index,relative)
}

pub(super) async fn replace_line_span(
    ctx:&ToolContext<'_>, path:&Path, args:&Value, new_text:&str, run_id:&str,
)->Result<Value,String> {
    // Small local models sometimes serialize a precise numbered range as
    // strings. Normalize only integer line coordinates; never guess a range.
    let line=|key:&str| {
        args[key].as_u64().or_else(||args[key].as_str()?.parse::<u64>().ok())
    };
    let original_start=line("startLine").filter(|n|*n>0)
        .ok_or("workspace_replace에는 oldText 또는 startLine이 필요합니다.")?;
    let original_end=line("endLine").filter(|n|*n>0)
        .ok_or("endLine은 1 이상의 실제 소스 줄 번호여야 합니다.")?;
    // A model may reverse the two *absolute* endpoints. Reordering them
    // preserves the same unambiguous set of source lines, unlike guessing
    // that a short endLine is a relative length. The file CAS and 120-line
    // limit below remain mandatory for this normalization.
    let (start,end)=(original_start.min(original_end),original_start.max(original_end));
    let reordered=original_start>original_end;
    if end-start>=120 {
        return Err("한 번에 최대 120줄까지만 바꿀 수 있습니다.".into());
    }
    // Recover an omitted source hash only from a persisted repository index
    // that was bound to this RSI session, never from the model's prose.
    // The observed file bytes must still match that frozen index below.
    let session=ctx.store.get("session",ctx.session_id).unwrap_or(Value::Null);
    let indexed_sha=if args["expectedSha256"].is_null()
        && session["rsiRun"]==true {
        session["rsiCodeIndexId"].as_str().and_then(|id|
            path.strip_prefix(ctx.workspace).ok()
                .and_then(|relative|relative.to_str())
                .and_then(|relative|crate::engine::code_index::indexed_source_hash(
                    ctx.state,id,relative)))
    } else {None};
    let expected=args["expectedSha256"].as_str().or(indexed_sha.as_deref())
        .filter(|sha|sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
        .ok_or("줄 범위 수정에는 workspace_read_range 또는 현재 색인의 sourceSha256이 필요합니다.")?;
    if new_text.len()>32*1024 {
        return Err("줄 범위 새 코드는 최대 32KiB입니다.".into());
    }
    let source=tokio::fs::read_to_string(path).await
        .map_err(|e|format!("{}: {e}",path.display()))?;
    if source.len()>1_500_000 {
        return Err("소스 파일이 1.5MB를 초과합니다.".into());
    }
    let observed=format!("{:x}",Sha256::digest(source.as_bytes()));
    if !observed.eq_ignore_ascii_case(expected) {
        return Ok(json!({
            "error":"읽은 뒤 소스가 변경되었습니다. 같은 파일의 현재 범위를 다시 읽으세요.",
            "staleVersion":true,"expectedSha256":expected,"observedSha256":observed
        }));
    }
    // split_inclusive preserves the untouched prefix, suffix, and CRLF bytes.
    let lines=source.split_inclusive('\n').collect::<Vec<_>>();
    let first=start as usize;
    let last=end as usize;
    if first==0 || last>lines.len() {
        return Ok(json!({"error":"읽은 범위가 파일의 현재 줄 수를 초과합니다.",
            "startLine":start,"endLine":end,"totalLines":lines.len()}));
    }
    let mut inserted=new_text.to_string();
    if !inserted.is_empty() && !inserted.ends_with('\n')
        && lines[last-1].ends_with('\n') {
        inserted.push('\n');
    }
    let updated=format!("{}{}{}",
        lines[..first-1].concat(),inserted,lines[last..].concat());
    if updated==source {
        return Ok(json!({"error":"지정한 범위의 코드가 바뀌지 않았습니다.",
            "noChange":true,"written":false}));
    }
    let mut result=checked_write(ctx,path,&updated,run_id,true,Some(expected)).await?;
    result["replaced"]=json!(result["written"]==true);
    if result["written"]==true {
        result["replacedLines"]=json!(end-start+1);
        result["editMode"]=json!("versioned_line_span");
        result["postSha256"]=json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
        result["sourceHashRecoveredFromIndex"]=json!(indexed_sha.is_some());
        result["reorderedLineEndpoints"]=json!(reordered);
    }
    Ok(result)
}

/// Extract existing unindented Rust functions without rewriting unrelated
/// modules. An ambiguous nested/generic signature is rejected rather than
/// pretending an unsafe approximate span is a verified model edit.
pub(super) fn rsi_top_level_functions(source:&str)->Vec<(String,&str)> {
    let mut starts=Vec::<(String,usize)>::new();
    let mut offset=0usize;
    for line in source.split_inclusive('\n') {
        let text=line.trim_end_matches(['\r','\n']);
        if let Some((prefix,tail))=text.split_once("fn ") {
            if matches!(prefix,""|"pub "|"pub(crate) "|"async "|
                "pub async "|"pub(crate) async "|"unsafe "|"pub unsafe "|
                "const "|"pub const ") {
                let name=tail.chars().take_while(|c|c.is_ascii_alphanumeric()||*c=='_')
                    .collect::<String>();
                if !name.is_empty() && tail[name.len()..].trim_start().starts_with('(') {
                    starts.push((name,offset));
                }
            }
        }
        offset+=line.len();
    }
    starts.iter().enumerate().map(|(i,(name,start))|{
        let end=starts.get(i+1).map_or(source.len(),|(_,next)|*next);
        (name.clone(),&source[*start..end])
    }).collect()
}

/// Derived from Sub1's verified source-function tool. It uses a complete,
/// model-authored function and exact original SHA, never a predefined
/// optimization, and returns an actual written-byte receipt. The ordinary
/// independent paired runner alone decides whether it improves a capability.
pub(super) async fn replace_compiled_rust_function(
    ctx:&ToolContext<'_>,path:&Path,function:&str,new_text:&str,
    expected_sha256:&str,run_id:&str
)->Result<Value,String> {
    if path.extension().and_then(|s|s.to_str())!=Some("rs") {
        return Err("workspace_replace_function requires a Rust source file".into());
    }
    if function.is_empty() || !function.chars().next()
        .is_some_and(|c|c.is_ascii_alphabetic()||c=='_')
        || !function.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')
        || expected_sha256.len()!=64
        || !expected_sha256.bytes().all(|c|c.is_ascii_hexdigit()) {
        return Err("workspace_replace_function requires a valid function name and observed SHA".into());
    }
    if new_text.is_empty() || new_text.len()>16000 || !new_text.trim_end().ends_with('}') {
        return Err("workspace_replace_function requires exactly one complete Rust function".into());
    }
    let repl=rsi_top_level_functions(new_text);
    if repl.len()!=1 || repl[0].0!=function {
        return Err("workspace_replace_function expects a whole matching top-level function".into());
    }
    let original=tokio::fs::read_to_string(path).await.map_err(|e|e.to_string())?;
    let expected=expected_sha256.to_ascii_lowercase();
    let observed=format!("{:x}",Sha256::digest(original.as_bytes()));
    if observed!=expected {
        return Ok(json!({"error":"Source changed since observation",
            "staleVersion":true,"observedSha256":observed,"expectedSha256":expected}));
    }
    let matches=rsi_top_level_functions(&original).into_iter()
        .filter(|(name,_)|name==function).collect::<Vec<_>>();
    if matches.len()!=1 {
        return Err("workspace_replace_function did not find one unique existing target".into());
    }
    let old=matches[0].1;
    let Some(close)=old.rfind("\n}\n") else {
        return Err("workspace_replace_function cannot identify a complete final closing brace".into());
    };
    let suffix=&old[close+3..];
    let updated=original.replacen(old,
        &format!("{}\n{}",new_text.trim_end(),suffix),1);
    if updated==original {
        return Err("workspace_replace_function produced no changed source".into());
    }
    let mut receipt=checked_write(ctx,path,&updated,run_id,true,Some(&expected)).await?;
    receipt["replaced"]=json!(receipt["written"]==true);
    receipt["replacedFunction"]=json!(function);
    receipt["editMode"]=json!("sha_bound_full_function");
    if receipt["written"]==true {
        receipt["postSha256"]=json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    }
    Ok(receipt)
}

pub(super) async fn checked_write(
    ctx: &ToolContext<'_>,
    path: &Path,
    content: &str,
    run_id: &str,
    workspace_write: bool,
    expected_sha256: Option<&str>,
) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let policy = preflight_policy(ctx, path, run_id);
    let canonical = canonical_target(path);
    let lock_path = match &canonical {
        Ok(path) => path.as_path(),
        Err(error) if policy.is_some() => return Err(error.to_string()),
        Err(_) => path,
    };
    let lock = {
        let mut locks = ctx.state.file_mutations.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(lock_path).and_then(std::sync::Weak::upgrade) {
            lock
        } else {
            let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
            locks.insert(lock_path.to_path_buf(), std::sync::Arc::downgrade(&lock));
            lock
        }
    };
    let _guard = lock.lock().await;
    let Some(policy) = policy else {
        // Even without the opt-in guard, a caller that supplies a frozen
        // version gets a compare-before-write under the same per-file lock.
        let Some(expected) = expected_sha256 else {
            return write_text(path, content).await;
        };
        let canonical = canonical.map_err(|error| error.to_string())?;
        let bytes = match tokio::fs::read(&canonical).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(json!({
                    "error": format!("파일을 다시 읽지 못했습니다: {error}"),
                    "staleVersion": true,
                    "expectedSha256": expected,
                }))
            }
        };
        let observed = format!("{:x}", Sha256::digest(&bytes));
        if observed != expected {
            return Ok(json!({
                "error": "파일이 읽은 뒤 변경되었습니다. 다시 읽고 판단하세요.",
                "staleVersion": true,
                "expectedSha256": expected,
                "observedSha256": observed,
                "path": path.to_string_lossy(),
            }));
        }
        tokio::fs::write(&canonical, content)
            .await
            .map_err(|error| format!("{}: {error}", path.display()))?;
        return Ok(json!({
            "path": path.to_string_lossy(),
            "bytes": content.len(),
            "written": true,
            "conditionalSha256": observed,
        }));
    };
    let canonical = canonical.map_err(|error| error.to_string())?;
    let blocked = |reason: &str| {
        json!({"error": format!("Preflight: {reason}. Read this exact file with workspace_read or file_read, then retry with its current content."),
            "preflightBlocked": true, "preflightRevision": policy["revision"]})
    };
    let target = canonical.to_string_lossy();
    let operations = ctx.store.execution_operations(run_id)?;
    let previous = operations.iter().rev().find(|operation| {
        (operation["status"] == "completed" || operation["result"]["preflightBlocked"] == true)
            && (operation["fileObservation"]["path"].as_str() == Some(target.as_ref())
                || operation["result"]["preflightPath"].as_str() == Some(target.as_ref()))
    });
    let metadata = match tokio::fs::metadata(&canonical).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if previous.is_some() {
                return Ok(blocked("the previously observed target disappeared"));
            }
            if let Some(parent) = canonical
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            // An absent target can be created, but must not overwrite a file
            // that appeared between the probe and the open.
            let mut file = match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&canonical)
                .await
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(blocked("the target now exists"));
                }
                Err(error) => return Err(error.to_string()),
            };
            file.write_all(content.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            file.flush().await.map_err(|error| error.to_string())?;
            return Ok(
                json!({"path": path.to_string_lossy(), "bytes": content.len(), "written": true,
                "preflightPath": target}),
            );
        }
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Err("Preflight target is not a regular file".into());
    }
    let revision = previous.and_then(|operation| operation["fileObservation"]["sha256"].as_str());
    let inline_observation = workspace_write
        && policy["parameters"]["kernel"] == crate::engine::preflight_fixture::GUARD_KERNEL
        && metadata.len() <= 16 * 1024;
    if revision.is_none() && !inline_observation {
        return Ok(blocked("there is no successful read of the existing target in this run"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&canonical)
        .await
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let read_limit = if revision.is_none() { 16 * 1024 + 1 } else { 1_500_001 };
    (&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    let observed = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() > 1_500_000 || revision != Some(observed.as_str()) {
        let mut result = blocked(if revision.is_some() {
            "the target changed after the read"
        } else {
            "there is no successful read of the existing target in this run"
        });
        if inline_observation && bytes.len() <= 16 * 1024 {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                result["error"] = json!("Preflight: write skipped. The current file is below. Apply only the requested change and retry this write using the same path.");
                result["currentContent"] = json!(text);
                result["fileObservation"] = json!({"path":target,"sha256":observed});
            }
        }
        return Ok(result);
    }
    if let Some(expected) = expected_sha256 {
        if expected != observed {
            let mut result = blocked("the target changed after the frozen version");
            result["staleVersion"] = json!(true);
            result["expectedSha256"] = json!(expected);
            result["observedSha256"] = json!(observed);
            return Ok(result);
        }
    }
    use tokio::io::AsyncSeekExt;
    file.rewind().await.map_err(|error| error.to_string())?;
    file.write_all(content.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    file.flush().await.map_err(|error| error.to_string())?;
    file.set_len(content.len() as u64)
        .await
        .map_err(|error| error.to_string())?;
    Ok(
        json!({"path": path.to_string_lossy(), "bytes": content.len(), "written": true,
        "preflightRevision": policy["revision"], "preflightPath": target, "observedSha256": observed}),
    )
}

pub(super) async fn write_text(path: &Path, content: &str) -> Result<Value, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes": content.len(),
        "written": true
    }))
}

