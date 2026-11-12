//! Executable, lineage-bearing source-improvement programs.
//! A program is synthesized from completed generations; the runtime advances
//! its stages from persisted tool receipts, not from a model's claimed progress.
use serde_json::Value;

const KINDS: &[&str] = &["discover", "ground", "contrast", "edit", "reground", "verify"];

pub fn baseline() -> Vec<String> {
    ["discover", "ground", "edit", "verify"].iter().map(|s| s.to_string()).collect()
}

/// Canonical program: optional discovery and counterexample search, mandatory
/// grounding, patch attempt and independent verification, optional re-grounding.
fn canonical(steps: &[String]) -> Vec<String> {
    let has = |name: &str| steps.iter().any(|step| step == name);
    let mut result = Vec::new();
    if has("discover") { result.push("discover".to_string()); }
    result.push("ground".to_string());
    if has("contrast") { result.push("contrast".to_string()); }
    result.push("edit".to_string());
    if has("reground") { result.push("reground".to_string()); }
    result.push("verify".to_string());
    result
}

pub fn normalize(raw: &Value) -> Vec<String> {
    let Some(array) = raw.as_array() else { return baseline(); };
    let values = array.iter().filter_map(Value::as_str).collect::<Vec<_>>();
    if values.is_empty() || values.iter().any(|v| !KINDS.contains(v))
        || !values.contains(&"edit") || !values.contains(&"verify") {
        return baseline();
    }
    canonical(&values.into_iter().map(str::to_string).collect::<Vec<_>>())
}

/// Rewrite the *procedure*, not its numerical search/retry parameters.
/// The new nodes alter which tools are executable at a particular stage.
/// History remains a proposal until paired verification establishes a gain.
pub fn mutate(parent: &[String], outcome: &Value) -> Vec<String> {
    let mut next = canonical(parent);
    let error = format!("{} {} {}",
        outcome["error"].as_str().unwrap_or(""),
        outcome["captureError"].as_str().unwrap_or(""),
        outcome["repairFailure"].as_str().unwrap_or("")
    ).to_lowercase();
    let failures = outcome["editEvidence"]["recentFailures"].as_array()
        .into_iter().flatten().filter_map(Value::as_str)
        .collect::<Vec<_>>().join(" ").to_lowercase();
    let verified = outcome["editEvidence"]["verifiedWrites"].as_u64().unwrap_or(0);
    let attempted = outcome["editEvidence"]["attempts"].as_u64().unwrap_or(0);
    let candidate = outcome["candidateId"].is_string();
    // Only a persisted stage receipt can identify where the previous
    // *program* stalled. Do not infer phase execution from a policy proposal.
    let last_executed = outcome["programStages"].as_array()
        .and_then(|stages|stages.last())
        .and_then(|stage|stage["phase"].as_str()).unwrap_or("");
    let neutral = candidate && outcome["accepted"] != true
        && outcome["evaluation"]["verdict"] != "improved";
    let add = |steps: &mut Vec<String>, name: &str| {
        if !steps.iter().any(|s|s==name) { steps.push(name.to_string()); }
    };
    if error.contains("token_budget") || error.contains("토큰 예산") {
        next.retain(|s|s!="discover" && s!="contrast");
    } else if failures.contains("argument") || failures.contains("json")
        || failures.contains("unchanged") || failures.contains("sha")
        || failures.contains("인수") {
        add(&mut next,"reground");
    } else if neutral {
        add(&mut next,"contrast");
    } else if last_executed=="contrast" && verified==0 && !candidate {
        next.retain(|s|s!="contrast");
    } else if last_executed=="discover" && verified==0 && !candidate {
        next.retain(|s|s!="discover");
    } else if error.contains("compile") || error.contains("syntax")
        || error.contains("build_or_typecheck") || (verified>0 && !candidate) {
        add(&mut next,"reground");
    } else if attempted>0 && verified==0 {
        // A read-only or failed-edit worker gets a short direct-grounding
        // procedure rather than re-running its unproductive broad search.
        next.retain(|s|s!="discover");
    } else {
        // Exploration must change an executable judgment step even when
        // previous generations provide only sparse failure metadata.
        if next.iter().any(|s|s=="contrast") {next.retain(|s|s!="contrast");}
        else {add(&mut next,"contrast");}
    }
    canonical(&next)
}

