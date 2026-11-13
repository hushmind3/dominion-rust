#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_file_is_not_meta_improvement() {
        assert!(meta_source("src/engine/rsi/planner/descendant.rs"));
        assert!(meta_source("src/engine/rsi_system/lineage.rs"));
        assert!(meta_source("src/engine/rsi_system/handoff.rs"));
        assert!(!meta_source("src/engine/memory.rs"));
        assert!(!meta_source("README.md"));
    }
    #[test]
    fn forged_action_flag_does_not_pass_handoff(){
        let p=json!({"generatorCommit":"old","acceptedCommit":"new","generatorBinarySha256":"a"});
        let c=json!({"generatorCommit":"new","generatorBinarySha256":"b",
            "behaviorCounterfactual":{"status":"behavior_changed"}});
        assert!(!handoff(&p,&c,true));
    }
}
