        use super::install::*;
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
