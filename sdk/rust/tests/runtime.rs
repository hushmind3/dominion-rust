use dominion_sdk::{DominionRuntime,RuntimeConfig,DominionError};
use std::time::Duration;

#[tokio::test]
async fn runtime_host_rejects_missing_binary_without_starting_any_server(){
    let folder=std::env::temp_dir().join(format!("dominion-sdk-runtime-{}",
        uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let config=RuntimeConfig::new(folder.join("not-an-executable"),
        &folder,folder.join("state"));
    let error=DominionRuntime::spawn(config).await.unwrap_err();
    assert!(matches!(error,DominionError::Configuration(_)));
    assert!(!folder.join("state").exists());
    std::fs::remove_dir_all(folder).unwrap();
}

/// With the actual binary available, run this to prove process ownership,
/// readiness identity and graceful teardown. No provider or fake model needed.
#[tokio::test]
#[ignore]
async fn real_native_engine_spawns_and_exits_with_exact_identity(){
    let Ok(binary)=std::env::var("DOMINION_RUST_BINARY") else{return};
    let folder=std::env::temp_dir().join(format!("dominion-runtime-live-{}",
        uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let mut config=RuntimeConfig::new(binary,&folder,folder.join("state"));
    config.startup_timeout=Duration::from_secs(45);
    let runtime=DominionRuntime::spawn(config).await.unwrap();
    let health=runtime.client().health().await.unwrap();
    assert_eq!(health["runtime"],"rust");
    assert!(!runtime.instance_id().is_empty());
    runtime.shutdown().await.unwrap();
    std::fs::remove_dir_all(folder).unwrap();
}
