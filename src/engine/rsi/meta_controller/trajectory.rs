//! Evidence-bound experience reuse for compiled RSI descendants.
//! Episodes inform the next design; only the frozen evaluator can adopt code.
use super::*;
use std::collections::BTreeSet;

/// Locate an independently accepted *alternative* on the same original source
/// commit and overlapping edited files. This is stricter than matching a
/// natural-language goal, and deliberately does not claim causal repair.
pub(super) fn source_matched_alternatives(history: &[Value]) -> Vec<Value> {
    let mut output = Vec::new();
    let mut unique = BTreeSet::new();
    for (index, failed) in history.iter().enumerate().rev() {
        if output.len() >= 4 { break; }
        if model_design_roles(failed).is_empty() { continue; }
        let Some(kind) = observed_class(failed) else { continue };
        if kind == "adopted" { continue; }
        let Some(base) = failed["generatorCommit"].as_str()
            .filter(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        else { continue };
        let failed_roles = model_design_roles(failed);
        let failed_paths = failed["systemDesign"]["targetFiles"].as_array()
            .into_iter().flatten().filter_map(Value::as_str)
            .filter(|path| crate::engine::rsi::safe_path(path))
            .collect::<Vec<_>>();
        if failed_paths.is_empty() { continue; }
        // The alternative follows this attempt, and both began from the
        // same source. An unrelated later success cannot masquerade as repair.
        let matched = history.iter().skip(index + 1).rev().find_map(|accepted| {
            if !independent_adoption(accepted)
                || accepted["generatorCommit"].as_str() != Some(base)
                || !model_design_roles(accepted).iter().any(|r| failed_roles.contains(r))
            { return None; }
            let paths = accepted["systemDesign"]["targetFiles"].as_array()?;
            let shared = paths.iter().filter_map(Value::as_str)
                .filter(|path| failed_paths.contains(path))
                .filter_map(|path| {
                    let sha = accepted["acceptedSourceHashes"][path].as_str()?;
                    (sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()))
                        .then(|| json!({"path":path,"acceptedSha256":sha}))
                }).collect::<Vec<_>>();
            if shared.is_empty() { return None; }
            Some((accepted, shared))
        });
        let Some((accepted, shared)) = matched else { continue };
        let key = json!([kind,base,accepted["acceptedCommit"],shared]).to_string();
        if !unique.insert(key) { continue; }
        output.push(json!({
            "failedJobId":failed["jobId"],
            "acceptedJobId":accepted["jobId"],
            "failureKind":kind,
            "baseCommit":base,
            "acceptedCommit":accepted["acceptedCommit"],
            "sharedChangedSources":shared,
            "acceptedHypothesis":accepted["systemDesign"]["hypothesis"]
                .as_str().unwrap_or("").chars().take(400).collect::<String>(),
            "source":"model_authored_paired_independent_adoption",
            "causalRepairProven":false
        }));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_alternative_only_with_same_base_and_real_source_receipts() {
        let base="a".repeat(40);
        let path="src/engine/jobs.rs";
        let failed=json!({
            "jobId":"bad","generatorCommit":base,"accepted":false,
            "candidateId":"candidate-bad",
            "evaluation":{"verdict":"build_failed"},
            "systemDesign":{
                "source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":"bad",
                "component":"harness","targetFiles":[path],
                "hypothesis":"first plan failed to compile"
            }
        });
        let adopted=json!({
            "jobId":"good","generatorCommit":base,"accepted":true,
            "pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":"b".repeat(40),
            "acceptedSourceHashes":{"src/engine/jobs.rs":"c".repeat(64)},
            "evaluation":{"verdict":"adopted","metrics":{"eligible":true}},
            "systemDesign":{
                "source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":"good",
                "component":"harness","targetFiles":[path],
                "hypothesis":"fix the orchestration with a new approach"
            }
        });
        let learned=source_matched_alternatives(&[failed.clone(),adopted.clone()]);
        assert_eq!(learned.len(),1);
        assert_eq!(learned[0]["failureKind"],"compiler_failure");
        assert_eq!(learned[0]["acceptedJobId"],"good");
        assert_eq!(learned[0]["causalRepairProven"],false);
        assert_eq!(learned[0]["sharedChangedSources"][0]["acceptedSha256"],"c".repeat(64));
        let mut fake=adopted.clone();
        fake["pairedRevalidated"]=json!(false);
        assert!(source_matched_alternatives(&[failed.clone(),fake]).is_empty());
        let mut unrelated=adopted.clone();
        unrelated["generatorCommit"]=json!("d".repeat(40));
        unrelated["systemDesign"]["generatorCommit"]=json!("d".repeat(40));
        assert!(source_matched_alternatives(&[failed.clone(),unrelated]).is_empty());
        let mut wrong_file=adopted;
        wrong_file["systemDesign"]["targetFiles"]=json!(["src/engine/memory.rs"]);
        wrong_file["acceptedSourceHashes"]=json!({"src/engine/memory.rs":"c".repeat(64)});
        assert!(source_matched_alternatives(&[failed,wrong_file]).is_empty());
    }
}