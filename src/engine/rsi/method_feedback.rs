//! Descendant-editable feedback adaptation interface.
//! The independent proof supplies replayed evidence; experiment strategy remains editable.
use super::*;

pub(super) fn method_snapshot(state:&AppState,history:&[Value],binary_commit:&str)->Value {
    super::proof::inherited_method_snapshot(state,history,binary_commit)
}

pub(super) fn downstream_feedback(state:&AppState,history:&[Value],binary_commit:&str)->Value {
    super::proof::verified_downstream_feedback(state,history,binary_commit)
}
