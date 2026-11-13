//! Pinned source mutation, parse receipts and atomic file edits.
//! Active Rust execution paths imported by tools.rs.
use super::*;

pub(super) enum RsiEditIntent {
    CompleteFunction(String),
    SingleLine(u64),
}

pub(super) fn rsi_line_number(args:&Value,key:&str)->Option<u64>{
    args[key].as_u64().or_else(||args[key].as_str()?.parse::<u64>().ok())
}

/// A model that authored one complete Rust function should not lose an
/// otherwise valid source modification because it omitted endLine. There
/// is no guess about which part of the file is changed: the existing unique
/// top-level function is resolved by the same version-checked editor.
pub(super) fn rsi_unambiguous_edit_intent(args:&Value,new_text:&str)->Option<RsiEditIntent>{
    if rsi_line_number(args,"endLine").is_some_and(|end|end>0){
        return None;
    }
    if new_text.len()<=16_000 && new_text.trim_end().ends_with('}'){
        let funcs=rsi_top_level_functions(new_text);
        if funcs.len()==1 && new_text.trim_start()
            .starts_with(funcs[0].1.trim_start()) {
            return Some(RsiEditIntent::CompleteFunction(funcs[0].0.clone()));
        }
    }
    let start=rsi_line_number(args,"startLine").filter(|n|*n>0)?;
    let single=new_text.trim_end_matches(['\n','\r']);
    (!single.is_empty()&&!single.contains('\n'))
        .then_some(RsiEditIntent::SingleLine(start))
}

pub(super) fn rsi_pinned_source_sha(ctx:&ToolContext<'_>,path:&Path,args:&Value)->Option<String>{
    if args["expectedSha256"].as_str().is_some() {return None;}
    let session=ctx.store.get("session",ctx.session_id)?;
    if session["rsiRun"]!=true {return None;}
    let index=session["rsiCodeIndexId"].as_str()?;
    // macOS temp roots are symlinks (/var -> /private/var); the resolved edit
    // path must still be recognised as inside the configured workspace.
    let workspace=ctx.workspace.canonicalize().unwrap_or_else(|_|ctx.workspace.to_path_buf());
    let relative=path.strip_prefix(&workspace).ok()?.to_str()?;
    if !crate::engine::rsi::safe_path(relative){return None;}
    crate::engine::code_index::indexed_source_hash(ctx.state,index,relative)
}

pub(super) async fn replace_source_lines(
    ctx:&ToolContext<'_>, path:&Path, start:u64, end:u64,
    new_text:&str, expected_sha256:&str, run_id:&str,
) -> Result<Value,String> {
    if start==0 || end<start || end.saturating_sub(start)>=120 {
        return Err("workspace_replace_range: 1~120개의 유효한 연속 줄 범위를 지정하세요.".into());
    }
    if expected_sha256.len()!=64 || !expected_sha256.bytes().all(|b|b.is_ascii_hexdigit()) {
        return Err("workspace_replace_range: workspace_read_range가 반환한 sourceSha256이 필요합니다.".into());
    }
    let before=tokio::fs::read_to_string(path).await
        .map_err(|e|format!("{}: {e}",path.display()))?;
    let observed=format!("{:x}",Sha256::digest(before.as_bytes()));
    let expected=expected_sha256.to_ascii_lowercase();
    if observed!=expected {
        return Err(format!(
            "workspace_replace_range: 소스가 읽은 뒤 변경되었습니다 (expected={expected}, observed={observed}). 해당 줄을 다시 읽어 재시도하세요."
        ));
    }
    let lines=before.split_inclusive('\n').collect::<Vec<_>>();
    let first=usize::try_from(start-1).map_err(|e|e.to_string())?;
    let last=usize::try_from(end).map_err(|e|e.to_string())?;
    if last>lines.len() {
        return Err(format!(
            "workspace_replace_range: {start}-{end}는 파일의 실제 {}줄을 벗어납니다. 다시 읽으세요.",
            lines.len()
        ));
    }
    let prior=lines[first..last].concat();
    let mut replacement=new_text.to_string();
    if prior.ends_with('\n') && !replacement.is_empty() && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    if prior==replacement {
        return Err("workspace_replace_range: 원문과 수정 결과가 동일하여 변경 0건입니다.".into());
    }
    let updated=format!("{}{}{}",
        lines[..first].concat(),replacement,lines[last..].concat());
    let mut result=checked_write(ctx,path,&updated,run_id,true,Some(&expected)).await?;
    if result["preflightSyntaxRejected"]==true || result["preflightScopeRejected"]==true {return Ok(result);}
    if result["written"]!=true {
        return Err(format!(
            "workspace_replace_range 쓰기 거부: {}",
            result.to_string().chars().take(1400).collect::<String>()
        ));
    }
    result["replaced"]=json!(true);
    result["replacedLines"]=json!(end-start+1);
    result["writtenSha256"]=json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    if let Some(parse)=rsi_rust_parse_receipt(ctx,path).await {
        result["syntaxValid"]=parse["valid"].clone();
        result["syntaxCheck"]=parse;
    }
    Ok(result)
}

