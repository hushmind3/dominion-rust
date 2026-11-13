//! RSI guardrails, verified edit routing and bounded intent.
//! Extracted without changing the execution algorithms or stored evidence schema.
use super::*;

/// Task contracts bound intent to 4096 characters. Keep the full model prompt
/// intact but derive a bounded, content-unique value for the contract.
pub(super) fn bounded_contract_intent(input: &str) -> String {
    if input.chars().count() <= 4096 {
        return input.to_string();
    }
    format!(
        "{} [{}]",
        input.chars().take(4000).collect::<String>(),
        crate::engine::memory::stable_id(&[input])
    )
}

/// Repository maps, quoted source and prior evidence are context, not tasks.
pub(super) fn task_input<'a>(session: &'a Value, input: &'a str) -> &'a str {
    if session["rsiRun"]==true {
        session["rsiObjective"].as_str().filter(|text|!text.trim().is_empty()).unwrap_or(input)
    } else {input}
}

pub(super) fn rsi_output_allowance(spent: u64, input: u64) -> Result<u64,String> {
    let remaining=MAX_TOKENS.saturating_sub(spent).saturating_sub(input);
    if remaining<256 {return Err(format!("토큰 예산({MAX_TOKENS})이 부족해 다음 호출 전에 중단했습니다."));}
    Ok(remaining.min(8192))
}

/// When RSI has spent most of the turn on exploration, stop offering read
/// tools. The model must decide: make the grounded change, or finish without
/// one. This is a decision checkpoint, not a forced write.
pub(super) fn rsi_should_checkpoint(tool_calls: u64, has_write: bool) -> bool {
    !has_write && tool_calls >= 6
}

/// A concrete source-edit goal cannot spend every model round re-reading its
/// evidence. An edit-only stage is reached only after real source material was
/// delivered in the prompt or returned by a successful read tool. Unfocused
/// autonomous research retains its more permissive decision checkpoint.
pub(super) fn rsi_checkpoint_stage_with_method(
    tool_calls:u64, read_calls:u64, has_write:bool,
    focus_available:bool, source_in_prompt:bool, policy:&Value,
)->Option<bool>{
    if has_write {return None;}
    let threshold=policy["checkpointAfter"].as_u64().unwrap_or(3).clamp(2,5);
    if focus_available && (source_in_prompt || read_calls>0)
        && tool_calls>=threshold {return Some(true);}
    rsi_should_checkpoint(tool_calls,has_write).then_some(false)
}
pub(super) fn rsi_checkpoint_stage(
    tool_calls: u64, read_calls: u64, has_write: bool,
    explicit_focus: bool, source_in_prompt: bool,
) -> Option<bool> {
    rsi_checkpoint_stage_with_method(tool_calls,read_calls,has_write,
        explicit_focus,source_in_prompt,&Value::Null)
}

/// Stop broad discovery but allow exact missing ranges to be read. Bounded
/// excerpts are intentionally incomplete, so banning all reads would prevent edits.
pub(super) fn rsi_checkpoint_tool(name: &str, edit_only: bool) -> bool {
    if edit_only {
        return matches!(name, "workspace_write" | "workspace_replace" | "workspace_replace_range");
    }
    matches!(
        name,
        "workspace_read_range" | "workspace_write" | "workspace_replace" | "workspace_replace_range" | "rsi_method_propose" | "algorithm_propose"
    )
}

/// Learned search/contrast stages must not deadlock a grounded RSI worker.
/// Real anonymous models sometimes choose an edit before the controller's
/// search-only substage advances. If a CURRENT source read succeeded and no
/// re-ground is required, offer both existing-file edits as bounded fallback.
/// Hash/oldText preconditions and detached worktree verification still apply.
pub(super) fn rsi_offer_grounded_edit_fallback(
    menu:&mut Vec<Value>, catalog:&[Value],
    focused:bool, has_current_read:bool, must_reground:bool, verified_write:bool,
)->bool {
    if !focused || !has_current_read || must_reground || verified_write {
        return false;
    }
    let mut changed=false;
    for definition in catalog {
        let name=tool_name(definition);
        if matches!(name,"workspace_replace_range"|"workspace_replace")
            && !rsi_tool_advertised(name,menu) {
            menu.push(definition.clone());
            changed=true;
        }
    }
    changed
}

/// Execute the verifier-selected compiler repair as an actual tool menu
/// transition. Every retry re-observes the failed source in this run before
/// an edit is offered. A failed edit invalidates that observation again.
pub(super) fn rsi_compiler_repair_route(
    session:&Value, operations:&[Value], verified_write:bool
)->Option<crate::engine::rsi_edit_program::Route>{
    let target=session["rsiRepairPath"].as_str().filter(|p|!p.is_empty())?;
    if verified_write {return None;}
    let mut grounded=false;
    for op in operations {
        if op["tool"]=="workspace_read_range"
            && op["status"]=="completed"
            && op["arguments"]["path"].as_str()==Some(target)
            && op["result"]["sourceSha256"].as_str().is_some_and(|digest|
                digest.len()==64 && digest.bytes().all(|byte|byte.is_ascii_hexdigit())) {
            grounded=true;
        }
        if matches!(op["tool"].as_str(),
            Some("workspace_replace_range"|"workspace_replace"|"workspace_write"))
            && (op["status"]!="completed" || op["result"]["written"]!=true
                || op["toolReceipt"]["verified"]!=true) {
            grounded=false;
        }
    }
    Some(crate::engine::rsi_edit_program::Route{
        phase:if grounded {"edit"} else {"reground"}
    })
}

