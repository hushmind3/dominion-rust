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
