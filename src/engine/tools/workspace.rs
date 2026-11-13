//! Validated workspace resolution and code-index read operations.
//! Active Rust execution paths imported by tools.rs.
use super::*;

pub(super) fn required_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing argument: {key}"))
}

pub(super) fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|value| value.as_str())
}

pub const WORKSPACE_ESCAPE: &str = "Path escapes the workspace";

/// Reject `..` traversal and symlink escapes for workspace-scoped tools.
pub(in super::super) fn resolve_workspace(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
    let base = canonical_target(workspace).map_err(|error| error.to_string())?;
    let candidate = Path::new(relative);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WORKSPACE_ESCAPE.into());
    }
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };
    let resolved = canonical_target(&joined).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&base) {
        return Err(WORKSPACE_ESCAPE.into());
    }
    Ok(resolved)
}

/// Resolve existing ancestors even when the final file has not been created.
pub(super) fn canonical_target(path: &Path) -> std::io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(error);
            }
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(error);
            };
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            Ok(canonical_target(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn list_dir(path: &Path) -> Result<Value, String> {
    let mut entries = Vec::new();
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    while let Ok(Some(entry)) = reader.next_entry().await {
        let metadata = entry.metadata().await.ok();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "type": if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) { "dir" } else { "file" },
            "size": metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        }));
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Ok(json!({ "path": path.to_string_lossy(), "entries": entries }))
}

pub(super) async fn read_text(path: &Path) -> Result<Value, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err("Target is not a regular file".into());
    }
    if metadata.len() > 1_500_000 {
        return Err("File is larger than 1.5 MB".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy(), "content": text }))
}

pub(super) fn preflight_policy(ctx: &ToolContext<'_>, path: &Path, run_id: &str) -> Option<Value> {
    let run = ctx.store.get("execution", run_id)?;
    let policy = &run["preflightPolicy"];
    (crate::engine::policy::is_preflight_kernel(&policy["parameters"]["kernel"])
        && policy["targetPath"].as_str().is_none_or(|target| {
            let target = Path::new(target);
            path == target || canonical_target(path).is_ok_and(|path| path == target)
        }))
        .then(|| policy.clone())
}

pub(super) async fn observed_read(ctx: &ToolContext<'_>, path: &Path, run_id: &str) -> Result<Value, String> {
    if preflight_policy(ctx, path, run_id).is_none() {
        return read_text(path).await;
    }
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut result = read_text(&canonical).await?;
    result["path"] = json!(path.to_string_lossy());
    let text = result["content"].as_str().unwrap_or_default();
    result["fileObservation"] = json!({
        "path": canonical.to_string_lossy(),
        "sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
    });
    Ok(result)
}

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
        if session["rsiRun"]==true
            && crate::engine::rsi::hidden_rsi_oracle_path(path) {
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

/// Native small-model edit: the verified full-file hash binds a concise
/// line replacement to exactly the source observed by workspace_read_range.
/// checked_write repeats the hash comparison under its file mutation lock.
#[derive(Debug,PartialEq,Eq)]
