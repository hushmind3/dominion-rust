//! Source-stable, versioned interfaces for agents, experiments and proof.
//! This schema boundary has no store, provider, model or planner dependency.
//! Neither the editable policy nor the frozen evaluator owns the other's types.
#[path = "contracts/experiment.rs"]
mod experiment;
pub(crate) use experiment::{
    ExperimentDesign,ExperimentProtocol,ExperimentTarget,verify_experiment_design
};
