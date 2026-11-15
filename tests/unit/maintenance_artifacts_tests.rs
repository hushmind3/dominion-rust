use super::*;
use crate::{config::Config,db::Store,vault::Vault};
use serde_json::json;

fn fixture()->(AppState,PathBuf){
    let root=std::env::temp_dir().join(format!("dominion-prune-test-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let config=Config {port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let store=Store::open(&config.db_path()).unwrap();
    let state=AppState::new(config.clone(),store,Vault::open(&config.key_path()));
    (state,root)
}
fn far_future()->SystemTime {
    SystemTime::now()+Duration::from_secs(5*24*60*60)
}
fn job(state:&AppState,id:&str,kind:&str,status:&str){
    state.store.put("job",&json!({"id":id,"kind":kind,"status":status,
        "finishedAt":"2026-11-01T00:00:00Z"})).unwrap();
}
#[test]
fn successful_videos_only_lose_render_intermediates_not_their_master_or_custom_files(){
    let (state,root)=fixture();
    let id=uuid::Uuid::new_v4().to_string();
    let folder=root.join("video").join(&id);
    let shot=folder.join("shot-01");
    std::fs::create_dir_all(&shot).unwrap();
    std::fs::write(folder.join("output.mp4"),vec![1;400]).unwrap();
    std::fs::write(folder.join("output.part.mp4"),vec![1;100]).unwrap();
    std::fs::write(folder.join("shots.ffconcat"),"old concat").unwrap();
    std::fs::write(shot.join("output.mp4"),vec![1;600]).unwrap();
    std::fs::write(shot.join("custom-notes.txt"),"keep me").unwrap();
    job(&state,&id,"video","completed");
    let report=sweep_at(&state,far_future());
    assert!(report.files>=3);
    assert!(folder.join("output.mp4").exists());
    assert!(!folder.join("output.part.mp4").exists());
    assert!(!shot.join("output.mp4").exists());
    assert!(shot.join("custom-notes.txt").exists());
    assert!(!folder.join("shots.ffconcat").exists());
    let _=std::fs::remove_dir_all(root);
}
#[test]
fn active_and_needs_review_videos_keep_their_recovery_shots(){
    let (state,root)=fixture();
    for status in ["running","queued","needs_review"] {
        let id=uuid::Uuid::new_v4().to_string();
        let folder=root.join("video").join(&id).join("shot-01");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("output.mp4"),vec![1;400]).unwrap();
        job(&state,&id,"video",status);
    }
    let report=sweep_at(&state,far_future());
    assert_eq!(report.files,0);
    assert_eq!(std::fs::read_dir(root.join("video")).unwrap().count(),3);
    let _=std::fs::remove_dir_all(root);
}
#[test]
fn finished_rsi_build_caches_are_reclaimed_but_live_and_evidence_are_preserved(){
    let (state,root)=fixture();
    for (sub,kind) in [("rsi-build/generator","rsi_evolve"),("rsi-paired-build","rsi_compare")] {
        for status in ["completed","running","needs_review"] {
            let id=uuid::Uuid::new_v4().to_string();
            let dir=root.join(sub).join(&id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("libdummy.rlib"),vec![2;200]).unwrap();
            job(&state,&id,kind,status);
            let proof=root.join("rsi-paired").join(&id);
            std::fs::create_dir_all(&proof).unwrap();
            std::fs::write(proof.join("report.json"),"proof").unwrap();
            // Validation happens after the entire fixture has been populated.
        }
    }
    let report=sweep_at(&state,far_future());
    assert_eq!(report.directories,2);
    for (sub,kind) in [("rsi-build/generator","rsi_evolve"),("rsi-paired-build","rsi_compare")]{
        for entry in std::fs::read_dir(root.join(sub)).unwrap().flatten(){
            let id=entry.file_name().to_string_lossy().to_string();
            assert_ne!(state.store.get("job",&id).unwrap()["status"],"completed");
            assert_eq!(state.store.get("job",&id).unwrap()["kind"],kind);
        }
    }
    assert_eq!(std::fs::read_dir(root.join("rsi-paired")).unwrap().count(),6);
    let _=std::fs::remove_dir_all(root);
}
#[test]
fn cancelled_downloads_remove_only_matching_partial_files_without_installed_model_loss(){
    let (state,root)=fixture();
    let model=root.join("models").join("owner--repo");
    std::fs::create_dir_all(&model).unwrap();
    let good=model.join("model.gguf");
    let part=model.join("model.gguf.part");
    std::fs::write(&good,vec![1;700]).unwrap();
    std::fs::write(&part,vec![3;500]).unwrap();
    state.store.put("model_job",&json!({
        "id":"cancelled-download","targetDir":model.to_string_lossy(),
        "status":"cancelled","revision":"main",
        "finishedAt":"2026-11-01T00:00:00Z",
        "files":[{"path":"model.gguf"},{"path":"../../outside.txt"}]
    })).unwrap();
    let report=sweep_at(&state,far_future());
    assert_eq!(report.files,1);
    assert!(!part.exists());
    assert!(good.exists());
    assert_eq!(std::fs::metadata(good).unwrap().len(),700);
    let _=std::fs::remove_dir_all(root);
}
#[test]
fn paused_download_blocks_cancelled_part_reclamation_until_no_owner_remains(){
    let (state,root)=fixture();
    let model=root.join("models").join("owner--repo");
    std::fs::create_dir_all(&model).unwrap();
    let part=model.join("model.gguf.part");
    std::fs::write(&part,vec![3;500]).unwrap();
    state.store.put("model_job",&json!({"id":"old-cancel","targetDir":model.to_string_lossy(),
        "status":"cancelled","revision":"main","finishedAt":"2026-11-01T00:00:00Z",
        "files":[{"path":"model.gguf"}]})).unwrap();
    state.store.put("model_job",&json!({"id":"new-paused","targetDir":model.to_string_lossy(),
        "status":"paused","revision":"main","files":[{"path":"model.gguf"}]})).unwrap();
    sweep_at(&state,far_future());
    assert!(part.exists());
    let _=std::fs::remove_dir_all(root);
}
#[test]
fn only_app_owned_old_plain_backup_temp_is_removed(){
    let (state,root)=fixture();
    let backups=root.join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    let abandoned=backups.join(".backup-123.tmp");
    let keep=backups.join("dominion-20261101-000000.sqlite.enc");
    let unrelated=backups.join("owner.tmp");
    for path in [&abandoned,&keep,&unrelated] {std::fs::write(path,"data").unwrap();}
    let report=sweep_at(&state,far_future());
    assert_eq!(report.files,1);
    assert!(!abandoned.exists());
    assert!(keep.exists());
    assert!(unrelated.exists());
    let _=std::fs::remove_dir_all(root);
}
