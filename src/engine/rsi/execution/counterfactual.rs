//! Model-free counterfactual: replay two compiled improvement planners against
//! the same source tree, persisted history and task. This is not a quality score.
use super::{command, Command, Digest, Duration, JobContext, Path, PathBuf, Sha256, Value, json};

fn valid_sha(s:&str, n:usize)->bool {s.len()==n && s.bytes().all(|b|b.is_ascii_hexdigit())}
fn hash(bytes:&[u8])->String {format!("{:x}",Sha256::digest(bytes))}
fn retained(data:&Path,commit:&str,sha:&str)->Result<PathBuf,String>{
    if !valid_sha(commit,40)||!valid_sha(sha,64){return Err("invalid binary identity".into())}
    Ok(data.join("rsi-build/lineage-binaries").join(format!("{commit}-{sha}"))
        .join(format!("dominion-rust{}",std::env::consts::EXE_SUFFIX)))
}

pub(super) fn retain_generator(data:&Path,commit:&str,binary:&Path,sha:&str)->Result<(),String>{
    let target=retained(data,commit,sha)?;
    if hash(&std::fs::read(binary).map_err(|e|e.to_string())?)!=sha {
        return Err("compiled binary digest mismatch".into());
    }
    if target.exists(){
        let meta=std::fs::symlink_metadata(&target).map_err(|e|e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() ||
            hash(&std::fs::read(&target).map_err(|e|e.to_string())?)!=sha {
            return Err("retained generator is not the verified binary".into());
        }
        return Ok(());
    }
    let dir=target.parent().ok_or("missing retained binary directory")?;
    std::fs::create_dir_all(dir).map_err(|e|e.to_string())?;
    let tmp=dir.join(format!("{}.part",uuid::Uuid::new_v4()));
    std::fs::copy(binary,&tmp).map_err(|e|e.to_string())?;
    if hash(&std::fs::read(&tmp).map_err(|e|e.to_string())?)!=sha {
        let _=std::fs::remove_file(&tmp);
        return Err("copied generator digest mismatch".into());
    }
    if let Err(e)=std::fs::rename(&tmp,&target){
        let _=std::fs::remove_file(&tmp);
        if !target.is_file() || hash(&std::fs::read(&target).map_err(|e|e.to_string())?)!=sha {
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// Omit revision labels, commit IDs and generated hashes from the compared
/// decisions; changed metadata is not observable policy behavior.
pub(crate) fn effective_action(plan:&Value)->Value{
    json!({
        "domain":plan["domain"],"focusFiles":plan["focusFiles"],
        "selection":plan["selection"],
        "strategyImplementation":plan["strategy"]["implementation"],
        "strategyParameters":plan["strategy"]["parameters"],
        "metaRole":plan["metaPolicy"]["target"]["role"],
        "metaPath":plan["metaPolicy"]["target"]["path"],
        "selectionMode":plan["metaPolicy"]["selectionMode"],
        "mutualPhase":plan["mutualProgram"]["phase"],
        "reflexivePhase":plan["reflexiveProgram"]["phase"],
        "openTarget":plan["openImprovementProgram"]["target"]["path"],
        "editMethod":plan["editPolicy"]["method"],
        "editStrategy":plan["editPolicy"]["strategy"],
        // A new compiled planner selecting an executable method revision
        // changes its future source-editing behaviour even when the task
        // domain stays the same. This is verified against live worker use.
        "inheritedMethodId":plan["methodTransfer"]["methodId"],
        "inheritedMethodRevision":plan["methodTransfer"]["revision"],
        "feedbackDrivenMetaRevision":plan["feedbackRepairSelected"],
        "provider":plan["apiRoutePolicy"]["chosen"]["providerId"],
        "model":plan["apiRoutePolicy"]["chosen"]["model"]})
}
pub(crate) fn action_hash(plan:&Value)->String {
    hash(&serde_json::to_vec(&effective_action(plan)).unwrap_or_default())
}

pub(super) async fn matched_planner_intervention(
    context:&JobContext,lab:&Path,snapshot:&Value,current_plan:&Value,
    current_binary_sha:&str,history:&[Value]
)->Result<Value,String>{
    let Some(parent)=history.last().filter(|g|g["accepted"]==true
        && g["acceptedCommit"]==snapshot["generatorCommit"]) else{return Ok(Value::Null)};
    let prior_commit=parent["generatorCommit"].as_str().ok_or("missing parent commit")?;
    let prior_sha=parent["generatorBinarySha256"].as_str().ok_or("missing parent binary SHA")?;
    let head=snapshot["generatorCommit"].as_str().ok_or("missing descendant commit")?;
    if prior_commit==head || !valid_sha(current_binary_sha,64) ||
        current_plan["generatorCommit"]!=head ||
        current_plan["historyCount"].as_u64()!=Some(history.len() as u64){
        return Err("invalid distinct descendant planner pair".into());
    }
    let candidate=parent["candidateId"].as_str()
        .and_then(|id|context.state.store.get("rsi_candidate",id))
        .ok_or("parent candidate missing")?;
    if candidate["status"]!="verified" ||
        candidate["experimentAdoption"]["commit"]!=head ||
        !crate::engine::rsi::verified_pair_receipt(&context.state,&candidate){
        return Err("parent adoption has no independent paired receipt".into());
    }
    let binary=retained(&context.state.config.data_dir,prior_commit,prior_sha)?;
    if !binary.is_file() ||
        hash(&std::fs::read(&binary).map_err(|e|e.to_string())?)!=prior_sha {
        return Err("parent compiled binary not retained or digest invalid".into());
    }
    let mut previous=snapshot.clone();
    previous["generatorCommit"]=json!(prior_commit);
    previous["input"]["rsiGeneratorCommit"]=json!(prior_commit);
    previous["input"]["rsiBaseCommit"]=json!(prior_commit);
    let dir=context.state.config.data_dir.join("rsi-controller-plans");
    std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    let path=dir.join(format!("{}-matched-{}.json",context.job_id,uuid::Uuid::new_v4()));
    std::fs::write(&path,serde_json::to_vec(&previous).map_err(|e|e.to_string())?)
        .map_err(|e|e.to_string())?;
    let mut id=Command::new(&binary);
    id.arg("--rsi-worker-identity").current_dir(lab);
    let identity=command(context,id,Duration::from_secs(10)).await;
    if !matches!(identity,Ok(ref out) if out.status.success()
        && String::from_utf8_lossy(&out.stdout).trim()==prior_commit){
        let _=std::fs::remove_file(&path);
        return Err("retained parent binary identity rejected".into());
    }
    let mut probe=Command::new(&binary);
    probe.arg("--rsi-plan-generation").arg(&path).current_dir(lab)
        .env("DOMINION_DATA",&context.state.config.data_dir)
        .env("DOMINION_WORKSPACE",lab)
        .env_remove("DOMINION_DESKTOP_PARENT");
    let output=command(context,probe,Duration::from_secs(30)).await;
    let _=std::fs::remove_file(&path);
    let output=output?;
    if !output.status.success(){return Err(format!("old planner replay failed: {}",
        String::from_utf8_lossy(&output.stderr).chars().take(200).collect::<String>()));}
    let old:Value=serde_json::from_slice(&output.stdout).map_err(|e|e.to_string())?;
    if old["source"]!="compiled_descendant_system_controller_v2" ||
        old["generatorCommit"]!=prior_commit ||
        old["historyCount"].as_u64()!=Some(history.len() as u64) {
        return Err("old compiled planner has different history or format".into());
    }
    let earlier=effective_action(&old);
    let later=effective_action(current_plan);
    let changed=earlier!=later;
    Ok(json!({"source":"matched_compiled_planner_intervention_v1",
        "status":if changed {"behavior_changed"}else{"behavior_unchanged"},
        "sameSourceRoot":true,"samePersistedHistory":true,
        "onlyBinaryIdentityFieldsRebound":true,"historyCount":history.len(),
        "previousGeneratorCommit":prior_commit,
        "previousGeneratorBinarySha256":prior_sha,
        "parentAcceptedCommit":head,
        "descendantGeneratorCommit":head,
        "descendantGeneratorBinarySha256":current_binary_sha,
        "previousAction":earlier,"descendantAction":later,
        "previousActionSha256":action_hash(&old),
        "descendantActionSha256":action_hash(current_plan),
        "qualityProven":false,
        "attribution":"compiled_planner_difference_not_quality_proof"}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matched_action_ignores_metadata_and_detects_control_choice(){
        let mut a=json!({"generatorCommit":"first","domain":"memory","focusFiles":["src/engine/memory.rs"]});
        let before=action_hash(&a);
        a["generatorCommit"]=json!("second");
        assert_eq!(before,action_hash(&a));
        a["focusFiles"]=json!(["src/engine/rsi_meta_controller.rs"]);
        assert_ne!(before,action_hash(&a));
    }
}