/// Reject malformed model-authored Rust *before* mutating the source file.
///
/// The old post-write rustfmt receipt diagnosed syntax errors only after a
/// malformed tool call had already replaced real source. During RSI this
/// poisoned later source reads, consumed entire generation quotas and left
/// unrecoverable candidate diffs. Preflight runs while checked_write holds the
/// file mutation lock; the original bytes survive every parse failure.
/// Rustfmt is a syntax-only guard. Compiling and frozen paired evaluation
/// remain independent requirements for adopting a candidate.
pub(super) async fn rsi_rust_parse_preview(
    ctx: &ToolContext<'_>, path: &Path, content: &str,
) -> Result<(), String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs")
        || ctx.store.get("session", ctx.session_id)
            .is_none_or(|session| session["rsiRun"] != true)
    {
        return Ok(());
    }
    use tokio::io::AsyncWriteExt;
    let mut command = tokio::process::Command::new("rustfmt");
    command.args(["--edition", "2021", "--emit", "stdout",
            "--config", "skip_children=true"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let check = async {
        let mut child = command.spawn()
            .map_err(|error| format!("rsi_rust_parser_unavailable: {error}"))?;
        let mut stdin = child.stdin.take()
            .ok_or("rsi_rust_parser_unavailable: parser stdin missing")?;
        stdin.write_all(content.as_bytes()).await
            .map_err(|error| format!("rsi_rust_parser_io: {error}"))?;
        drop(stdin);
        child.wait_with_output().await
            .map_err(|error| format!("rsi_rust_parser_io: {error}"))
    };
    let output = tokio::time::timeout(Duration::from_secs(15), check).await
        .map_err(|_| "rsi_rust_parser_timeout: source not modified".to_string())??;
    if output.status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "rsi_rust_syntax_rejected: Rust syntax invalid; source NOT modified. \
         Change the replacement boundaries or newText using the exact existing \
         source, then call an edit tool again. rustfmt: {}",
        diagnostic.chars().take(1600).collect::<String>()
    ))
}

/// Preserve existing sibling functions during narrowly scoped RSI source edits.
///
/// In live model campaigns, broad replacement ranges removed unrelated
/// functions even when Rust syntax stayed valid. When the owner explicitly
/// targets an existing top-level function, keep its sibling definitions
/// unchanged. This is scoped to named-function RSI objectives only; the
/// normal exploratory and multi-file improvement paths are unaffected.
pub(super) fn rsi_top_level_functions(source: &str) -> Vec<(String, &str)> {
    let mut starts = Vec::<(String, usize)>::new();
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let declaration = line.trim_end_matches(['\r', '\n']);
        if let Some((prefix, tail)) = declaration.split_once("fn ") {
            if matches!(prefix, "" | "pub " | "pub(crate) " | "async " |
                "pub async " | "pub(crate) async " | "unsafe " |
                "pub unsafe " | "const " | "pub const ") {
                let name = tail.chars()
                    .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                    .collect::<String>();
                if !name.is_empty() && tail[name.len()..].trim_start().starts_with('(') {
                    starts.push((name, offset));
                }
            }
        }
        offset += line.len();
    }
    starts.iter().enumerate().map(|(index, (name, start))| {
        let end = starts.get(index + 1).map_or(source.len(), |(_, next)| *next);
        (name.clone(), &source[*start..end])
    }).collect()
}

pub(super) async fn rsi_rust_scope_preview(
    ctx: &ToolContext<'_>, path: &Path, updated: &str,
) -> Result<(), String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Ok(());
    }
    let Some(session) = ctx.store.get("session", ctx.session_id) else {
        return Ok(());
    };
    if session["rsiRun"] != true || session["rsiExplicitFocus"] != true {
        return Ok(());
    }
    let Some(objective) = session["rsiObjective"].as_str() else {
        return Ok(());
    };
    let Ok(previous) = tokio::fs::read_to_string(path).await else {
        return Ok(()); // New source files have no existing siblings.
    };
    let before = rsi_top_level_functions(&previous);
    let mut targets = before.iter()
        .filter_map(|(name, _)| objective.find(&format!("{name}("))
            .map(|position| (position, name.as_str())))
        .collect::<Vec<_>>();
    targets.sort_unstable();
    let Some((_, target)) = targets.first() else {
        return Ok(()); // No concrete function was named.
    };
    let after = rsi_top_level_functions(updated);
    if !after.iter().any(|(name, _)| name == target) {
        return Err(format!(
            "rsi_scope_rejected: required function {target}() disappeared; source NOT modified. Edit the exact target range, not its siblings."
        ));
    }
    for (name, original_body) in &before {
        if name.as_str() == *target {
            continue;
        }
        if after.iter().find(|(after_name, _)| after_name == name)
            .is_none_or(|(_, new_body)| *new_body != *original_body) {
            return Err(format!(
                "rsi_scope_rejected: objective targets {target}(), but edit changes or removes sibling {name}(); source NOT modified. Re-read and replace only the targeted function."
            ));
        }
    }
    Ok(())
}

