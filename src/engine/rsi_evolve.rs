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
#[path = "rsi/tests/inheritance.rs"]
mod sub2_inherited_main_capabilities_tests;