/// Query-based tool discovery must not shrink the RSI toolset: a generation
/// needs read, search and replace together to produce a grounded patch.
pub(super) fn tool_discovery_applies(is_room: bool, rsi_run: bool) -> bool {
    !is_room && !rsi_run
}

/// Small OpenAI-compatible models sometimes send a structured tool action as
/// plain assistant text even after advertising native functions. Only recover
/// such replies inside the focused RSI edit stage, where the available menu
/// has already been reduced to source-editing tools. Ordinary native model
/// answers remain untouched.
pub(super) fn rsi_textual_edit_fallback(rsi_run: bool, explicit_focus: bool, edit_stage: bool) -> bool {
    rsi_run && explicit_focus && edit_stage
}

/// OpenAI-compatible models sometimes return a tool which was advertised in
/// an earlier round rather than this round. The RSI decision checkpoint must
/// be enforced by the executor, not solely by a provider's tool_choice hint.
pub(super) fn rsi_tool_advertised(name: &str, round_tools: &[Value]) -> bool {
    round_tools.iter().any(|definition| tool_name(definition) == name)
}

/// Some compatible model gateways leak a channel delimiter INTO the name of
/// an otherwise valid native tool. Recover only an EXACT advertised tool with
/// a known trailing protocol delimiter. Never introduce a tool that the RSI
/// stage did not advertise: discovery restrictions remain enforceable.
pub(super) fn rsi_canonical_tool_name<'a>(name:&'a str, round_tools:&[Value])->Option<&'a str>{
    if rsi_tool_advertised(name,round_tools) {return Some(name);}
    for suffix in ["<|channel|>commentary","<|channel|>analysis"] {
        if let Some(base)=name.strip_suffix(suffix) {
            if rsi_tool_advertised(base,round_tools) {return Some(base);}
        }
    }
    None
}

/// "completed" is not synonymous with a source change: an unchanged
/// replacement can legitimately return success. Only a verified readback of
/// written bytes may advance the RSI source-edit checkpoint.
pub(super) fn rsi_verified_write_operation(operation: &Value) -> bool {
    operation["status"] == "completed"
        && matches!(operation["tool"].as_str(),
            Some("workspace_write" | "workspace_replace" | "workspace_replace_range"))
        && operation["result"]["written"] == true
        && operation["toolReceipt"]["verified"] == true
}

/// A focused single-target RSI run should hand a real, hash-receipted edit
/// straight to the independent worktree verifier. The verifier, not the model's
/// closing prose, decides whether the resulting patch compiles or improves.
/// This avoids paying another full model context after source modification.
pub(super) fn rsi_patch_ready_for_verification(
    session: &Value, tool: &str, args: &Value, result: &Value, ok: bool,
) -> bool {
    let compiler_repair_target = session["rsiRepairPath"].as_str()
        .filter(|path|args["path"].as_str()==Some(*path));
    ok && session["rsiRun"] == true
        && (session["rsiExplicitFocus"] == true || session["rsiAutoFocus"] == true
            || compiler_repair_target.is_some())
        && (session["rsiCampaignId"].is_string() || compiler_repair_target.is_some())
        && (compiler_repair_target.is_some() || session["rsiFocusFiles"].as_array().is_some_and(|files|
            files.iter().any(|file| file.as_str().is_some() && file.as_str() == args["path"].as_str())))
        && matches!(tool, "workspace_replace_range" | "workspace_replace")
        && result["written"] == true
        && result["postSha256"].as_str().is_some_and(|sha|
            sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A small model may omit the hash or encode line numbers as strings. Recover
/// only information already observed by a successful range read in THIS run,
/// for the same file and covering the requested edit. Never infer newText,
/// overwrite a valid supplied digest, or bypass the final file hash check.
/// A malformed "hash" from a small model is not a usable source-version proof:
/// replace only that malformed value with the exact matching observed digest.
pub(super) fn rsi_ground_range_edit_args(mut args: Value, completed: &[Value]) -> Value {
    for field in ["startLine", "endLine"] {
        if let Some(raw) = args[field].as_str().map(str::to_string) {
            if !raw.is_empty() && raw.len() <= 12 && raw.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(number) = raw.parse::<u64>() {
                    args[field] = json!(number);
                }
            }
        }
    }
    if args["expectedSha256"].as_str().is_some_and(|s|
        s.len()==64 && s.bytes().all(|byte|byte.is_ascii_hexdigit())) {
        return args;
    }
    let Some(path) = args["path"].as_str() else {return args;};
    let Some(start) = args["startLine"].as_u64() else {return args;};
    let Some(end) = args["endLine"].as_u64() else {return args;};
    if start == 0 || end < start || args["newText"].as_str().is_none() {
        return args;
    }
    let matching = completed.iter()
        .filter(|op| op["status"] == "completed"
            && op["tool"] == "workspace_read_range"
            && op["arguments"]["path"].as_str() == Some(path)
            && op["result"]["startLine"].as_u64().is_some_and(|from| from <= start)
            && op["result"]["endLine"].as_u64().is_some_and(|to| end <= to)
            && op["result"]["sourceSha256"].as_str().is_some_and(|sha|
                sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit())))
        .max_by_key(|op| op["opSeq"].as_u64().unwrap_or(0));
    if let Some(read) = matching {
        args["expectedSha256"] = read["result"]["sourceSha256"].clone();
    }
    args
}

