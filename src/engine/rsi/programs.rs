//! Cohesive source-level RSI execution-program subsystem.
//! Fixed model weights: this subsystem evolves and reuses executable tool methods.
use super::*;

#[path = "programs/action.rs"]
mod action;
pub(super) use action::*;

#[path = "programs/selection.rs"]
mod selection;
pub(super) use selection::*;

#[path = "programs/transfer.rs"]
mod transfer;
pub(super) use transfer::*;
