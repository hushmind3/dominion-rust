//! Durable autonomous goal orchestration. Contracts, runtime, finalization and
//! review are deliberately separated to keep the parent module navigable.
use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest,run_quiet_outcome};
use crate::engine::verification::{infer_criteria,string,successful_evidence};
pub use crate::engine::verification::{check_acceptance,prepare_acceptance_checks,validate_criteria};
use crate::state::AppState;
use serde_json::{json,Value};
use std::collections::HashSet;
#[cfg(test)]
use std::path::PathBuf;

#[path = "report.rs"]
mod report;
#[path = "git.rs"]
mod git;
#[path = "progress.rs"]
mod progress;
#[path = "metrics.rs"]
pub mod metrics;
#[path = "operations.rs"]
mod operations;
#[path = "execution.rs"]
mod execution;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "review.rs"]
mod review;

pub use execution::run;
pub use review::parse_review;
use lifecycle::{finish,role_model,optional,tail,patch_goal};
use operations::{in_goal_session_tree,unknown_new_goal_operations};

#[cfg(test)]
#[path = "../../../tests/unit/engine_goals_tests.rs"]
mod tests;
