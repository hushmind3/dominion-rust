use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let branch =
        git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .map(|status| (!status.is_empty()).to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=DOMINION_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=DOMINION_BUILD_BRANCH={branch}");
    println!("cargo:rustc-env=DOMINION_BUILD_DIRTY={dirty}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    if branch != "HEAD" && branch != "unknown" {
        println!("cargo:rerun-if-changed=.git/refs/heads/{branch}");
    }
}
