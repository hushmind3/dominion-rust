//! Goal HTTP routing facade; responsibility-specific endpoints live below.
use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

#[path = "goals/queries.rs"]
mod queries;
#[path = "goals/owner.rs"]
mod owner;
#[path = "goals/create.rs"]
mod create;
#[path = "goals/worktree.rs"]
mod worktree;

pub use queries::{goal_metrics,list_goals,get_goal,delete_goal};
pub use owner::{edit_plan,steer_goal,cancel_goal,resume_goal};
pub use create::create_goal;
pub use worktree::{changes,discard_changes,apply_changes};

#[cfg(test)]
#[path = "../../tests/unit/goals_api_lifecycle_tests.rs"]
mod lifecycle_tests;
