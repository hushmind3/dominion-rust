//! Native repeated-observation guard. Inspired by source inspection of Rig's
//! Gate loop-breaker, but implemented for DOMINION's persisted tool lifecycle.
//! Never deduplicates writes, network operations, task polls or missing evidence.
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Clone)]
struct Observation {
    fingerprint: String,
    version: u64,
    digest: String,
    consecutive: usize,
}

#[derive(Default)]
pub(super) struct ReadLoopGuard {
    observations: HashMap<String, Observation>,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum RepeatDecision { Allow, Refuse, Stop }

fn local_snapshot_read(name: &str) -> bool {
    matches!(name,"workspace_read"|"workspace_read_range"|"file_read"
        |"workspace_list"|"file_list")
}

impl ReadLoopGuard {
    /// Only refuse after TWO observed identical successful outputs. A file
    /// mutation (new environment version), changed metadata or changed data
    /// always reopens observation. External status polling is never blocked.
    pub(super) fn decision(&self,tool:&str,key:&str,fingerprint:&str,version:u64,
        attempts:usize)->RepeatDecision {
        if !local_snapshot_read(tool) || attempts<3 {return RepeatDecision::Allow}
        let Some(prior)=self.observations.get(key) else{return RepeatDecision::Allow};
        if prior.version!=version || prior.fingerprint!=fingerprint || prior.consecutive<2 {
            return RepeatDecision::Allow;
        }
        if attempts>=5 {RepeatDecision::Stop} else {RepeatDecision::Refuse}
    }

    pub(super) fn observe(&mut self,tool:&str,key:&str,fingerprint:&str,version:u64,
        actual_result:&str,success:bool) {
        if !local_snapshot_read(tool) {return}
        if !success {
            self.observations.remove(key);
            return;
        }
        let digest=format!("{:x}",Sha256::digest(actual_result.as_bytes()));
        match self.observations.get_mut(key) {
            Some(old) if old.version==version && old.fingerprint==fingerprint
                && old.digest==digest => {
                old.consecutive=old.consecutive.saturating_add(1);
            }
            _ => {
                self.observations.insert(key.into(),Observation {
                    fingerprint:fingerprint.into(),version,digest,consecutive:1
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_identical_successful_local_read_is_refused_before_third_execution(){
        let mut guard=ReadLoopGuard::default();
        assert_eq!(guard.decision("workspace_read","k","fp",0,1),RepeatDecision::Allow);
        guard.observe("workspace_read","k","fp",0,"real file data",true);
        assert_eq!(guard.decision("workspace_read","k","fp",0,2),RepeatDecision::Allow);
        guard.observe("workspace_read","k","fp",0,"real file data",true);
        assert_eq!(guard.decision("workspace_read","k","fp",0,3),RepeatDecision::Refuse);
        assert_eq!(guard.decision("workspace_read","k","fp",0,5),RepeatDecision::Stop);
    }
    #[test]
    fn changed_observation_mutation_or_fingerprint_breaks_the_loop(){
        let mut guard=ReadLoopGuard::default();
        guard.observe("file_read","k","fp",0,"A",true);
        guard.observe("file_read","k","fp",0,"B",true);
        assert_eq!(guard.decision("file_read","k","fp",0,3),RepeatDecision::Allow);
        guard.observe("file_read","k","fp",0,"B",true);
        assert_eq!(guard.decision("file_read","k","fp",1,3),RepeatDecision::Allow);
        assert_eq!(guard.decision("file_read","k","new-file-mtime",0,3),RepeatDecision::Allow);
        guard.observe("file_read","k","fp",0,"ERROR",false);
        assert_eq!(guard.decision("file_read","k","fp",0,4),RepeatDecision::Allow);
    }
    #[test]
    fn changing_external_status_and_nonread_effects_never_auto_refuse(){
        let mut guard=ReadLoopGuard::default();
        for name in ["task_status","task_result","http_fetch","web_search",
            "github_api","shell_exec","workspace_write","delegate_task"] {
            guard.observe(name,"key","fp",0,"same",true);
            guard.observe(name,"key","fp",0,"same",true);
            assert_eq!(guard.decision(name,"key","fp",0,6),RepeatDecision::Allow);
        }
    }
}
