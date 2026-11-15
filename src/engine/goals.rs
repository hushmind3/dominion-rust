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

#[path = "goals/report.rs"]
mod report;
#[path = "goals/git.rs"]
mod git;
#[path = "goals/progress.rs"]
mod progress;
#[path = "goals/metrics.rs"]
pub mod metrics;
#[path = "goals/operations.rs"]
mod operations;
#[path = "goals/execution.rs"]
mod execution;
#[path = "goals/lifecycle.rs"]
mod lifecycle;
#[path = "goals/review.rs"]
mod review;

pub use execution::run;
pub use review::parse_review;
use lifecycle::{finish,role_model,optional,tail,patch_goal};
use operations::{in_goal_session_tree,unknown_new_goal_operations};

#[cfg(test)]
#[path = "../../tests/unit/engine_goals_tests.rs"]
mod tests;