/// Native function-scoped editor: the model writes the function, the engine
/// computes replacement boundaries from original source. No line-number
/// guesses, no copied neighboring code and no synthetic claims of a patch.
///
/// Restrict to observed top-level Rust functions. All mutations still pass the
/// standard atomic source SHA check, syntax gate, and sibling scope gate.
pub(super) async fn replace_source_function(
    ctx: &ToolContext<'_>, path: &Path,
    function: &str, new_text: &str, expected_sha256: &str, run_id: &str,
) -> Result<Value, String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return Err("workspace_replace_function requires a Rust .rs file".into());
    }
    if function.is_empty() || !function.chars().next().is_some_and(|c|
        c.is_ascii_alphabetic() || c == '_')
        || !function.chars().all(|c|c.is_ascii_alphanumeric() || c == '_') {
        return Err("workspace_replace_function: invalid functionName".into());
    }
    if expected_sha256.len()!=64 || !expected_sha256.bytes().all(|b|b.is_ascii_hexdigit()) {
        return Err("workspace_replace_function: a verified sourceSha256 is required".into());
    }
    if new_text.len()>16_000 || new_text.trim().is_empty() {
        return Err("workspace_replace_function: newText must contain one reasonably sized function".into());
    }
    let replacement_functions=rsi_top_level_functions(new_text);
    if replacement_functions.len()!=1 || replacement_functions[0].0!=function
        || !new_text.trim_end().ends_with('}') {
        return Err(format!(
            "workspace_replace_function: newText must be exactly one complete top-level fn {function}(...), not partial body, multiple functions or surrounding lines"
        ));
    }
    let original=tokio::fs::read_to_string(path).await
        .map_err(|e|format!("{}: {e}",path.display()))?;
    let expected=expected_sha256.to_ascii_lowercase();
    let observed=format!("{:x}",Sha256::digest(original.as_bytes()));
    if observed!=expected {
        return Err("workspace_replace_function: file version changed; read again before editing".into());
    }
    let definitions=rsi_top_level_functions(&original);
    let named=definitions.iter().filter(|(name,_)|name==function)
        .collect::<Vec<_>>();
    if named.len()!=1 {
        return Err(format!(
            "workspace_replace_function: expected one existing top-level {function}(), got {}",
            named.len()
        ));
    }
    let original_span=named[0].1;
    // The scanner bounds an item at the next top-level function. Keep the
    // inter-item suffix and next function's comments byte-identical.
    let close=original_span.rfind("\n}\n")
        .ok_or("workspace_replace_function: cannot safely identify the current function closing brace; use hash-checked range edit instead")?;
    let suffix=&original_span[close+3..];
    let updated=original.replacen(original_span,
        &format!("{}\n{}",new_text.trim_end(),suffix),1);
    if updated==original {
        return Err("workspace_replace_function: replacement does not modify source".into());
    }
    let mut result=checked_write(ctx,path,&updated,run_id,true,Some(&expected)).await?;
    if result["preflightSyntaxRejected"]==true || result["preflightScopeRejected"]==true {
        return Ok(result);
    }
    if result["written"]!=true {
        return Err(format!("workspace_replace_function: mutation declined: {result}"));
    }
    result["replacedFunction"]=json!(function);
    result["writtenSha256"]=json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    if let Some(parse)=rsi_rust_parse_receipt(ctx,path).await {
        result["syntaxValid"]=parse["valid"].clone();
        result["syntaxCheck"]=parse;
    }
    Ok(result)
}

