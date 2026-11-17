//! A source checkout is observed once at runtime; release builds can embed
//! immutable identity through explicit DOMINION_RELEASE_* environment variables.
use std::process::Command;
use std::sync::OnceLock;

pub struct Identity {
    pub commit: String,
    pub branch: String,
    pub dirty: Option<bool>,
    pub source: &'static str,
}

fn git(args: &[&str]) -> Option<String> {
    let output=Command::new("git").arg("-C").arg(env!("CARGO_MANIFEST_DIR"))
        .args(args).output().ok()?;
    if !output.status.success(){return None;}
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn running() -> &'static Identity {
    static BUILD:OnceLock<Identity>=OnceLock::new();
    BUILD.get_or_init(||{
        if let Some(commit)=option_env!("DOMINION_BUILD_COMMIT").filter(|v|!v.is_empty()){
            return Identity {
                commit:commit.to_string(),
                branch:option_env!("DOMINION_BUILD_BRANCH").unwrap_or("unknown").into(),
                dirty:option_env!("DOMINION_BUILD_DIRTY").and_then(|v|v.parse::<bool>().ok()),
                source:"embedded-release",
            };
        }
        let commit=git(&["rev-parse","HEAD"]).filter(|s|!s.is_empty());
        let branch=git(&["rev-parse","--abbrev-ref","HEAD"]).filter(|s|!s.is_empty());
        let dirty=git(&["status","--porcelain","--untracked-files=no"])
            .map(|output|!output.is_empty());
        let source=if commit.is_some(){"source-checkout"}else{"unavailable"};
        Identity {
            commit:commit.unwrap_or_else(||"unknown".into()),
            branch:branch.unwrap_or_else(||"unknown".into()),
            dirty,source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_source_is_explicit() {
        let identity=running();
        assert!(!identity.commit.is_empty());
        assert!(matches!(identity.source,"embedded-release"|"source-checkout"|"unavailable"));
    }
}
