//! The only AppState-reading adapter for compiled recursive meta planning.
//! Evidence, peer archives and an inherited procedure are resolved here.
//! The experiment-policy composer itself remains pure and may be recompiled
//! and rewritten by the descendant without importing the runtime store.
use super::{AppState,Value};

pub(super) struct PreparedMetaInputs {
    pub(super) method_transfer:Value,
    pub(super) peers:Value,
}
pub(super) fn prepare(
    state:&AppState,input:&Value,history:&[Value],
    campaign:&str,explicit:bool,
)->Result<PreparedMetaInputs,String>{
    let method_transfer=if explicit {Value::Null}
        else{crate::engine::rsi_system::method_transfer_target(state,history,input)};
    let profile=crate::engine::rsi_profile::from_input(input)?;
    let peers=crate::engine::rsi_system::peer_lineage_references(
        state,campaign,profile["sha256"].as_str().unwrap_or(""),
        input["rsiIndexId"].as_str().unwrap_or(""),None);
    Ok(PreparedMetaInputs{method_transfer,peers})
}
