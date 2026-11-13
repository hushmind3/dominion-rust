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
        "src/engine/rsi/learning/principle_calibration.rs",
        "src/engine/rsi_system/inventory.rs",
        "src/engine/rsi_system/frontier.rs",
        "src/engine/rsi_system/validation.rs",
        "src/engine/rsi_system/blueprint.rs",
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

mod validation;
pub(crate) use validation::validate_plan_evidence;

mod blueprint;
pub(crate) use blueprint::blueprint;

// Heavy regression fixtures live outside the production source inventory.
#[cfg(test)]
include!("../../tests/support/engine/rsi_system.tests.rs");

