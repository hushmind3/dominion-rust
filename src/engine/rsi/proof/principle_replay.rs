//! Protected evidence replay and lineage attestation, separate from editable
//! improvement targeting and learned program synthesis.
use super::*;

pub(crate) fn verify_replay(state:&crate::state::AppState,history:&[Value])->bool {
    for g in history.iter().filter(|g|independent_trial(g)) {
        let Some(id)=g["candidateId"].as_str() else{return false};
        let Some(candidate)=state.store.get("rsi_candidate",id) else{return false};
        let Some(metrics)=rsi::replayed_pair_metrics(state,&candidate) else{return false};
        if candidate["id"]!=id
            || candidate["baseCommit"]!=g["generatorCommit"]
            || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
            || candidate["evaluation"]["metrics"]!=metrics
            || g["evaluation"]["metrics"]!=metrics {
            return false;
        }
        let Some((_,_,path))=grounded_mechanism(g) else{return false};
        if !candidate["paths"].as_array().is_some_and(|paths|
            paths.iter().any(|v|v.as_str()==Some(path.as_str()))) {
            return false;
        }
        if g["accepted"]==true
            && (candidate["status"]!="verified"
                || !rsi::verified_pair_receipt(state,&candidate)
                || candidate["experimentAdoption"]["commit"]!=g["acceptedCommit"]
                || candidate["experimentAdoption"]["generatorCommit"]!=g["generatorCommit"]) {
            return false;
        }
    }
    true
}
