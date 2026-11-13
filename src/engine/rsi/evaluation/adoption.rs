//! evaluation adoption: compiled descendant RSI responsibility.
use super::*;

pub(super) fn proof_goal_reached(state:&AppState,input:&Value,record:&Value)->bool {
    // Stopping and displaying completion must use the EXACT same verified
    // model-authored A→B→A method closure. Three adopted patches alone never
    // end a campaign with a successful RSI verdict.
    input["rsiStopAfterProof"]==true
        && crate::engine::rsi::campaign_verdict(state,record)["actualCompletion"]==true
}

/// Empty strings are not model errors; a research-only generation sets the
/// field to "" and must not abort the whole campaign.
pub(super) fn generation_model_error(current: &Value) -> Option<&str> {
    current["result"]["rsi"]["modelError"]
        .as_str()
        .filter(|error| !error.trim().is_empty())
}

pub(crate) async fn adopt(
    context: &JobContext,
    lab: &Path,
    candidate: &Value,
    base: &str,
) -> Result<String, String> {
    if candidate["status"] != "verified"
        || candidate["evaluation"]["metrics"]["eligible"] != true
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || candidate["baseCommit"] != base
    {
        return Err("독립 평가를 통과한 현재 세대 후보만 실험 계보에 채택합니다.".into());
    }
    if git(context, lab, &["rev-parse", "HEAD"]).await? != base
        || !git(context, lab, &["status", "--porcelain"])
            .await?
            .is_empty()
    {
        return Err("실험 기준 소스가 변경됐습니다.".into());
    }
    let patch = candidate["patch"].as_str().ok_or("패치 없음")?;
    if format!("{:x}", Sha256::digest(patch.as_bytes()))
        != candidate["patchSha256"].as_str().unwrap_or("")
    {
        return Err("채택 패치 해시 불일치".into());
    }
    let file = lab
        .parent()
        .ok_or("실험 경로 오류")?
        .join(format!("{}.patch", uuid::Uuid::new_v4()));
    std::fs::write(&file, patch).map_err(|e| e.to_string())?;
    let applied = git(
        context,
        lab,
        &["apply", file.to_str().ok_or("패치 경로 오류")?],
    )
    .await;
    let _ = std::fs::remove_file(file);
    applied?;
    git(context, lab, &["add", "--all"]).await?;
    git(
        context,
        lab,
        &[
            "-c",
            "user.name=DOMINION experiment",
            "-c",
            "user.email=rsi@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--no-verify",
            "-m",
            "RSI: adopt independently measured experimental candidate",
        ],
    )
    .await?;
    git(context, lab, &["rev-parse", "HEAD"]).await
}

#[cfg(test)]
mod proof_goal_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    #[test]
    fn even_opted_in_campaign_cannot_stop_on_three_claimed_adoptions(){
        let root=std::env::temp_dir().join(
            format!("rsi-runtime-proof-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let claimed=json!({"id":"three-native", "acceptedStreak":3,
            "maxAcceptedStreak":3,"capabilityDemonstrated":true,
            "actualCompletion":true,"history":[
                {"accepted":true},{"accepted":true},{"accepted":true}]});
        assert!(!proof_goal_reached(&state,&json!({"rsiStopAfterProof":true}),&claimed));
        assert_eq!(crate::engine::rsi::campaign_verdict(&state,&claimed)["actualCompletion"],false);
        assert!(!proof_goal_reached(&state,&json!({"rsiStopAfterProof":false}),&claimed));
        let _=std::fs::remove_dir_all(root);
    }
}
