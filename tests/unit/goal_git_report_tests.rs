use super::*;
fn run(root:&Path,args:&[&str]){
    let status=Command::new("git").arg("-C").arg(root).args(args).status().unwrap();
    assert!(status.success(),"git {:?} failed",args);
}
#[test]
fn frozen_git_baseline_reports_real_multi_file_changes(){
    let root=std::env::temp_dir().join(format!("dominion-git-goal-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    run(&root,&["init","-q"]);
    std::fs::write(root.join("a.rs"),"fn original() {}\n").unwrap();
    run(&root,&["add","a.rs"]);
    run(&root,&["-c","user.email=dominion@example.invalid","-c","user.name=DOMINION","commit","-qm","initial"]);
    let before=snapshot(&root);
    assert!(before["head"].as_str().unwrap().len()==40);
    std::fs::write(root.join("a.rs"),"fn changed() {}\n").unwrap();
    std::fs::write(root.join("new.rs"),"fn added() {}\n").unwrap();
    let after=report(&root,&before);
    assert_eq!(after["baselineHead"],before["head"]);
    assert!(after["changedFiles"].as_array().unwrap().iter().any(|v|v["path"]=="a.rs"));
    assert!(after["diffPreview"].as_str().unwrap().contains("fn changed()"));
    assert!(after["currentStatus"].as_str().unwrap().contains("new.rs"));
    assert!(snapshot(root.parent().unwrap()).is_null(),
        "must not silently inspect the parent of the configured workspace");
    std::fs::remove_dir_all(root).unwrap();
}
