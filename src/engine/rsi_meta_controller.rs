//! Recompilable, self-modifying improvement controller.
//! The successor owns proposal selection and the model-authored redesign.
//! The resident coordinator only freezes budgets, source identity and oracles.
//!
//! Influences: HyperAgents' editable meta-agent, IDM's calibrated predictions,
//! MGM's comparative evolution and GEPA-Curator's trusted experience selection.
//! Unlike a hard-coded patch operator, this policy and its exploration model
//! are ordinary mutable Rust source in every accepted descendant.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

// This module preserves the stable engine API; implementation ownership
// belongs to the RSI meta subsystem. These modules are actual live code.
#[path = "rsi/meta/controller_evidence.rs"]
mod evidence;
use evidence::*;
#[path = "rsi/meta/controller_selection.rs"]
mod selection;
pub(crate) use selection::synthesize;
#[path = "rsi/meta/controller_policy.rs"]
mod policy;
pub(crate) use policy::*;
#[path = "rsi/meta/controller_feedback.rs"]
mod feedback_logic;
pub(crate) use feedback_logic::feedback;

// Heavy regression fixtures live outside the production source inventory.
#[cfg(test)]
include!("../../tests/support/engine/rsi_meta_controller.tests.rs");