/// Recombine independently observed procedures without inheriting any
/// unverified quality claims. This can import a useful failure-recovery stage
/// while preserving a different candidate's counterexample-search stage.
pub fn recombine(parent: &[String], donor: &[String]) -> Vec<String> {
    let mut combined = canonical(parent);
    if donor.iter().any(|s|s=="reground") && !combined.iter().any(|s|s=="reground") {
        combined.push("reground".to_string());
    }
    if donor.iter().any(|s|s=="contrast") && !combined.iter().any(|s|s=="contrast") {
        combined.push("contrast".to_string());
    }
    canonical(&combined)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub phase: &'static str,
}
impl Route {
    pub fn edit_only(self) -> bool {self.phase == "edit"}
    pub fn allows(self, name: &str) -> bool {
        match self.phase {
            "discover" => matches!(name,"code_search"|"workspace_read_range"),
            "ground" => matches!(name,"workspace_read_range"|"code_search"),
            "contrast" => matches!(name,"code_search"|"workspace_read_range"),
            "reground" | "focus_ground" => name=="workspace_read_range",
            "edit" => matches!(name,"workspace_replace_range"|"workspace_replace"|"workspace_write"),
            _ => true,
        }
    }
}

/// Execute a stage from the actual ordered tool outcomes of this run.
/// When the model has not yet grounded a claim, no forced write is permitted.
/// After a failed source edit, re-observe the current hash before retrying.
pub fn route(policy: &Value, operations: &[Value], calls: u64,
    focused: bool, source_in_prompt: bool) -> Option<Route> {
    if !focused || !policy["program"].is_array() {return None;}
    let program=normalize(&policy["program"]);
    let has=|name:&str|program.iter().any(|s|s==name);
    let mut successful_read=false;
    let mut successful_search=false;
    let mut post_ground_search=false;
    let mut reground_needed=false;
    let mut verified_write=false;
    for operation in operations {
        let name=operation["tool"].as_str().unwrap_or("");
        let completed=operation["status"]=="completed";
        if completed && matches!(name,"workspace_read_range"|"workspace_read") {
            successful_read=true;
            reground_needed=false;
            post_ground_search=false;
        }
        if completed && name=="code_search" {
            successful_search=true;
            if successful_read || source_in_prompt {post_ground_search=true;}
        }
        if matches!(name,"workspace_replace_range"|"workspace_replace") {
            if completed && operation["result"]["written"]==true
                && operation["toolReceipt"]["verified"]==true {
                verified_write=true;
            } else {
                // A failed or unverified edit cannot be retried against a
                // potentially stale source. Re-observe the native file hash.
                reground_needed=true;
            }
        }
    }
    if verified_write {return None;}
    // A focused worker with a source excerpt already injected by the
    // coordinator needs the exact live hash, NOT another broad search loop.
    // Require one real range-read before exposing edit operators.
    if source_in_prompt && !successful_read && !reground_needed {
        return Some(Route{phase:"focus_ground"});
    }
    let grounded=source_in_prompt || successful_read;
    let checkpoint=policy["checkpointAfter"].as_u64().unwrap_or(3).clamp(2,5);
    if !grounded {
        if has("discover") && !successful_search && calls<checkpoint {
            return Some(Route{phase:"discover"});
        }
        return Some(Route{phase:"ground"});
    }
    if reground_needed {
        // Enforce recovery even for first-generation policies that do not
        // yet include the learned reground node; record the phase so the
        // next descendant can incorporate it into its stage program.
        return Some(Route{phase:"reground"});
    }
    if has("contrast") && !post_ground_search && calls < checkpoint+2 {
        return Some(Route{phase:"contrast"});
    }
    // The focused task already supplied the exact source region. Once a
    // current range/hash was observed, discovery is complete even if a tiny
    // model has used fewer than checkpoint tool calls.
    if (source_in_prompt && successful_read)
        || calls>=checkpoint || (has("contrast") && post_ground_search) {
        return Some(Route{phase:"edit"});
    }
    None // Early grounded edits stay possible before the checkpoint.
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn op(tool:&str,status:&str,result:Value)->Value {
        json!({"tool":tool,"status":status,"result":result})
    }
    #[test]
    fn observed_failures_create_new_executable_judgment_nodes() {
        let base=baseline();
        let bad=json!({"editEvidence":{"recentFailures":["workspace_replace_range: argument hash mismatch"]}});
        let improved=mutate(&base,&bad);
        assert!(improved.contains(&"reground".to_string()));
        assert!(improved.iter().position(|p|p=="reground") > improved.iter().position(|p|p=="edit"));
        let neutral=json!({"candidateId":"c","accepted":false,"evaluation":{"verdict":"unchanged"}});
        let recomposed=mutate(&improved,&neutral);
        assert!(recomposed.contains(&"contrast".to_string()));
        assert!(recomposed.contains(&"reground".to_string()));
        let exhausted=mutate(&recomposed,&json!({"error":"rsi_token_budget: exhausted"}));
        assert!(!exhausted.contains(&"discover".to_string()));
        assert!(!exhausted.contains(&"contrast".to_string()));
        assert!(exhausted.contains(&"reground".to_string()));
    }
    #[test]
    fn recovery_and_counterexample_stages_really_change_tool_execution() {
        let policy=json!({"program":["discover","ground","contrast","edit","reground","verify"],"checkpointAfter":2});
        let search=route(&policy,&[],0,true,false).unwrap();
        assert_eq!(search.phase,"discover");
        assert!(search.allows("code_search"));
        assert!(!search.allows("workspace_replace"));
        let grounded=vec![
            op("code_search","completed",json!({})),
            op("workspace_read_range","completed",json!({"sourceSha256":"abc"}))
        ];
        let contrast=route(&policy,&grounded,2,true,false).unwrap();
        assert_eq!(contrast.phase,"contrast");
        assert!(!contrast.allows("workspace_replace"));
        let mut compared=grounded.clone();
        compared.push(op("code_search","completed",json!({})));
        let editing=route(&policy,&compared,3,true,false).unwrap();
        assert_eq!(editing.phase,"edit");
        assert!(editing.allows("workspace_replace_range"));
        assert!(!editing.allows("code_search"));
        compared.push(op("workspace_replace_range","failed",json!({"error":"sha mismatch"})));
        let recovery=route(&policy,&compared,4,true,false).unwrap();
        assert_eq!(recovery.phase,"reground");
        assert!(recovery.allows("workspace_read_range"));
        assert!(!recovery.allows("workspace_replace_range"),
            "a failed hash must be re-read before the next write");
        compared.push(op("workspace_read_range","completed",json!({"sourceSha256":"new"})));
        assert_eq!(route(&policy,&compared,5,true,false).unwrap().phase,"edit");
        compared.push(op("code_search","completed",json!({})));
        assert_eq!(route(&policy,&compared,6,true,false).unwrap().phase,"edit");
        let mut verified=op("workspace_replace_range","completed",json!({"written":true}));
        verified["toolReceipt"]=json!({"verified":true});
        compared.push(verified);
        assert!(route(&policy,&compared,7,true,false).is_none());
    }
    #[test]
    fn observed_program_stage_stalls_change_descendant_procedure() {
        let parent=["discover","ground","contrast","edit","verify"]
            .iter().map(|s|s.to_string()).collect::<Vec<_>>();
        let observed=json!({"jobId":"worker","tokens":12400,
            "programStages":[{"phase":"discover"},{"phase":"ground"},{"phase":"contrast"}],
            "editEvidence":{"verifiedWrites":0},"candidateId":null});
        let child=mutate(&parent,&observed);
        assert!(!child.contains(&"contrast".to_string()));
        assert!(child.contains(&"edit".to_string()));
        // A fabricated unexecuted phase is not read directly by the
        // parent campaign: rsi_edit_genome filters zero-token workers.
    }

    #[test]
    fn focused_source_hash_is_read_once_then_a_real_edit_is_required(){
        let p=serde_json::json!({"program":["discover","ground","edit","verify"],
            "checkpointAfter":5});
        let first=route(&p,&[],0,true,true).unwrap();
        assert_eq!(first.phase,"focus_ground");
        assert!(first.allows("workspace_read_range"));
        assert!(!first.allows("code_search"));
        assert!(!first.allows("workspace_replace"));
        let read=vec![op("workspace_read_range","completed",
            serde_json::json!({"sourceSha256":"current"}) )];
        let second=route(&p,&read,1,true,true).unwrap();
        assert_eq!(second.phase,"edit");
        assert!(second.allows("workspace_replace_range"));
        // A failed hash forces re-grounding before any further edits.
        let mut recovered=read;
        recovered.push(op("workspace_replace_range","failed",
            serde_json::json!({"error":"sha mismatch"})));
        let p2=serde_json::json!({"program":["ground","edit","reground","verify"]});
        assert_eq!(route(&p2,&recovered,2,true,true).unwrap().phase,"reground");
        assert!(route(&p,&[],0,false,true).is_none());
    }
    #[test]
    fn unverified_hash_edit_forces_recovery_without_learned_repair_node(){
        let policy=serde_json::json!({"program":["discover","ground","edit","verify"],
            "checkpointAfter":2});
        let ops=vec![
            op("workspace_read_range","completed",serde_json::json!({})),
            op("workspace_replace_range","completed",serde_json::json!({"written":true}))
        ];
        assert_eq!(route(&policy,&ops,2,true,false).unwrap().phase,"reground");
        let mut recovered=ops;
        recovered.push(op("workspace_read_range","completed",serde_json::json!({})));
        assert_eq!(route(&policy,&recovered,3,true,false).unwrap().phase,"edit");
    }
    #[test]
    fn normal_chat_and_legacy_controller_are_unchanged() {
        assert!(route(&json!({}),&[],0,true,false).is_none());
        let policy=json!({"program":["ground","edit","verify"]});
        assert!(route(&policy,&[],0,false,false).is_none());
        assert_eq!(route(&policy,&[],3,true,false).unwrap().phase,"ground");
    }
}
