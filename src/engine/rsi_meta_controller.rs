//! Editable metacognitive controller compiled into each accepted descendant.
//! Inspired by HyperAgents, predictive improvement dynamics, and comparative
//! cross-trajectory evolution. It proposes search experiments; frozen independent
//! evaluation is the sole authority for adoption. Every forecast is calibrated
//! against the *next* observed, separately verified outcome.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};


// The adopted descendant compiles these stages into its own revisable controller.
// State transitions are proposals, never substitutes for independent evidence.
#[path = "rsi/meta_controller/provenance.rs"]
mod provenance;
use provenance::*;
pub(crate) use provenance::meta_source_changed;
#[path = "rsi/meta_controller/reciprocal.rs"]
mod reciprocal;
use reciprocal::*;
pub(crate) use reciprocal::attach_reciprocal_focus;
#[path = "rsi/meta_controller/selection.rs"]
mod selection;
use selection::*;
pub(crate) use selection::synthesize;
#[path = "rsi/meta_controller/transfer.rs"]
mod transfer;
use transfer::*;
pub(crate) use transfer::{comparison_profile, peer_lineages, validate_peer_lineages,
    next_method_hypothesis, compositional_hypothesis};
#[path = "rsi/meta_controller/feedback.rs"]
mod feedback_stage;
use feedback_stage::*;
pub(crate) use feedback_stage::{instruction, feedback};
#[cfg(test)]
#[path = "rsi/meta_controller/tests.rs"]
mod tests;
