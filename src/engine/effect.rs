//! Effect receipts for tool calls.
//!
//! A model answer, a tool call, and an external effect are different facts.
//! This module records the intent of a mutating tool *before* it runs, then
//! seals what was observed afterwards. It reuses the existing
//! `execution_operation` record instead of introducing a second execution log.
//!
//! The only strong guarantee here is the one we can actually verify locally:
//! a workspace/file write is re-read and hashed. Everything else is recorded
//! as `applied` (tool claimed success) or `effect_unknown`, never as verified.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const WRITE_VERIFY_LIMIT: u64 = 8 * 1024 * 1024;

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hash_text(text: &str) -> String {
    hash(text.as_bytes())
}

fn stable_id(parts: &[&str]) -> String {
    crate::engine::memory::stable_id(parts)
}

fn canonical_arguments(value: &Value) -> String {
    // serde_json maps are ordered deterministically in this crate (no
    // preserve_order feature), so equal arguments produce equal digests.
    value.to_string()
}

/// The durable identity of one logical effect. It deliberately excludes the
/// execution id: after a restart or timeout the same session + tool + inputs
/// must resolve to the same effect, so a retry can find the earlier attempt.
pub fn effect_id(session_id: &str, tool: &str, arguments: &Value) -> Option<String> {
    is_effectful(tool, arguments).then(|| {
        format!(
            "effect-{}",
            stable_id(&[session_id, tool, &canonical_arguments(arguments)])
        )
    })
}

pub fn request_digest(tool: &str, arguments: &Value) -> String {
    let material = json!({"tool": tool, "arguments": arguments});
    format!("sha256:{}", hash_text(&canonical_arguments(&material)))
}

fn normalize_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// A model-neutral resource identity. Different tools that operate on the same
/// underlying object can still be compared through this key.
pub fn resource_key(tool: &str, arguments: &Value) -> Option<String> {
    let path = arguments["path"].as_str();
    match tool {
        "workspace_read" | "workspace_write" | "workspace_replace" | "workspace_list" => {
            path.map(|path| format!("workspace:{}", normalize_path(path)))
        }
        "file_read" | "file_write" | "file_list" => {
            path.map(|path| format!("file:{}", normalize_path(path)))
        }
        "github_api" => path.map(|path| format!("github:{}", normalize_path(path))),
        "http_fetch" => arguments["url"].as_str().map(|url| format!("http:{url}")),
        "shell_exec" | "terminal_exec" => Some(format!(
            "process:{}",
            arguments["cwd"]
                .as_str()
                .map(normalize_path)
                .unwrap_or_else(|| "default".into())
        )),
        tool if tool.starts_with("mcp__") => {
            let target = ["key", "id", "path", "resource"]
                .iter()
                .find_map(|key| arguments[*key].as_str())
                .unwrap_or("default");
            // Observations and mutations of one server target must share a
            // key, otherwise a read can never resolve a pending write.
            let server = tool.rsplit_once("__").map(|(server, _)| server).unwrap_or(tool);
            Some(format!("{server}:{}", normalize_path(target)))
        }
        _ => None,
    }
}

