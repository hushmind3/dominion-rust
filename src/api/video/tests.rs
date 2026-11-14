//! Video boundary tests. Not included in production builds.
use super::*;
    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-video-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir,
        };
        AppState::new(
            cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()),
        )
    }
    #[test]
    fn enqueue_is_durable_and_never_publishes_implicitly() {
        let s = state();
        let result = video::enqueue(
            &s,
            &json!({"brief":"테스트 영상","engine":"native","duration":3,"publish":true}),
        );
        if video::ffmpeg(&s).is_some() {
            let j = result.unwrap();
            let raw = s.store.get("job", j["id"].as_str().unwrap()).unwrap();
            assert_eq!(raw["input"]["publish"], false);
            assert_eq!(raw["status"], "queued");
            assert!(outputs(&s).is_empty());
        }
        assert!(video::enqueue(&s, &json!({"brief":"x","duration":900})).is_err());
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }
    #[tokio::test]
    async fn native_pipeline_produces_a_real_playable_container() {
        let s = state();
        if video::ffmpeg(&s).is_none() {
            return;
        }
        let j = video::enqueue(
            &s,
            &json!({"brief":"실제 영상 테스트. 한글 프레임 확인","duration":3}),
        )
        .unwrap();
        let id = j["id"].as_str().unwrap().to_string();
        let job = s.store.get("job", &id).unwrap();
        let ctx = crate::engine::jobs::JobContext::new(s.clone(), id.clone());
        let result = video::run(&ctx, &job).await.unwrap();
        assert!(result["bytes"].as_u64().unwrap() > 1000);
        // Actual FFmpeg-decoded source content, not just MP4 magic bytes.
        assert_eq!(result["visualSignals"]["status"],"sampled",
            "generated native movie must be decoded for technical visuals: {result}");
        assert_eq!(result["visualSignals"]["semanticQualityVerified"],false);
        let data = std::fs::read(video::directory(&s, &id).unwrap().join("output.mp4")).unwrap();
        assert_eq!(&data[4..8], b"ftyp");
        assert!(
            video::output_path(&s, &id).is_err(),
            "unfinished job must not be served"
        );
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }

    #[test]
    fn paid_ltx_enqueue_fails_closed_until_server_quote_is_approved() {
        let s=state();
        s.save_secret("video:ltx:api","fixture-only-do-not-call");
        let body=json!({"engine":"ltx","brief":"A continuous coastal film",
            "videoModel":"ltx-2-5-fast","videoQuality":"1080p",
            "duration":8,"aspectRatio":"16:9","generateAudio":true,"fps":24});
        let quote=video::paid_quote(&s,&body).unwrap();
        assert_eq!(quote["paidCalls"],1);
        assert_eq!(quote["totalUsdCents"],104);
        assert_eq!(quote["referenceKrw"],1390);
        assert_eq!(s.store.count("job"),0);
        assert!(video::enqueue(&s,&body).is_err());
        let mut approved=body.clone();
        approved["approvedPaidQuote"]=json!(true);
        approved["maxUsdCents"]=json!(103);
        approved["quoteDigest"]=quote["quoteDigest"].clone();
        assert!(video::enqueue(&s,&approved).is_err());
        approved["maxUsdCents"]=quote["totalUsdCents"].clone();
        approved["quoteDigest"]=json!("different-settings-or-price");
        assert!(video::enqueue(&s,&approved).is_err());
        approved["quoteDigest"]=quote["quoteDigest"].clone();
        let queued=video::enqueue(&s,&approved).unwrap();
        let job=s.store.get("job",queued["id"].as_str().unwrap()).unwrap();
        assert_eq!(job["input"]["paidQuote"]["totalUsdCents"],104);
        assert_eq!(job["input"]["paidApprovalMaxUsdCents"],104);
        assert_eq!(job["status"],"queued");
        let _=std::fs::remove_dir_all(s.config.data_dir.clone());
    }

    #[test]
    fn ltx_quote_is_bound_to_model_audio_resolution_and_duration() {
        let s=state();
        let body=json!({"engine":"ltx","videoModel":"ltx-2-5-pro",
            "videoQuality":"720p","duration":10,"fps":24,
            "aspectRatio":"9:16","generateAudio":true});
        let original=video::paid_quote(&s,&body).unwrap();
        assert_eq!(original["totalUsdCents"],120);
        for (key,value) in [("videoModel",json!("ltx-2-5-fast")),
            ("videoQuality",json!("1080p")),
            ("generateAudio",json!(false)),("duration",json!(8))] {
            let mut changed=body.clone();changed[key]=value;
            let new_quote=video::paid_quote(&s,&changed).unwrap();
            assert_ne!(original["quoteDigest"],new_quote["quoteDigest"],"changed {key}");
        }
        let _=std::fs::remove_dir_all(s.config.data_dir.clone());
    }
