//! Compiled descendant generation, revision identity and one-step previous
//! binary snapshot. The orchestration loop does not own compiler semantics.
use super::*;

pub(super) struct GeneratorBuild<'a> {
    pub context:&'a JobContext,
    pub record:&'a mut Value,
    pub head:&'a str,
    pub built_commit:&'a mut String,
    pub binary:&'a Path,
    pub control_binary:&'a Path,
    pub lab:&'a Path,
    pub target:&'a Path,
    pub remaining:u64,
}

/// Every accepted source revision becomes the ACTUAL executable generator for
/// the next descendant. The prior binary is retained only for a source-matched
/// controlled comparison and is never promoted as a candidate.
pub(super) async fn ensure(run:GeneratorBuild<'_>)->Result<(),String> {
    let GeneratorBuild {
        context,record,head,built_commit,binary,control_binary,
        lab,target,remaining
    }=run;
        if built_commit.as_str() != head {
            if !built_commit.is_empty() && binary.is_file() {
                std::fs::copy(&binary,&control_binary)
                    .map_err(|e|format!("이전 개선기 대조 바이너리 보존 실패: {e}"))?;
                record["controlGeneratorCommit"]=json!(built_commit.as_str());
                record["controlGeneratorBinarySha256"]=json!(format!("{:x}",
                    Sha256::digest(std::fs::read(&control_binary)
                        .map_err(|e|e.to_string())?)));
            }else{
                record["controlGeneratorCommit"]=Value::Null;
                record["controlGeneratorBinarySha256"]=Value::Null;
            }
            record["phase"] = json!("building_generator");
            save(context, record)?;
            let mut build = Command::new(rsi_worktree::cargo_executable());
            build
                .current_dir(&lab)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "--bin",
                    "dominion-rust",
                    "--target-dir",
                ])
                .arg(&target)
                .env("DOMINION_RSI_BUILD_COMMIT", &head);
            let build_timeout = if remaining == u64::MAX {
                3600
            } else {
                remaining.min(600)
            };
            let output = command(context, build, Duration::from_secs(build_timeout)).await?;
            if !output.status.success() {
                return Err(format!(
                    "차세대 개선기 빌드 실패: {}",
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                ));
            }
            let mut identity = Command::new(&binary);
            identity.arg("--rsi-worker-identity");
            let output = command(context, identity, Duration::from_secs(10)).await?;
            if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != head {
                return Err("실제 개선기 바이너리와 세대 커밋이 다릅니다.".into());
            }
            record["generatorBinarySha256"] = json!(format!(
                "{:x}",
                Sha256::digest(std::fs::read(&binary).map_err(|e| e.to_string())?)
            ));
            record["actualGeneratorCommit"] = json!(head);
            *built_commit = head.to_string();
        }
    Ok(())
}
