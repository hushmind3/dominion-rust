//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs,collections::{BTreeMap,BTreeSet}};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/reasoning.rs"]),
    ("harness",&[
        "src/engine/jobs.rs", "src/engine/rsi_worktree.rs",
        "src/engine/chat/runtime.rs",
        "src/engine/chat/runtime/intent.rs",
        "src/engine/chat/runtime/turn.rs",
        "src/engine/chat/runtime/adaptation.rs",
        "src/engine/chat/runtime/tool_catalog.rs",
        "src/engine/chat/runtime/execution_program.rs",
        "src/engine/chat/runtime/dispatch.rs",
        "src/engine/chat/runtime/planning.rs",
        "src/engine/chat/runtime/finalization.rs",
        "src/engine/decompose.rs",
    ]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&[
        "src/engine/rsi/planner/domain_selection.rs",
        "src/engine/rsi/planner/policy.rs","src/engine/rsi/planner/context.rs",
        "src/engine/rsi/planner/descendant.rs",
        "src/engine/rsi/execution/generation_preparation.rs",
        "src/engine/rsi/execution/generation.rs",
        "src/engine/rsi/execution/worker.rs",
        "src/engine/rsi/lineage/source.rs","src/engine/rsi/lineage/campaign.rs",
        "src/engine/rsi/learning/methods.rs","src/engine/rsi/learning/research.rs",
        "src/engine/rsi/learning/generation_feedback.rs",
        "src/engine/rsi/meta/evolution.rs",
        "src/engine/rsi/meta/transition_policy.rs",
        "src/engine/rsi/meta/inputs.rs",
        "src/engine/rsi/meta/policy_dispatch.rs",
        "src/engine/rsi/meta/experiment_dispatch.rs",
        "src/engine/rsi/meta/composition.rs",
        "src/engine/rsi/meta/controller_evidence.rs",
        "src/engine/rsi/meta/controller_selection.rs",
        "src/engine/rsi/meta/controller_policy.rs",
        "src/engine/rsi/meta/controller_feedback.rs",
        "src/engine/rsi/meta/reflexive_design.rs",
        "src/engine/rsi/evaluation/trials.rs",
        "src/engine/rsi_system/lineage.rs",
        "src/engine/rsi_system/guidance.rs",
        "src/engine/rsi_factorial.rs","src/engine/rsi_reflexive.rs",
        "src/engine/rsi_component_birth.rs","src/engine/rsi_program_graph.rs",
        "src/engine/rsi_principle_learning.rs","src/engine/rsi_api_route_learning.rs"
    ]),
    ("models",&[
        "src/engine/models.rs",
        "src/engine/models/inventory.rs",
        "src/engine/models/runtime_install.rs",
        "src/engine/models/registration.rs",
        "src/engine/models/session.rs",
        "src/engine/models/catalog.rs",
        "src/engine/models/downloads.rs",
        "src/api/providers.rs",
        "src/api/providers/auth_sources.rs",
        "src/api/providers/discovery.rs",
        "src/api/providers/cli_credentials.rs",
        "src/api/providers/codex_login.rs",
        "src/api/providers/provider_store.rs",
        "src/api/providers/connection_test.rs",
        "src/api/providers/model_list.rs",
        "src/engine/cli_model.rs", "src/engine/router.rs", "src/engine/gateway.rs",
    ]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

mod inventory;
use inventory::{source_inventory,source_role};
pub(crate) use inventory::source_facts;

mod design;
use design::{design_roles,role_domain,valid_role};
pub(crate) use design::design_implemented;

mod lineage;
use lineage::{adopted_design,latest_adopted_design};
pub(crate) use lineage::{lineage_capsule,peer_lineage_references,
    inherited_domain,inherited_stage,method_transfer_target};

// Peer receipt replay is an immutable supervisor gate, unlike mutable ranking.
mod peer_verifier;
pub(crate) use peer_verifier::validate_peer_references;

mod guidance;
pub(crate) use guidance::{instruction,improvement_directive};

mod proposal;
pub(crate) use proposal::propose;

mod handoff;
pub(crate) use handoff::handoff_evidence;

/// Ordinary agent work feeds system evolution as evidence, never as an
/// automatic success label. Retain record IDs so the model can inspect the
/// original execution rather than repeatedly collecting the same failure.
pub(crate) fn usage_evidence(state:&crate::state::AppState)->Value {
    let report=crate::engine::rsi_scan::report(state);
    let signals=report["signals"].as_array().into_iter().flatten()
        .filter(|signal|matches!(signal["kind"].as_str(),Some("execution_failures"|"failed_jobs")))
        .take(6).cloned().collect::<Vec<_>>();
    json!({"source":"recorded_agent_work","signals":signals,"checks":report["checks"],"qualityProven":false})
}

mod frontier;
pub(crate) use frontier::architecture_frontier;

/// Immutable fact validation intentionally accepts different descendant
/// rankings/roles. Recomputing architecture_frontier here would freeze the
/// previous generation's intelligence and reject legitimate evolution.
pub(crate) fn validate_plan_evidence(state:&crate::state::AppState, root:&Path,
    commit:&str,index_id:&str,plan:&Value)->Result<(),String> {
    let model=&plan["systemBlueprint"];let facts=source_facts(root);
    if model["generatorCommit"]!=commit || model["sourceTreeSha256"]!=facts["sha256"]
        || model["sourceFileCount"]!=facts["fileCount"] {return Err("후손의 자기모형이 현재 전체 소스와 다릅니다.".into());}
    let roles=model["architecture"].as_array().filter(|roles|!roles.is_empty()&&roles.len()<=64)
        .ok_or("후손의 구성요소 목록이 없습니다.")?;
    let mut names=BTreeSet::new();
    for role in roles {
        let name=role["role"].as_str().filter(|name|valid_role(name)).ok_or("구성요소 이름 오류")?;
        if !names.insert(name){return Err("중복 구성요소 이름".into());}
        for entry in role["representativeSources"].as_array().into_iter().flatten() {
            validate_source_entry(state,root,index_id,entry,"sha256")?;
        }
    }
    let rows=plan["architectureFrontier"].as_array().filter(|rows|rows.len()<=64)
        .ok_or("후손의 구조 탐색 근거가 없습니다.")?;
    for row in rows {
        if !row["role"].as_str().is_some_and(|role|names.contains(role))
            || !row["domain"].as_str().is_some_and(|domain|crate::engine::rsi_evolve::DOMAINS.contains(&domain)) {
            return Err("후손 탐색 근거의 구성요소·분야 오류".into());
        }
        validate_source_entry(state,root,index_id,row,"sourceSha256")?;
    }
    if !plan["architectureTarget"].is_null() && !rows.iter().any(|row|*row==plan["architectureTarget"]) {
        return Err("선택한 구조 과제의 실제 소스 근거가 없습니다.".into());
    }
    Ok(())
}

fn validate_source_entry(state:&crate::state::AppState,root:&Path,index_id:&str,
    entry:&Value,hash_key:&str)->Result<(),String> {
    let path=entry["path"].as_str().filter(|path|crate::engine::rsi::safe_path(path)).ok_or("허용되지 않은 소스 경로")?;
    let sha=entry[hash_key].as_str().ok_or("소스 해시가 없습니다.")?;
    let base=fs::canonicalize(root).map_err(|e|e.to_string())?;
    let actual=fs::canonicalize(base.join(path)).map_err(|e|e.to_string())?;
    if !actual.starts_with(&base) || !actual.is_file()
        || crate::engine::code_index::indexed_source_hash(state,index_id,path).as_deref()!=Some(sha)
        || format!("{:x}",Sha256::digest(fs::read(actual).map_err(|e|e.to_string())?))!=sha {
        return Err(format!("현재 소스와 다른 탐색 근거: {path}"));
    }
    Ok(())
}

/// Read only source assets under the experiment's pinned checkout. The
/// coordinator cannot author or alter this blueprint without changing its
/// measured generator revision; no holdout content is read here.
pub(crate) fn blueprint(root:&Path, revision:&str, history:&[Value])->Value {
    let canonical_root=fs::canonicalize(root).ok();
    let all_files=source_inventory(root);
    let mut components=Vec::new();
    for (role,paths) in COMPONENTS {
        let mut inventory=Vec::new();
        for relative in *paths {
            if !crate::engine::rsi::safe_path(relative) {continue;}
            let Some(root)=canonical_root.as_ref() else {continue;};
            let Ok(file)=fs::canonicalize(root.join(relative)) else {continue;};
            if !file.starts_with(root)||!file.is_file(){continue;}
            let Ok(contents)=fs::read(&file) else {continue;};
            inventory.push(json!({"path":relative,"sha256":format!("{:x}",Sha256::digest(&contents)),
                "bytes":contents.len()}));
        }
        // Candidate selection is source-graph aware: newly created files may
        // enter any subsystem without first being added to this static table.
        // Bound the transmitted representatives, not the source fingerprint.
        let total_sources=all_files.iter().filter(|entry|
            entry["path"].as_str().is_some_and(|path|source_role(path)==*role)).count();
        for entry in all_files.iter().filter(|entry|
            entry["path"].as_str().is_some_and(|path|source_role(path)==*role)) {
            if inventory.len()>=24 {break;}
            if !inventory.iter().any(|item|item["path"]==entry["path"]) {
                inventory.push(entry.clone());
            }
        }
        let row=json!({"role":role,"representativeSources":inventory});
        let domain=role_domain(&row).to_string();
        components.push(json!({"role":role,"domain":domain,"representativeSources":inventory,
            "discoveredSourceCount":total_sources}));
    }
    for generation in history.iter().filter(|generation|
        crate::engine::rsi_mutual_program::adopted_source(generation)) {
        let design=&generation["systemDesign"];
        for role in design_roles(design) {
            if components.iter().any(|item|item["role"]==role){continue;}
            // Explicitly invented capabilities have a stronger contract:
            // both the module and its distinct runtime integration must be
            // independently adopted; the old fallback cannot bypass it.
            if design["newComponents"].as_array().is_some_and(|items|
                items.iter().any(|item|item["role"]==role)) {continue;}
            let paths=design["componentFiles"][&role].as_array().or_else(||
                if design_roles(design).len()==1 {design["targetFiles"].as_array()}else{None});
            let sources=paths.into_iter().flatten().filter_map(|path| {
                let path=path.as_str().filter(|path|crate::engine::rsi::safe_path(path))?;
                let bytes=fs::read(root.join(path)).ok()?;let sha=format!("{:x}",Sha256::digest(&bytes));
                if generation["acceptedSourceHashes"][path]!=sha {return None;}
                Some(json!({"path":path,"sha256":sha,"bytes":bytes.len()}))
            }).collect::<Vec<_>>();
            if !sources.is_empty() && components.len()<64 {
                components.push(json!({"role":role,"domain":design["nextDomain"],"representativeSources":sources}));
            }
        }
    }
    for born in crate::engine::rsi_component_birth::admitted(root,history) {
        if !components.iter().any(|item|item["role"]==born["role"])
            && components.len()<64 {
            components.push(born);
        }
    }
    let evidence=history.iter().rev().take(8).map(|generation|json!({
        "generatorCommit":generation["generatorCommit"],
        "acceptedCommit":generation["acceptedCommit"],
        "domain":generation["domain"],
        "candidateTarget":generation["candidateTarget"],
        "accepted":generation["accepted"]==true,
        "verdict":generation["evaluation"]["verdict"],
        "improvedHoldouts":generation["evaluation"]["metrics"]["improvedHoldouts"],
        "metaHoldoutGains":generation["evaluation"]["metrics"]["metaHoldoutGains"],
        "regressions":generation["evaluation"]["metrics"]["regressions"],
        "methodAblation":generation["methodAblation"],
        "candidateOrigin":generation["candidateOrigin"]
    })).collect::<Vec<_>>();
    let files=all_files;
    let fingerprint=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()));
    let total=files.len();
    // Hash ALL indexed source, but transport only a bounded sample: a
    // 2,048-module system would overflow the planner's stdout capture and
    // needlessly resend tens of thousands of SHA strings to the model.
    let preview=files.into_iter().take(64).collect::<Vec<_>>();
    json!({
        "source":"compiled_descendant_system_blueprint_v1",
        "generatorCommit":revision,
        "architecture":components,
        "sourceInventory":preview,
        "sourceFileCount":total,
        "sourceInventoryTruncated":total>64,
        "sourceTreeSha256":fingerprint,
        "historyCount":history.len(),
        "inheritedDesign":latest_adopted_design(history).cloned().unwrap_or(Value::Null),
        "inheritedDesignLineage":lineage_capsule(history)["acceptedDesigns"].clone(),
        "lineageSha256":lineage_capsule(history)["sha256"].clone(),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_source_editing_runtime",
        "immutableBoundary":["experiment_baseline","frozen_paired_evaluator","budget","production_apply_gate"]
    })
}

// Heavy regression fixtures live outside the production source inventory.
#[cfg(test)]
include!("../../tests/support/engine/rsi_system.tests.rs");

