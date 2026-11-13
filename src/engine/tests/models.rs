use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-models-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn file_sha256_matches_known_digest() {
        let dir = std::env::temp_dir().join(format!("dominion-sha-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            file_sha256(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mlx_runtime_is_detected_only_with_a_ready_marker() {
        let (state, dir) = temp_state();
        let runtime = dir.join("mlx-runtime");
        let python = runtime.join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, b"#!/bin/sh\n").unwrap();
        assert!(mlx_runtime_python(&state).is_none());
        std::fs::write(runtime.join(".ready"), b"0.32.2").unwrap();
        assert_eq!(mlx_runtime_python(&state), Some(python));
        assert!(!mlx_installable(&state));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn managed_model_dir_rejects_traversal_and_escapes() {
        let dir = std::env::temp_dir().join(format!("dominion-model-dir-{}", uuid::Uuid::new_v4()));
        let root = dir.join("models");
        std::fs::create_dir_all(root.join("owner/repo")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        assert!(managed_model_dir(&root, "owner/repo").is_some());
        assert!(managed_model_dir(&root, "../outside").is_none());
        assert!(managed_model_dir(&root, "owner/../../outside").is_none());
        assert!(managed_model_dir(&root, "").is_none());
        assert!(managed_model_dir(&root, ".").is_none());
        #[cfg(unix)]
        {
            let link = root.join("escape");
            std::os::unix::fs::symlink(dir.join("outside"), &link).unwrap();
            assert!(managed_model_dir(&root, "escape").is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_state_tracks_progress_and_activity() {
        set_install(install_progress(
            "downloading",
            "release",
            "asset.zip",
            10,
            100,
            "asset.zip",
        ));
        let snapshot = install_snapshot();
        assert_eq!(snapshot["status"], "downloading");
        assert_eq!(snapshot["downloaded"], 10);
        assert_eq!(snapshot["total"], 100);
        assert!(install_active());
        set_install(install_progress(
            "installed",
            "release",
            "asset.zip",
            100,
            100,
            "llama-server",
        ));
        assert!(!install_active());
        assert_eq!(install_snapshot()["status"], "installed");
        set_install(Value::Null);
    }

    #[test]
    fn lists_only_gguf_files() {
        let (state, dir) = temp_state();
        let models = models_dir(&state);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("tiny.gguf"), b"x").unwrap();
        std::fs::write(models.join("notes.txt"), b"x").unwrap();
        let items = installed(&state)
            .into_iter()
            .filter(|item| !matches!(item["source"].as_str(), Some("lmstudio" | "mlx-download")))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "tiny.gguf");
        assert_eq!(items[0]["format"], "gguf");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn imports_a_local_gguf() {
        let (state, dir) = temp_state();
        let source = dir.join("source.gguf");
        std::fs::write(&source, b"model").unwrap();
        let item = import_local(&state, &source.to_string_lossy()).expect("import");
        assert_eq!(item["id"], "source.gguf");
        assert!(models_dir(&state).join("source.gguf").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_non_gguf_import() {
        let (state, dir) = temp_state();
        let source = dir.join("model.bin");
        std::fs::write(&source, b"x").unwrap();
        assert!(import_local(&state, &source.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_repo_traversal_and_scopes_download_targets() {
        assert!(safe_repo("../etc").is_err());
        assert!(safe_repo("owner/../model").is_err());
        assert!(safe_repo("owner/model").is_ok());
        let (state, dir) = temp_state();
        assert_eq!(
            target_root(&state, "gguf", "owner/model"),
            models_dir(&state).join("owner--model")
        );
        assert_eq!(
            target_root(&state, "mlx", "owner/model"),
            mlx_models_root(&state).join("owner/model")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hf_catalog_filters_are_server_side_and_preserve_sorting() {
        let url = hf_catalog_url("Qwen", 80, "gguf", "likes", Some(4), true).unwrap();
        let values = url.query_pairs().into_owned().collect::<std::collections::HashMap<_, _>>();
        assert_eq!(values.get("search").map(String::as_str), Some("Qwen"));
        assert_eq!(values.get("sort").map(String::as_str), Some("likes"));
        assert_eq!(values.get("filter").map(String::as_str), Some("gguf"));
        assert_eq!(values.get("num_parameters").map(String::as_str), Some("max:4B"));
        assert_eq!(values.get("pipeline_tag").map(String::as_str), Some("text-generation"));
        let without = hf_catalog_url("", 3, "mlx", "downloads", None, false).unwrap();
        assert!(!without.as_str().contains("num_parameters"));
        assert!(!without.as_str().contains("pipeline_tag"));
    }

    #[test]
    fn split_gguf_is_one_complete_download_and_never_a_partial_model() {
        let siblings = vec![
            json!({"rfilename":"Q4/model-00002-of-00003.gguf","size":20}),
            json!({"rfilename":"Q4/model-00001-of-00003.gguf","size":10}),
            json!({"rfilename":"Q4/model-00003-of-00003.gguf","size":30}),
            json!({"rfilename":"simple-Q8_0.gguf","size":60}),
            json!({"rfilename":"broken-00001-of-00002.gguf","size":99}),
        ];
        let options = gguf_download_options(&siblings);
        assert_eq!(options.len(), 2, "{options:?}");
        let split = options.iter().find(|o| o["shards"] == 3).unwrap();
        assert_eq!(split["bytes"], 60);
        assert_eq!(split["files"], json!([
            "Q4/model-00001-of-00003.gguf",
            "Q4/model-00002-of-00003.gguf",
            "Q4/model-00003-of-00003.gguf"
        ]));
        assert_eq!(validated_gguf_files(split["id"].as_str().unwrap(),
            split["files"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().to_string()).collect()).unwrap().len(), 3);
        assert!(validated_gguf_files("Q4/model-00001-of-00003.gguf", vec![]).is_err());
        assert!(validated_gguf_files("Q4/model-00002-of-00003.gguf",
            vec!["Q4/model-00002-of-00003.gguf".into()]).is_err());
        assert!(validated_gguf_files("simple-Q8_0.gguf",
            vec!["simple-Q8_0.gguf".into(),"another.gguf".into()]).is_err());
    }

    #[test]
    fn removing_split_gguf_cleans_up_every_shard() {
        let (state, dir) = temp_state();
        let model_dir = models_dir(&state).join("split");
        std::fs::create_dir_all(&model_dir).unwrap();
        for part in 1..=3 {
            std::fs::write(model_dir.join(format!("model-{part:05}-of-00003.gguf")), b"x").unwrap();
        }
        assert_eq!(installed(&state).iter().filter(|m| m["format"] == "gguf").count(), 1);
        remove_installed(&state, "split/model-00001-of-00003.gguf").unwrap();
        assert!(std::fs::read_dir(&model_dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn split_gguf_appears_only_when_all_shards_exist() {
        let (state, dir) = temp_state();
        let model_dir = models_dir(&state).join("split");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model-00001-of-00003.gguf"), b"a").unwrap();
        assert!(!installed(&state).iter().any(|m| m["id"] == "split/model-00001-of-00003.gguf"));
        std::fs::write(model_dir.join("model-00002-of-00003.gguf"), b"bb").unwrap();
        std::fs::write(model_dir.join("model-00003-of-00003.gguf"), b"ccc").unwrap();
        let models = installed(&state).into_iter().filter(|m| m["format"] == "gguf").collect::<Vec<_>>();
        assert_eq!(models.len(), 1, "{models:?}");
        assert_eq!(models[0]["id"], "split/model-00001-of-00003.gguf");
        assert_eq!(models[0]["bytes"], 6);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn split_gguf_import_copies_all_parts_and_rejects_incomplete_sets() {
        let (state, dir) = temp_state();
        let source_dir = dir.join("external");
        std::fs::create_dir_all(&source_dir).unwrap();
        let first = source_dir.join("model-00001-of-00003.gguf");
        std::fs::write(&first, b"a").unwrap();
        assert!(import_local(&state, &first.to_string_lossy()).is_err());
        assert!(!models_dir(&state).join("model-00001-of-00003.gguf").exists());
        for i in 2..=3 {
            std::fs::write(source_dir.join(format!("model-{i:05}-of-00003.gguf")), b"bb").unwrap();
        }
        let imported = import_local(&state, &first.to_string_lossy()).unwrap();
        assert_eq!(imported["id"], "model-00001-of-00003.gguf");
        assert_eq!(imported["bytes"], 5);
        for i in 1..=3 {
            assert!(models_dir(&state).join(format!("model-{i:05}-of-00003.gguf")).exists());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn external_split_gguf_registration_needs_every_part() {
        let (state, dir) = temp_state();
        let first = dir.join("model-00001-of-00002.gguf");
        let second = dir.join("model-00002-of-00002.gguf");
        std::fs::write(&first, b"aaa").unwrap();
        assert!(register_local(&state, &first.to_string_lossy()).is_err());
        std::fs::write(&second, b"bbbb").unwrap();
        let model = register_local(&state, &first.to_string_lossy()).unwrap();
        assert_eq!(model["shards"], 2);
        assert_eq!(model["bytes"], 7);
        assert!(register_local(&state, &second.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn replacing_a_downloaded_model_keeps_the_new_bytes() {
        let (state, dir) = temp_state();
        let target = models_dir(&state).join("old.gguf");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old").unwrap();
        let partial = partial_download_path(&target, &"a".repeat(40));
        std::fs::write(&partial, b"new").unwrap();
        promote_download_file(&partial, &target).expect("promote");
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!partial.exists());
        assert!(!target.with_extension("gguf.dominion-previous").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_promotion_does_not_delete_existing_model() {
        let (state, dir) = temp_state();
        let target = models_dir(&state).join("old.gguf");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old").unwrap();
        let partial = partial_download_path(&target, &"b".repeat(40));
        assert!(promote_download_file(&partial, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn different_snapshot_downloads_never_share_partial_bytes() {
        let target = PathBuf::from("/tmp/weights.gguf");
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let first = partial_download_path(&target, &a);
        let second = partial_download_path(&target, &b);
        assert_ne!(first, second);
        assert_eq!(first, PathBuf::from(format!("/tmp/weights.gguf.part.{a}")));
        assert_eq!(partial_download_path(&target, "main"), PathBuf::from("/tmp/weights.gguf.part"));
    }

    #[test]
    fn competing_model_revisions_cannot_share_one_partial_file() {
        let (state, dir) = temp_state();
        let previous = "a".repeat(40);
        let requested = "b".repeat(40);
        state.store.put(MODEL_KIND, &json!({
            "repo": "owner/model", "optionId": "model.gguf",
            "revision": previous, "status": "paused"
        }));
        let error = start_download_at_revision(
            state.clone(), "owner/model".into(), "model.gguf".into(),
            "gguf".into(), Vec::new(), Some(&requested)
        ).unwrap_err();
        assert!(error.contains("다른 버전"), "{error}");
        assert!(start_download_at_revision(
            state, "owner/model".into(), "model.gguf".into(),
            "gguf".into(), Vec::new(), Some("bad/revision")
        ).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_download_is_idempotent_but_different_manifest_cannot_skip_shards() {
        let (state,dir)=temp_state();
        let root=target_root(&state,"gguf","owner/model");
        let old=state.store.put(MODEL_KIND,&json!({
            "repo":"owner/model","optionId":"model.gguf","revision":"main",
            "format":"gguf","status":"paused","targetDir":root.to_string_lossy(),
            "files":[{"path":"model.gguf"}]
        })).unwrap();
        let same=start_download_at_revision(state.clone(),"owner/model".into(),
            "model.gguf".into(),"gguf".into(),Vec::new(),None).unwrap();
        assert_eq!(old["id"],same["id"]);
        assert_eq!(state.store.all(MODEL_KIND).len(),1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_mlx_selections_must_not_write_the_same_snapshot_files() {
        let (state,dir)=temp_state();
        let root=target_root(&state,"mlx","owner/model");
        state.store.put(MODEL_KIND,&json!({
            "repo":"owner/model","optionId":"all-shards","revision":"a".repeat(40),
            "format":"mlx","status":"paused","targetDir":root.to_string_lossy(),
            "files":[{"path":"config.json"},{"path":"model.safetensors"}]
        })).unwrap();
        let err=start_download_at_revision(state.clone(),"owner/model".into(),
            "another-selection".into(),"mlx".into(),
            vec!["config.json".into(),"model.safetensors".into()],
            Some(&"b".repeat(40))).unwrap_err();
        assert!(err.contains("동일한 대상 파일"),"{err}");
        assert_eq!(state.store.all(MODEL_KIND).len(),1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resume_cannot_take_files_owned_by_another_active_download() {
        let (state,root)=temp_state();
        let target=target_root(&state,"mlx","owner/model");
        let old=state.store.put(MODEL_KIND,&json!({
            "repo":"owner/model","optionId":"old-selection","status":"cancelled",
            "format":"mlx","targetDir":target.to_string_lossy(),
            "files":[{"path":"config.json"},{"path":"model.safetensors"}]
        })).unwrap();
        let conflict=state.store.put(MODEL_KIND,&json!({
            "repo":"owner/model","optionId":"active-selection","status":"paused",
            "format":"mlx","targetDir":target.to_string_lossy(),
            "files":[{"path":"model.safetensors"}]
        })).unwrap();
        assert!(resume_job(&state,old["id"].as_str().unwrap()).is_none());
        assert_eq!(state.store.get(MODEL_KIND,old["id"].as_str().unwrap()).unwrap()["status"],"cancelled");
        assert_eq!(state.store.get(MODEL_KIND,conflict["id"].as_str().unwrap()).unwrap()["status"],"paused");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn hugging_face_snapshot_requires_an_immutable_commit_hash() {
        assert!(valid_hf_revision("0123456789abcdef0123456789abcdef01234567"));
        assert!(!valid_hf_revision("main"));
        assert!(!valid_hf_revision("../main"));
        assert!(!valid_hf_revision("0123456789abcdef0123456789abcdef0123456/"));
        assert!(!valid_hf_revision("0123456789abcdef0123456789abcdef0123456z"));
    }

    #[test]
    fn resumable_download_rejects_misaligned_content_range() {
        assert!(content_range_starts_at(Some("bytes 1024-2047/4096"), 1024));
        assert!(content_range_starts_at(Some("bytes 1024-2047/*"), 1024));
        assert!(!content_range_starts_at(Some("bytes 0-1023/4096"), 1024));
        assert!(!content_range_starts_at(Some("bytes 1024-2047/2047"), 1024));
        assert!(!content_range_starts_at(Some("bytes 1024-1023/4096"), 1024));
        assert!(!content_range_starts_at(None, 1024));
    }

    #[test]
    fn mlx_option_requires_config_and_keeps_safetensors() {
        let siblings = vec![
            json!({"rfilename":"config.json","size":100}),
            json!({"rfilename":"model.safetensors","size":1000}),
            json!({"rfilename":"tokenizer.json","size":50}),
            json!({"rfilename":"README.md","size":10}),
            json!({"rfilename":"original/model.safetensors","size":1000}),
        ];
        let option = mlx_download_files(&siblings).expect("mlx option");
        assert_eq!(option["format"], "mlx");
        let files = option["files"].as_array().unwrap();
        assert!(files.iter().any(|file| file == "config.json"));
        assert!(files.iter().any(|file| file == "model.safetensors"));
        assert!(files.iter().any(|file| file == "tokenizer.json"));
        assert!(!files.iter().any(|file| file == "README.md"));
        assert!(!files.iter().any(|file| file == "original/model.safetensors"));
        assert_eq!(option["shards"], 1);
    }
