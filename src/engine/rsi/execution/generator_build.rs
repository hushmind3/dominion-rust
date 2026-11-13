//! Compilation of a candidate descendant and its executable provenance.
//! The campaign owns budget, state persistence and candidate adoption;
//! this module only builds one source revision and verifies that the actual
//! executable identifies itself as the exact requested revision.
use super::*;

pub(super) async fn ensure_compiled_generator(
    context:&JobContext,lab:&Path,target:&Path,binary:&Path,
    record:&mut Value,head:&str,remaining:u64,built_commit:&mut String,
)->Result<(),String>{
    if built_commit.as_str() != head {
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
            *built_commit = head.to_owned();
        }
    Ok(())
}
