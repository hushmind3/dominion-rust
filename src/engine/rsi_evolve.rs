//! Opt-in experimental evolution. The coordinator and its oracle stay frozen;
//! each accepted source revision is rebuilt and actually generates the next patch.
//! Only private detached worktrees are committed. Production refs never move.
use crate::{
    engine::{
        experience,
        jobs::{self, JobContext},
        memory::MemoryScope,
        rsi, rsi_worktree, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::process::Command;

pub const DOMAINS: &[&str] = &["memory", "tools", "context", "learning", "ui", "improver"];
// Responsibility boundaries. The top-level coordinator retains its existing
// Rust API; each subsystem owns the same implementation, not a copy.
#[path = "rsi/context.rs"]
mod context;
#[path = "rsi/research.rs"]
mod research;
#[path = "rsi/lineage.rs"]
mod lineage;
#[path = "rsi/worker.rs"]
mod worker;
#[path = "rsi/search.rs"]
mod search;
#[path = "rsi/source.rs"]
mod source;
#[path = "rsi/campaign.rs"]
mod campaign;
#[path = "rsi/learning.rs"]
mod learning;
#[path = "rsi/evaluation.rs"]
mod evaluation;
#[path = "rsi/execution.rs"]
mod execution;
#[path = "rsi/planner.rs"]
mod planner;

use context::*;
use research::*;
use lineage::*;
use worker::*;
use search::*;
use source::*;
use campaign::*;
use learning::*;
use evaluation::*;
use execution::*;
use planner::*;

// Stable public facade: callers keep crate::engine::rsi_evolve::* paths.
pub use search::{limits,evaluate_search_algorithm};
pub(crate) use search::select_domain;
pub use source::{
    auto_write_allowed,auto_write_denial_reason,campaign_source,
    remaining_output_tokens,lineage_fingerprint_valid,
};
pub(crate) use source::command;
pub use campaign::{choose,run};
pub(crate) use context::policy_requests_source_edit;
pub(crate) use research::{remember_research,reuse_research};
pub(crate) use lineage::adopt;
pub use worker::{plan_generation,worker};

// Preserve external call sites in tools, policy and descendant workers.
pub(crate) use learning::{
    propose_method, configure_method_session,
    method_strategies, collect_method_executions,
};


#[cfg(test)]
#[path = "rsi_oracle_fixture.rs"]
mod oracle_tests;

#[cfg(test)]
#[path = "rsi/tests/evolve.rs"]
mod tests;

#[cfg(test)]
mod sub2_inherited_main_capabilities_tests {
    use super::*;
    #[test]
    fn an_unevaluated_or_model_claimed_rule_cannot_earn_quality_credit() {
        let fake=json!({"jobId":"worker-1","accepted":true,"pairedRevalidated":true,
            "modelAttributionConsistent":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evidenceVerified":true,
            "evaluation":{"metrics":{"eligible":true}},
            "editPolicy":{"revision":"r1","rules":[{"id":"learnt-guard"}]},
            "acceptedSourceHashes":{"src/engine/memory.rs":"a".repeat(64)},
            "ruleExecutions":[{"ruleId":"learnt-guard",
                "generationJobId":"worker-1",
                "policyRevision":"r1","runId":"run1","opSeqBefore":1,
                "allow":["workspace_replace"]}],
            "toolTimeline":[]});
        assert_eq!(sub2_edit_policy_reward(&fake),0.0);
        let mut unverified=fake.clone();
        unverified["evidenceVerified"]=json!(false);
        assert_eq!(sub2_edit_policy_reward(&unverified),0.0);
        let mut no_receipt=fake.clone();
        no_receipt["toolTimeline"]=json!([{"runId":"run1","opSeq":2,
            "tool":"workspace_replace","status":"completed",
            "path":"src/engine/memory.rs",
            "result":{"written":true,"postSha256":"a".repeat(64)},
            "toolReceipt":{"verified":false}}]);
        assert_eq!(sub2_edit_policy_reward(&no_receipt),0.0);
        let mut real=no_receipt;
        real["toolTimeline"][0]["toolReceipt"]["verified"]=json!(true);
        assert_eq!(sub2_edit_policy_reward(&real),1.0);
        real["modelAttributionConsistent"]=json!(false);
        assert_eq!(sub2_edit_policy_reward(&real),0.0,
            "mixed-model sessions cannot promote an edit rule");
    }
    #[test]
    fn rewritten_edit_program_needs_a_later_verified_adopted_source_write() {
        let mut g=json!({
            "jobId":"worker-program","accepted":true,
            "modelAttributionConsistent":true,
            "pairedRevalidated":true,"evidenceVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "editPolicy":{"revision":"rewritten-v2",
                "parentRevision":"original-v1","rules":[]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"a".repeat(64)},
            "programStages":[{"runId":"session-1","generationJobId":"worker-program",
                "policyRevision":"rewritten-v2","phase":"ground","opSeqBefore":1}],
            "toolTimeline":[{"runId":"session-1","opSeq":4,
                "tool":"workspace_replace_function","status":"completed",
                "path":"src/engine/learning.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}}]
        });
        assert_eq!(sub2_edit_policy_reward(&g),0.0,
            "source success cannot be credited to a ground-only policy");
        g["programStages"][0]["phase"]=json!("edit");
        assert_eq!(sub2_edit_policy_reward(&g),1.0);
        g["programStages"][0]["opSeqBefore"]=json!(4);
        assert_eq!(sub2_edit_policy_reward(&g),0.0,
            "an edit-stage claim after the actual write cannot steal credit");
        g["programStages"][0]["opSeqBefore"]=json!(1);
        g["toolTimeline"][0]["toolReceipt"]["verified"]=json!(false);
        assert_eq!(sub2_edit_policy_reward(&g),0.0,
            "a tool result without independently verified source receipt is not proof");
    }
    #[test]
    fn neutral_source_patch_is_not_edit_program_quality_proof() {
        let neutral=json!({"accepted":false,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":"neutral-1",
            "evaluation":{"metrics":{"regressions":0}},
            "editPolicy":{"revision":"r1","rules":[]}});
        assert!(sub2_edit_policy_reward(&neutral)<0.11);
        let native=json!({"accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"native_inductive_source_mutation",
            "evidenceVerified":true,
            "editPolicy":{"revision":"r1","rules":[]}});
        assert_eq!(sub2_edit_policy_reward(&native),0.0);
    }
}
