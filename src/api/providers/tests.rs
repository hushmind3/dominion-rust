    use super::*;
    use serde_json::json;

    #[test]
    fn codex_catalog_uses_real_slugs_and_no_obsolete_stub() {
        let models = parse_codex_models(&json!({"models":[
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol",
             "visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]},
            {"slug":"gpt-6.1-terra","visibility":"list"},
            {"slug":"gpt-6-sol","visibility":"list"},
            {"slug":"gpt-6-luna","visibility":"list"},
            {"slug":"hidden-model","visibility":"hide"},
            {"slug":"gpt-6.1-sol","visibility":"list"}
        ]})).unwrap();
        assert_eq!(models.len(),4);
        assert_eq!(models[0]["id"],"gpt-6.1-sol");
        assert_eq!(models[0]["reasoningEfforts"],json!(["low","high"]));
        assert_eq!(models[0]["freeStatus"],"unknown");
        assert!(!models.iter().any(|model|model["id"]=="hidden-model"));
    }

    #[test]
    fn codex_catalog_rejects_empty_or_invented_model_list() {
        assert!(parse_codex_models(&json!({"models":[]})).is_err());
        assert!(parse_codex_models(&json!({"data":[{"model":"real-model"}]})).is_ok());
        assert!(parse_codex_models(&json!({"wrong":[]})).is_err());
    }

    #[test]
    fn codex_catalog_has_no_four_model_cap_and_preserves_all_advertised_efforts() {
        let rows = (0..9).map(|index| json!({
            "id":format!("account-model-{index}"), "displayName":format!("Account model {index}"),
            "supportedReasoningEfforts":["low",{"reasoningEffort":"xhigh"},{"effort":"max"},"ultra","future_tier"],
            "defaultReasoningEffort":"xhigh"
        })).collect::<Vec<_>>();
        let models = parse_codex_models(&json!({"data":rows})).unwrap();
        assert_eq!(models.len(), 9);
        assert_eq!(models[8]["reasoningEfforts"], json!(["low","xhigh","max","ultra","future_tier"]));
        assert_eq!(models[8]["defaultReasoningEffort"], "xhigh");
    }

    #[test]
    fn account_placeholder_is_detected_only_when_unfilled() {
        assert!(needs_account_discovery(&json!({
            "baseUrl":"https://api.cloudflare.com/client/v4/accounts/ACCOUNT_ID/ai/v1"
        })));
        assert!(!needs_account_discovery(&json!({
            "baseUrl":"https://api.cloudflare.com/client/v4/accounts/ACCOUNT_ID/ai/v1",
            "accountId":"abc123"
        })));
        assert!(!needs_account_discovery(&json!({"baseUrl":"https://api.example.com/v1"})));
    }

    #[test]
    fn groq_free_plan_does_not_mark_all_catalog_models_free() {
        let base = "https://api.groq.com/openai/v1";
        for id in [
            "openai/gpt-oss-120b",
            "openai/gpt-oss-20b",
            "openai/gpt-oss-safeguard-20b",
            "qwen/qwen3.8-27b",
        ] {
            assert!(model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
        for id in ["llama-3.1-8b-instant", "unknown-model", "whisper-large-v3"] {
            assert!(!model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
    }

    #[test]
    fn cloudflare_free_plan_excludes_models_requiring_paid_billing() {
        let base = "https://api.cloudflare.com/client/v4/accounts/abc/ai/v1";
        for id in [
            "@cf/zai-org/glm-4.7-flash",
            "@cf/google/gemma-4-26b-a4b-it",
            "@cf/nvidia/nemotron-3-120b-a12b",
            "@cf/deepseek-ai/deepseek-v4.1-flash-1017",
        ] {
            assert!(model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
        for id in [
            "@cf/moonshotai/kimi-k2.6",
            "@cf/moonshotai/kimi-k2.7-code",
            "@cf/zai-org/glm-5.2",
            "@cf/zai-org/glm-5.3",
            "@cf/zai-org/glm-5.3-flash",
            "@cf/deepseek-ai/deepseek-v4-flash-0731",
            "@cf/deepseek-ai/deepseek-v4-pro-0813",
            "unrecognized-model",
        ] {
            assert!(!model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
    }

    #[test]
    fn free_models_require_local_runtime_or_pricing_evidence() {
        let local = json!({"id":"p","name":"local","baseUrl":"http://127.0.0.1:11434/v1"});
        assert!(model_is_free(&local, "openai", "http://127.0.0.1:11434/v1", &Value::Null, "x"));
        let proxy = json!({"id":"p","name":"proxy","baseUrl":"http://127.0.0.1:4100/v1"});
        assert!(!model_is_free(&proxy, "openai", "http://127.0.0.1:4100/v1", &Value::Null, "paid/model"));
        let free_api = json!({"id":"p","name":"free","integration":"freellmapi"});
        assert!(!model_is_free(&free_api, "openai", "https://api.example/v1", &Value::Null, "x"));
        let free_suffix = json!({"id":"p","name":"openrouter"});
        assert!(model_is_free(&free_suffix, "openai", "https://openrouter.ai/api/v1", &Value::Null, "meta/llama:free"));
        let zero = json!({"pricing":{"prompt":"0","completion":"0"}});
        assert!(model_is_free(&free_suffix, "openai", "https://openrouter.ai/api/v1", &zero, "meta/llama"));
        let paid = json!({"pricing":{"prompt":"0.000001","completion":"0.000002"}});
        assert!(!model_is_free(&free_suffix, "openai", "https://openrouter.ai/api/v1", &paid, "meta/llama"));
    }

    #[test]
    fn opencode_go_urls_normalize_to_official_base() {
        for input in [
            "https://opencode.ai/zen/go",
            "https://opencode.ai/zen/go/v1",
            "https://opencode.ai/inference/go",
            "https://opencode.ai/inference/go/v1",
        ] {
            assert_eq!(official_opencode_go_url(input), "https://opencode.ai/inference/go/openai/v1");
        }
        assert_eq!(
            official_opencode_go_url("https://opencode.ai/inference/go/anthropic/v1"),
            "https://opencode.ai/inference/go/anthropic/v1"
        );
    }

    #[test]
    fn opencode_go_import_only_accepts_explicit_key_locations() {
        assert_eq!(opencode_go_key_in_auth(&json!({
            "opencode-go":{"type":"api","key":"go-key"},
            "opencode":{"type":"oauth","access":"private-oauth"}
        })), Some("go-key"));
        assert!(opencode_go_key_in_auth(&json!({
            "opencode-go":{"type":"oauth","access":"private-oauth"}
        })).is_none());
    }

    #[test]
    fn opencode_import_reads_only_plain_api_keys() {
        let auth = json!({
            "openrouter":{"type":"api","key":"sk-or-real"},
            "anthropic":{"type":"oauth","access":"oauth-token","refresh":"refresh"},
            "google":{"type":"wellknown","key":"google-key"},
            "unknown-provider":{"type":"api","key":"unknown-key"}
        });
        let mut entries = opencode_api_entries(&auth);
        entries.sort();
        assert_eq!(entries.len(), 3);
        assert!(entries.contains(&("openrouter".into(), "sk-or-real".into())));
        assert!(entries.contains(&("google".into(), "google-key".into())));
        assert!(!entries.iter().any(|(provider, _)| provider == "anthropic"));
        assert_eq!(opencode_target("openrouter").unwrap().1, "https://openrouter.ai/api/v1");
        assert_eq!(opencode_target("anthropic").unwrap().2, "anthropic");
        assert!(opencode_target("unknown-provider").is_none());
        assert_eq!(provider_slug("z-ai"), "z-ai");
    }