/// Read-only Rust parser immediately after a model-authored source
/// mutation. Same-turn feedback is not an independent quality verdict.
pub(super) async fn rsi_rust_parse_receipt(ctx:&ToolContext<'_>,path:&Path)->Option<Value> {
    if path.extension().and_then(|ext|ext.to_str())!=Some("rs")
        || ctx.store.get("session",ctx.session_id)?["rsiRun"]!=true {
        return None;
    }
    let mut command=tokio::process::Command::new("rustfmt");
    command.args(["--edition","2021","--emit","stdout",
        "--config","skip_children=true"])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let output=tokio::time::timeout(Duration::from_secs(12),command.output())
        .await.ok()?.ok()?;
    if output.status.success() {
        return Some(json!({"valid":true,"tool":"rustfmt_syntax_read_only"}));
    }
    let diagnostic=String::from_utf8_lossy(&output.stderr).to_string();
    if !diagnostic.contains("error") {return None;}
    let line=diagnostic.lines().find_map(|row|{
        let row=row.trim();
        if !row.starts_with("-->"){return None;}
        let (prefix,col)=row.rsplit_once(':')?;
        col.trim().parse::<u64>().ok()?;
        let (_,line)=prefix.rsplit_once(':')?;
        line.trim().parse::<u64>().ok()
    });
    Some(json!({"valid":false,"tool":"rustfmt_syntax_read_only",
        "line":line,
        "diagnostic":diagnostic.chars().take(1800).collect::<String>()}))
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
    if old_text == new_text {
        return Err("workspace_replace: oldText와 newText가 동일하여 소스 변경이 없습니다.".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let occurrences = text.matches(old_text).count();
    if occurrences != 1 {
        return Err(format!(
            "workspace_replace 실행 실패: oldText 출현 {occurrences}회 (정확히 1회 필요), 파일 {}. workspace_read_range로 실제 소스와 들여쓰기·줄바꿈을 다시 확인한 뒤 교정해 재호출하세요.",
            path.display()
        ));
    }
    let updated = text.replacen(old_text, new_text, 1);
    let mut result = checked_write(ctx, path, &updated, run_id, true, expected_sha256).await?;
    if result["preflightSyntaxRejected"]==true || result["preflightScopeRejected"]==true {return Ok(result);}
    // checked_write can return an error *inside* an Ok(Value) for a stale
    // revision or a preflight refusal. Do not label that as a real edit.
    if result["written"] != true {
        return Err(format!(
            "workspace_replace 실행 거부 (수정 0회): {}. 최신 파일을 읽고 인자를 고쳐 다시 시도하세요.",
            result.to_string().chars().take(1400).collect::<String>()
        ));
    }
    result["replaced"] = json!(true);
    result["replacedChars"] = json!(old_text.chars().count());
    result["writtenSha256"] = json!(format!("{:x}",Sha256::digest(updated.as_bytes())));
    if let Some(parse)=rsi_rust_parse_receipt(ctx,path).await {
        result["syntaxValid"]=parse["valid"].clone();
        result["syntaxCheck"]=parse;
    }
    Ok(result)
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

    // The self-modifying intelligence may edit every one of its own code
    // layers. The independent comparator and approval boundary remain fixed.
    // Enforce this at the actual write entry point, for all edit tool names.
    if ctx.store.get("session",ctx.session_id)
        .is_some_and(|session|session["rsiRun"]==true
            && session["rsiSystemEvolution"]==true){
        let root=canonical_target(ctx.workspace).map_err(|e|e.to_string())?;
        let target=canonical_target(path).map_err(|e|e.to_string())?;
        let relative=target.strip_prefix(&root)
            .map_err(|_|"후손의 소스 편집 경로가 실험 외부입니다.".to_string())?;
        let relative=relative.to_str().ok_or("UTF-8 소스 경로 필요")?;
        if !crate::engine::rsi_system::intelligence_editable_path(relative){
            return Err("전체 지능 자기개선은 독립 평가기·승격기가 아닌 자신의 시스템 소스만 수정할 수 있습니다.".into());
        }
    }
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
    if let Err(error)=rsi_rust_scope_preview(ctx,path,content).await {
        if error.starts_with("rsi_scope_rejected:") {
            // Keep the same-turn native editor alive and preserve its read SHA.
            return Ok(json!({"path":path.to_string_lossy(),
                "written":false,"sourceUnchanged":true,
                "preflightScopeRejected":true,"error":error}));
        }
        return Err(error);
    }
    if let Err(error)=rsi_rust_parse_preview(ctx,path,content).await {
        if error.starts_with("rsi_rust_syntax_rejected:") {
            // A recoverable model proposal failure, not a changed file.
            // Return a typed receipt so same-turn editing can continue without
            // launching expensive generic model adaptation.
            return Ok(json!({
                "path":path.to_string_lossy(),"written":false,
                "preflightSyntaxRejected":true,"sourceUnchanged":true,
                "syntaxValid":false,
                "syntaxCheck":{"valid":false,"tool":"rustfmt_syntax_preflight",
                    "diagnostic":error.chars().take(1800).collect::<String>()},
                "error":error,
            }));
        }
        return Err(error);
    }
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

