use super::*;
use crate::{config::Config,db::Store,state::AppState,vault::Vault};

#[test]
fn segmentation_requires_consent_and_bounded_six_clips(){
    assert_eq!(split_duration(22,10).unwrap(),vec![10,6,6]);
    assert_eq!(split_duration(60,10).unwrap(),vec![10,10,10,10,10,10]);
    assert_eq!(split_duration(38,20).unwrap(),vec![20,18]);
    assert!(split_duration(62,10).is_err());
    assert!(split_duration(8,10).is_err());
    assert_eq!(max_clip("ltx-2-5-pro","1080p",24).unwrap(),10);
    assert_eq!(max_clip("ltx-2-5-fast","720p",24).unwrap(),20);
    let cfg=json!({"videoModel":"ltx-2-5-pro","videoQuality":"1080p"});
    let body=json!({"duration":22,"brief":"Ocean lighthouse",
        "videoModel":"ltx-2-5-pro","videoQuality":"1080p","fps":24,
        "aspectRatio":"16:9"});
    assert!(spec(&body,&cfg).is_err()); // no consent or FFmpeg
}
#[test]
fn sha_receipt_does_not_reuse_unknown_or_mutated_paid_clip(){
    let dir=std::env::temp_dir().join(format!("film-receipt-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(saved_receipt(&dir).unwrap().is_none());
    let file=dir.join("output.mp4");
    std::fs::write(&file,b"this is a stand-in file not an actual billed video").unwrap();
    assert!(saved_receipt(&dir).is_err());
    let hash=file_sha(&file).unwrap();
    let receipt=json!({"quality":{"status":"technical_pass"},"outputSha256":hash});
    persist_receipt(&dir,&receipt).unwrap();
    assert!(saved_receipt(&dir).unwrap().is_some());
    std::fs::write(&file,b"changed content").unwrap();
    assert!(saved_receipt(&dir).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn retake_consent_counts_only_missing_and_target_shots(){
    let home=std::env::temp_dir().join(format!("film-retake-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let cfg=Config{port:0,data_dir:home.clone(),web_dir:home.join("web"),
        workspace:home.clone()};
    let store=Store::open(&cfg.db_path()).unwrap();
    let state=AppState::new(cfg,store,Vault::open(&home.join("secret.key")));
    let clips=vec![
        json!({"duration":10,"render":{"prompt":"scene zero"}}),
        json!({"duration":6,"render":{"prompt":"scene one"}}),
        json!({"duration":6,"render":{"prompt":"scene two"}})];
    let job=state.store.put("job",&json!({"kind":"video","status":"needs_review",
        "input":{"engine":"ltx","ltxFilm":true,
            "render":{"film":{"clips":clips}}}})).unwrap();
    let id=job["id"].as_str().unwrap();
    let root=super::super::directory(&state,id).unwrap();
    let good=root.join("clip-00");
    let bad=root.join("clip-01");
    std::fs::create_dir_all(&good).unwrap();
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(good.join("output.mp4"),vec![b'a';2048]).unwrap();
    let good_sha=file_sha(&good.join("output.mp4")).unwrap();
    persist_receipt(&good,&json!({"outputSha256":good_sha,
        "quality":{"status":"technical_pass"}})).unwrap();
    std::fs::write(bad.join("output.mp4"),vec![b'b';2048]).unwrap();
    let digest=file_sha(&bad.join("output.mp4")).unwrap();
    let manifest=scenes(&state,&job).unwrap();
    assert_eq!(manifest["clips"][1]["canRetake"],true);
    assert_eq!(manifest["clips"][1]["expectedPaidCalls"],2);
    assert!(retake_input(&state,&job,1,&digest,Some("new shot"),1).is_err());
    let planned=retake_input(&state,&job,1,&digest,Some("new shot"),2).unwrap();
    assert_eq!(planned["retake"]["newPaidCalls"],2);
    assert_eq!(planned["render"]["film"]["clips"][1]["render"]["prompt"],"new shot");
    assert_eq!(planned["retake"]["sourceJobId"],id);
    assert!(retake_input(&state,&job,1,&"0".repeat(64),Some("new shot"),2).is_err());
    drop(state);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn retake_quote_counts_only_selected_and_unrendered_shots(){
    let home=std::env::temp_dir().join(format!("film-priced-retake-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let cfg=Config{port:0,data_dir:home.clone(),web_dir:home.join("web"),
        workspace:home.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
        Vault::open(&cfg.key_path()));
    let clips=vec![
        json!({"duration":10,"render":{"prompt":"scene zero"}}),
        json!({"duration":6,"render":{"prompt":"scene one"}}),
        json!({"duration":6,"render":{"prompt":"scene two"}})
    ];
    let job=state.store.put("job",&json!({"kind":"video","status":"needs_review",
        "input":{"engine":"ltx","ltxFilm":true,"videoModel":"ltx-2-5-fast",
            "videoQuality":"1080p","generateAudio":true,
            "render":{"film":{"clips":clips}}}})).unwrap();
    let id=job["id"].as_str().unwrap();
    let root=super::super::directory(&state,id).unwrap();
    for index in 0..2 {
        let dir=root.join(format!("clip-{index:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("output.mp4"),vec![b'a'+index as u8;2048]).unwrap();
        let sha=file_sha(&dir.join("output.mp4")).unwrap();
        persist_receipt(&dir,&json!({"outputSha256":sha,
            "quality":{"status":"technical_pass"}})).unwrap();
    }
    let manifest=super::super::film_scenes(&state,id).unwrap();
    let target=&manifest["clips"][0];
    assert_eq!(target["expectedPaidCalls"],2);
    assert_eq!(target["retakeQuote"]["billableSeconds"],16); // 10 + 6, not 22
    assert_eq!(target["retakeQuote"]["totalUsdCents"],208);
    assert_eq!(target["retakeQuote"]["referenceKrw"],2779);
    let original=target["outputSha256"].as_str().unwrap();
    let mut request=json!({"index":0,"expectedSha256":original,
        "confirmedPaidCallCount":2,"confirmNewPaidCalls":true,
        "approvedPaidQuote":true,
        "maxUsdCents":207,"quoteDigest":target["retakeQuote"]["quoteDigest"]});
    assert!(super::super::enqueue_retake(&state,id,&request).is_err());
    request["maxUsdCents"]=json!(208);
    let spawned=super::super::enqueue_retake(&state,id,&request).unwrap();
    let raw=state.store.get("job",spawned["id"].as_str().unwrap()).unwrap();
    assert_eq!(raw["input"]["paidQuote"]["billableSeconds"],16);
    assert_eq!(raw["input"]["paidApprovalMaxUsdCents"],208);
    assert_ne!(raw["id"],job["id"]);
    // Even a missing MP4 cannot be rebought while the original provider
    // submission is unresolved and could still produce a charged result.
    state.store.patch_if("job",id,"status",&json!("needs_review"),
        &json!({"videoState":{"inFlight":true,"remoteJobId":"pending-remote"}}));
    let blocked=super::super::film_scenes(&state,id).unwrap();
    assert_eq!(blocked["unknownEffect"],true);
    assert_eq!(blocked["clips"][0]["canRetake"],false);
    assert!(super::super::enqueue_retake(&state,id,&request).is_err());
    let _=std::fs::remove_dir_all(home);
}
