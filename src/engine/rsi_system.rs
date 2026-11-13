//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs,collections::{BTreeMap,BTreeSet}};

// The current compiled descendant owns this revisable source model.
// These responsibilities are separate from the immutable paired evaluator.
#[path = "rsi/system/inventory.rs"]
mod inventory;
use inventory::*;
pub(crate) use inventory::{intelligence_editable_path, source_facts};
#[path = "rsi/system/blueprint.rs"]
mod blueprint_stage;
use blueprint_stage::*;
pub(crate) use blueprint_stage::blueprint;
#[path = "rsi/system/discovery.rs"]
mod discovery;
use discovery::*;
pub(crate) use discovery::research_archive;
#[path = "rsi/system/frontier.rs"]
mod frontier;
use frontier::*;
pub(crate) use frontier::architecture_frontier;
#[path = "rsi/system/agenda.rs"]
mod agenda;
use agenda::*;
pub(crate) use agenda::{system_agenda,validate_system_agenda,inherited_program,
    inherited_stage,agenda_instruction,instruction};
#[path = "rsi/system/design.rs"]
mod design;
use design::*;
pub(crate) use design::{propose,inherited_domain};
#[cfg(test)]
#[path = "rsi/system/tests.rs"]
mod tests;
