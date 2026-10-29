use procshelf::{
    configuration::{Config, watch::ConfigWatch},
    processes::manager::Manager,
};
use std::time::Duration;
#[tokio::test]
async fn atomic_save_invalid_edit_and_recovery_are_applied_without_restarting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    let manager = Manager::new(
        Config::parse("", dir.path()).unwrap(),
        env!("CARGO_BIN_EXE_procshelf").into(),
    );
    let _watch = ConfigWatch::start(path.clone(), manager.clone(), String::new()).unwrap();
    let next = dir.path().join("config.tmp");
    let valid = "[processes.api]\nprogram='/bin/sh'\n";
    std::fs::write(&next, valid).unwrap();
    std::fs::rename(&next, &path).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while manager.snapshot().await.processes.is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("atomic rename must be observed");
    std::fs::write(&path, "[processes.api\nsecret='must-not-appear'").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while manager.snapshot().await.config_error.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let snapshot = manager.snapshot().await;
    assert_eq!(snapshot.processes.len(), 1);
    assert!(!snapshot.config_error.unwrap().contains("must-not-appear"));
    std::fs::write(&path, valid).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while manager.snapshot().await.config_error.is_some() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.snapshot().await.processes[0].state, "stopped");
    manager.shutdown().await;
}
