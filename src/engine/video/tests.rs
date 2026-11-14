//! Video boundary tests. Not included in production builds.
use super::*;
    #[test]
    fn frozen_ffmpeg_path_wins_over_current_settings() {
        let dir = std::env::temp_dir().join(format!("dominion-ffmpeg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("ffmpeg-custom");
        std::fs::write(&fake, b"binary").unwrap();
        let cfg = json!({"ffmpegPath": fake.to_string_lossy()});
        assert_eq!(ffmpeg_from_config(&cfg).as_deref(), Some(fake.as_path()));
        assert!(ffmpeg_from_config(&json!({"ffmpegPath":"/definitely/missing/ffmpeg"})).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn native_cards_render_unicode_and_escape_markup() {
        let dir =
            std::env::temp_dir().join(format!("dominion-video-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        make_cards(
            &dir,
            "테스트 <제목>",
            &["한글 자막 & 실제 프레임".into()],
            "9:16",
        )
        .unwrap();
        let bytes = std::fs::read(dir.join("card-00.png")).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(bytes.len() > 10000);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn template_substitution_never_changes_json_structure() {
        let mut value = json!({"1":{"inputs":{"text":"{{prompt}}","seconds":"{{duration}}"}}});
        substitute(&mut value, &json!({"prompt":"\"},evil:{","duration":15}));
        assert_eq!(value["1"]["inputs"]["text"], "\"},evil:{");
        assert_eq!(value["1"]["inputs"]["seconds"], 15);
        assert_eq!(value.as_object().unwrap().len(), 1);
    }

    #[test]
    fn paid_worker_checks_quote_again_before_any_remote_submission(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("paid-worker-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let mut input=json!({"engine":"ltx","ltxFilm":false,
            "videoModel":"ltx-2-5-fast","videoQuality":"1080p",
            "duration":8,"fps":24,"aspectRatio":"16:9",
            "generateAudio":true});
        let quote=paid_quote(&state,&input).unwrap();
        assert!(authorize_paid_execution(&state,&input).is_err());
        input["paidQuote"]=quote.clone();
        input["paidApprovalMaxUsdCents"]=quote["totalUsdCents"].clone();
        assert!(authorize_paid_execution(&state,&input).is_ok());
        let mut tampered=input.clone();
        tampered["duration"]=json!(10);
        assert!(authorize_paid_execution(&state,&tampered).is_err());
        tampered=input.clone();
        tampered["paidApprovalMaxUsdCents"]=json!(1);
        assert!(authorize_paid_execution(&state,&tampered).is_err());
        tampered=input.clone();
        tampered["paidQuote"]["quoteDigest"]=json!("false-approval");
        assert!(authorize_paid_execution(&state,&tampered).is_err());
        assert!(authorize_paid_execution(&state,&json!({"engine":"native"})).is_ok());
        let _=std::fs::remove_dir_all(dir);
    }