/// Reads are observations; writes, shells and non-GET API calls may change the
/// world. A read can still have side effects (locks, billing reservations), so
/// callers must not treat this classification as a safety proof.
pub fn is_effectful(tool: &str, arguments: &Value) -> bool {
    if matches!(tool, "shell_exec" | "terminal_exec") {
        return true;
    }
    if tool == "github_api" {
        return arguments["method"]
            .as_str()
            .map(|method| !method.eq_ignore_ascii_case("GET"))
            .unwrap_or(true);
    }
    if tool == "http_fetch" {
        return false;
    }
    [
        "write", "update", "delete", "remove", "patch", "put", "post", "create", "move",
        "rename", "edit", "commit", "push", "archive", "insert", "replace", "exec",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

fn is_read(tool: &str, arguments: &Value) -> bool {
    if tool == "http_fetch" {
        return true;
    }
    if tool == "github_api" {
        return !is_effectful(tool, arguments);
    }
    if tool.starts_with("mcp__") {
        let lower = tool.to_ascii_lowercase();
        return ["read", "get", "list", "search", "inspect"]
            .iter()
            .any(|word| lower.contains(word));
    }
    [
        "read", "list", "get", "status", "head", "search", "inspect", "show", "diff",
        "fetch", "verify", "check", "query", "describe",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

/// Content-addressed file writes are safe to repeat: the effect id already
/// includes the exact bytes. Shell commands are recorded but not blocked,
/// because their effects cannot be confirmed generically and a false block
/// would permanently strand a common read-only command.
fn replay_is_safe(tool: &str) -> bool {
    matches!(tool, "workspace_write" | "file_write" | "shell_exec" | "terminal_exec")
}

/// Only tools whose target can be observed again are allowed to block a retry.
/// File writes are idempotent; shell effects are opaque; API and MCP mutations
/// expose a resource that an observation can resolve.
fn gated(tool: &str, arguments: &Value) -> bool {
    is_effectful(tool, arguments)
        && (tool == "github_api"
            || tool.starts_with("mcp__")
            || tool.contains("http_request"))
}

fn operation_version(operation: &Value) -> Option<(String, String)> {
    let version = operation["observedVersion"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            operation["fileObservation"]["sha256"]
                .as_str()
                .map(|sha| format!("sha256:{sha}"))
        })
        .or_else(|| {
            operation["result"]["fileObservation"]["sha256"]
                .as_str()
                .map(|sha| format!("sha256:{sha}"))
        })?;
    let id = operation["id"].as_str().unwrap_or("").to_string();
    Some((version, id))
}

fn latest_version(operations: &[Value], resource: &str) -> Option<(String, String, String)> {
    operations.iter().rev().find_map(|operation| {
        if operation["resourceKey"].as_str() != Some(resource) {
            return None;
        }
        let (version, id) = operation_version(operation)?;
        let observed_at = operation["finishedAt"]
            .as_str()
            .or_else(|| operation["startedAt"].as_str())
            .unwrap_or("")
            .to_string();
        Some((version, id, observed_at))
    })
}

/// Builds the pre-execution contract from the session-scoped operation history.
pub fn prepare(
    session_id: &str,
    tool: &str,
    arguments: &Value,
    operations: &[Value],
) -> Value {
    let resource = resource_key(tool, arguments);
    let effect_id = effect_id(session_id, tool, arguments);
    let before = resource
        .as_deref()
        .and_then(|resource| latest_version(operations, resource));
    let before_version = before.as_ref().map(|(version, operation_id, observed_at)| {
        json!({
            "version": version,
            "sourceOperationId": operation_id,
            "observedAt": observed_at,
        })
    });
    json!({
        "effectId": effect_id,
        "resourceKey": resource,
        "requestDigest": request_digest(tool, arguments),
        "beforeVersion": before_version,
        "effectState": "prepared",
        "tool": tool,
        "gated": gated(tool, arguments),
    })
}

/// Returns the earlier attempt that makes an immediate retry unsafe. Only
/// outcomes that were never observed, or attempts still running, block the
/// call; a completed write is content-addressed and can be replayed.
pub fn unresolved(effect: &Value, operations: &[Value]) -> Option<Value> {
    let effect_id = effect["effectId"].as_str()?;
    if effect["gated"].as_bool() != Some(true)
        || replay_is_safe(effect["tool"].as_str().unwrap_or_default())
    {
        return None;
    }
    operations.iter().rev().find_map(|operation| {
        if operation["effectId"].as_str() != Some(effect_id) {
            return None;
        }
        let state = operation["effectState"].as_str().unwrap_or("");
        matches!(
            state,
            "effect_unknown" | "needs_review" | "started" | "prepared"
        )
        .then(|| operation.clone())
    })
}

fn failure_layer(ok: bool, denied: bool, result: &Value, effectful: bool) -> Option<&'static str> {
    if denied {
        return Some("authority");
    }
    if ok {
        return None;
    }
    let error = result
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if result["outcomeUnknown"].as_bool() == Some(true) {
        return Some("execution");
    }
    if error.contains("timed out")
        || error.contains("timeout")
        || error.contains("interrupted")
        || error.contains("중단")
    {
        return Some("execution");
    }
    if error.contains("permission")
        || error.contains("denied")
        || error.contains("허가")
        || error.contains("거부")
    {
        return Some("authority");
    }
    if error.contains("missing argument")
        || error.contains("invalid")
        || error.contains("schema")
    {
        return Some("tool_arguments");
    }
    if error.contains("conflict")
        || error.contains("stale")
        || error.contains("sha mismatch")
        || error.contains("preflight")
        || error.contains("not found")
        || error.contains("no such file")
        || error.contains("does not exist")
    {
        return Some("state");
    }
    if effectful {
        Some("execution")
    } else {
        Some("tool")
    }
}

async fn read_back(workspace: &Path, tool: &str, arguments: &Value) -> Option<(String, u64)> {
    let path = arguments["path"].as_str()?;
    let path: PathBuf = match tool {
        "workspace_write" | "workspace_replace" | "workspace_read" => {
            crate::engine::tools::resolve_workspace(workspace, path).ok()?
        }
        "file_write" | "file_read" => PathBuf::from(path),
        _ => return None,
    };
    let metadata = tokio::fs::metadata(&path).await.ok()?;
    let len = metadata.len();
    if len > WRITE_VERIFY_LIMIT {
        return None;
    }
    let bytes = tokio::fs::read(&path).await.ok()?;
    Some((format!("sha256:{}", hash(&bytes)), len))
}

fn expected_version(tool: &str, arguments: &Value) -> Option<String> {
    if matches!(tool, "workspace_write" | "file_write") {
        return arguments["content"].as_str().map(|content| {
            format!("sha256:{}", hash_text(content))
        });
    }
    None
}

/// Seals the call with the evidence actually available. This is the only place
/// that decides `verified`; a model message never does.
pub async fn seal(
    workspace: &Path,
    tool: &str,
    arguments: &Value,
    result: &Value,
    ok: bool,
    denied: bool,
    blocked: bool,
    effect: &Value,
) -> Value {
    let effectful = is_effectful(tool, arguments);
    let mut observed_version = None;
    let mut verified = false;
    let mut verifier = "none";
    let mut cost_bytes = 0u64;
    let mut effect_state = if blocked {
        "needs_review"
    } else if denied {
        "denied"
    } else if !ok {
        if result["outcomeUnknown"].as_bool() == Some(true) {
            "effect_unknown"
        } else {
            "failed"
        }
    } else if effectful {
        "applied"
    } else {
        "observed"
    };

    if ok && matches!(tool, "workspace_write" | "workspace_replace" | "file_write") {
        let expected=if tool=="workspace_replace" {
            result["postSha256"].as_str().filter(|sha|
                sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
                .map(|sha|format!("sha256:{sha}"))
        } else {
            expected_version(tool, arguments)
        };
        if let Some(expected) = expected {
            match read_back(workspace, tool, arguments).await {
                Some((actual, len)) => {
                    cost_bytes = len;
                    observed_version = Some(actual.clone());
                    verifier = "sha256_readback";
                    verified = actual == expected;
                    effect_state = if verified { "verified" } else { "effect_unknown" };
                }
                None => {
                    verifier = "sha256_readback_unavailable";
                    effect_state = "applied";
                }
            }
        }
    } else if ok && is_read(tool, arguments) {
        if let Some(content) = result["content"].as_str() {
            observed_version = Some(format!("sha256:{}", hash_text(content)));
            verifier = "sha256_content";
            verified = true;
            effect_state = "observed";
        } else if let Some(observation) = result["fileObservation"]["sha256"].as_str() {
            observed_version = Some(format!("sha256:{observation}"));
            verifier = "tool_observation";
            verified = true;
            effect_state = "observed";
        }
    }

    let mut layer = failure_layer(ok, denied, result, effectful);
    if blocked {
        // The call was stopped before execution because a prior effect was
        // unresolved; the defect is the missing state confirmation.
        layer = Some("state");
    } else if ok && effect_state == "effect_unknown" {
        layer = Some("verification");
    }
    let receipt = json!({
        "effectId": effect["effectId"],
        "tool": tool,
        "requestDigest": effect["requestDigest"],
        "resourceKey": effect["resourceKey"],
        "status": effect_state,
        "verified": verified,
        "verifier": {"kind": verifier, "revision": "effect-v1"},
        "beforeVersion": effect["beforeVersion"],
        "observedVersion": observed_version,
        "observedAt": chrono::Utc::now().to_rfc3339(),
        "cost": {"readBytes": cost_bytes},
        "failureLayer": layer,
        "outcomeUnknown": result["outcomeUnknown"].as_bool() == Some(true),
    });
    json!({
        "effectState": effect_state,
        "observedVersion": observed_version,
        "toolReceipt": receipt,
    })
}

/// A successful observation of the same resource resolves a pending effect to
/// `inspected`. It does not prove the effect was applied or absent; it only
/// removes the blind-retry block and leaves the evidence trail intact.
pub fn mark_inspected(operations: &[Value], resource: &str, inspector_id: &str) -> Vec<Value> {
    operations
        .iter()
        .filter(|operation| {
            operation["id"].as_str() != Some(inspector_id)
                && operation["resourceKey"].as_str() == Some(resource)
                && matches!(
                    operation["effectState"].as_str().unwrap_or(""),
                    "effect_unknown" | "needs_review" | "started" | "prepared"
                )
        })
        .map(|operation| {
            let mut updated = operation.clone();
            updated["effectState"] = json!("inspected");
            if !updated["toolReceipt"].is_object() {
                updated["toolReceipt"] = json!({});
            }
            updated["toolReceipt"]["status"] = json!("inspected");
            updated["toolReceipt"]["inspectedBy"] = json!(inspector_id);
            updated["toolReceipt"]["inspectedAt"] = json!(chrono::Utc::now().to_rfc3339());
            updated
        })
        .collect()
}

/// Aggregate evidence attached to a finalized execution. `unverifiedEffects`
/// is the signal the learning gate uses: a task can succeed while one of its
/// effects is still unknown, and that run must not teach a positive rule.
pub fn summary(operations: &[Value]) -> Value {
    let mut observed = 0u64;
    let mut verified = 0u64;
    let mut applied = 0u64;
    let mut inspected = 0u64;
    let mut unknown = 0u64;
    let mut failed = 0u64;
    let mut denied = 0u64;
    let mut layers = std::collections::BTreeMap::<String, u64>::new();
    for operation in operations {
        match operation["effectState"].as_str().unwrap_or("") {
            "observed" => observed += 1,
            "verified" => verified += 1,
            "applied" => {
                // The tool claimed success, but no independent observation
                // confirmed the external change.
                applied += 1;
                unknown += 1;
            }
            "inspected" => {
                inspected += 1;
                unknown += 1;
            }
            "effect_unknown" | "needs_review" => unknown += 1,
            "started" | "prepared" => unknown += 1,
            "failed" => failed += 1,
            "denied" => denied += 1,
            _ => {}
        }
        if let Some(layer) = operation["toolReceipt"]["failureLayer"].as_str() {
            *layers.entry(layer.to_string()).or_default() += 1;
        }
    }
    json!({
        "observed": observed,
        "verifiedEffects": verified,
        "appliedEffects": applied,
        "inspectedEffects": inspected,
        "unverifiedEffects": unknown,
        "failedEffects": failed,
        "deniedEffects": denied,
        "failureLayers": layers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, tool: &str, args: Value, state: &str, effect: &Value) -> Value {
        json!({
            "id": id,
            "tool": tool,
            "arguments": args,
            "effectId": effect["effectId"],
            "resourceKey": effect["resourceKey"],
            "effectState": state,
            "finishedAt": "2026-11-07T00:00:00Z",
        })
    }

    #[test]
    fn effect_identity_survives_a_new_run_and_rejects_new_content() {
        let args = json!({"path":"src/lib.rs","content":"hello"});
        let first = prepare("session-1", "workspace_write", &args, &[]);
        let retry = prepare("session-1", "workspace_write", &args, &[]);
        assert_eq!(first["effectId"], retry["effectId"]);
        let changed = prepare(
            "session-1",
            "workspace_write",
            &json!({"path":"src/lib.rs","content":"hello!"}),
            &[],
        );
        assert_ne!(first["effectId"], changed["effectId"]);
        let other_path = prepare(
            "session-1",
            "workspace_write",
            &json!({"path":"src/other.rs","content":"hello"}),
            &[],
        );
        assert_ne!(first["effectId"], other_path["effectId"]);
    }

    #[test]
    fn resources_are_normalized_across_tool_spellings() {
        assert_eq!(
            resource_key("workspace_write", &json!({"path":"./a/../b.txt"})).as_deref(),
            Some("workspace:a/../b.txt")
        );
        assert_eq!(
            resource_key("github_api", &json!({"path":"/repos/o/r"})).as_deref(),
            Some("github:repos/o/r")
        );
        assert_eq!(
            resource_key("mcp__db__write", &json!({"id":"row-1"})).as_deref(),
            Some("mcp__db:row-1")
        );
        assert_eq!(
            resource_key("mcp__db__read", &json!({"id":"row-1"})).as_deref(),
            Some("mcp__db:row-1")
        );
    }

    #[test]
    fn unknown_effect_blocks_a_non_idempotent_retry() {
        let args = json!({"method":"POST","path":"/charges","body":{"amount":10}});
        let effect = prepare("s", "github_api", &args, &[]);
        let prior = op("op-1", "github_api", args.clone(), "effect_unknown", &effect);
        assert!(unresolved(&effect, &[prior]).is_some());

        let write = json!({"path":"a.txt","content":"x"});
        let write_effect = prepare("s", "workspace_write", &write, &[]);
        let prior = op("op-2", "workspace_write", write, "effect_unknown", &write_effect);
        assert!(
            unresolved(&write_effect, &[prior]).is_none(),
            "identical file bytes are idempotent"
        );
    }

    #[tokio::test]
    async fn write_receipt_is_verified_by_independent_readback() {
        let dir = std::env::temp_dir().join(format!("dominion-effect-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let args = json!({"path":"note.txt","content":"verified bytes"});
        tokio::fs::write(dir.join("note.txt"), "verified bytes")
            .await
            .unwrap();
        let effect = prepare("s", "workspace_write", &args, &[]);
        let sealed = seal(
            &dir,
            "workspace_write",
            &args,
            &json!({"written":true}),
            true,
            false,
            false,
            &effect,
        )
        .await;
        assert_eq!(sealed["effectState"], "verified");
        assert_eq!(sealed["toolReceipt"]["verified"], true);
        assert_eq!(sealed["toolReceipt"]["verifier"]["kind"], "sha256_readback");
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn replacement_receipt_requires_independent_matching_postimage() {
        let dir=std::env::temp_dir().join(format!("dominion-replace-effect-{}",uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let args=json!({"path":"src/example.rs","oldText":"old","newText":"new"});
        tokio::fs::create_dir_all(dir.join("src")).await.unwrap();
        tokio::fs::write(dir.join("src/example.rs"),"new").await.unwrap();
        let effect=prepare("s","workspace_replace",&args,&[]);
        assert_eq!(effect["resourceKey"],"workspace:src/example.rs");
        let receipt=seal(&dir,"workspace_replace",&args,
            &json!({"written":true,"postSha256":hash_text("new")}),
            true,false,false,&effect).await;
        assert_eq!(receipt["toolReceipt"]["verified"],true,"{receipt}");
        let wrong=seal(&dir,"workspace_replace",&args,
            &json!({"written":true,"postSha256":hash_text("not written")}),
            true,false,false,&effect).await;
        assert_eq!(wrong["toolReceipt"]["verified"],false,"{wrong}");
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }


        let dir = std::env::temp_dir().join(format!("dominion-effect-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let args = json!({"path":"missing/note.txt","content":"x"});
        let effect = prepare("s", "workspace_write", &args, &[]);
        let sealed = seal(
            &dir,
            "workspace_write",
            &args,
            &json!({"written":true}),
            true,
            false,
            false,
            &effect,
        )
        .await;
        assert_eq!(sealed["effectState"], "applied");
        assert_eq!(sealed["toolReceipt"]["verified"], false);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[test]
    fn opaque_shell_effects_are_recorded_but_never_block_the_session() {
        let args = json!({"command":"ls"});
        let effect = prepare("s", "shell_exec", &args, &[]);
        assert_eq!(effect["gated"], false);
        let prior = op("op-1", "shell_exec", args, "effect_unknown", &effect);
        assert!(unresolved(&effect, &[prior]).is_none());
    }

    #[test]
    fn an_observation_resolves_a_pending_retry_block() {
        let args = json!({"method":"POST","path":"/charges","body":{"amount":10}});
        let effect = prepare("s", "github_api", &args, &[]);
        let prior = op("op-1", "github_api", args, "effect_unknown", &effect);
        assert!(unresolved(&effect, &[prior.clone()]).is_some());
        let resolved = mark_inspected(&[prior], "github:charges", "op-2");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0]["effectState"], "inspected");
        assert_eq!(resolved[0]["toolReceipt"]["inspectedBy"], "op-2");
        assert!(
            unresolved(&effect, &resolved).is_none(),
            "inspection removes the blind retry block"
        );
    }

    #[test]
    fn summary_counts_unknown_effects_for_the_learning_gate() {
        let summary = summary(&[
            json!({"effectState":"verified","toolReceipt":{"failureLayer":null}}),
            json!({"effectState":"effect_unknown","toolReceipt":{"failureLayer":"execution"}}),
            json!({"effectState":"applied","toolReceipt":{"failureLayer":null}}),
        ]);
        assert_eq!(summary["verifiedEffects"], 1);
        assert_eq!(summary["appliedEffects"], 1);
        assert_eq!(
            summary["unverifiedEffects"], 2,
            "tool-claimed success without independent verification is not evidence"
        );
        assert_eq!(summary["failureLayers"]["execution"], 1);
    }
}
