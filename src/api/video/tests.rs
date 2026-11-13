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
        let data = std::fs::read(video::directory(&s, &id).unwrap().join("output.mp4")).unwrap();
        assert_eq!(&data[4..8], b"ftyp");
        assert!(
            video::output_path(&s, &id).is_err(),
            "unfinished job must not be served"
        );
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }
