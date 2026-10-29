use procshelf::{
    configuration::Config,
    processes::manager::{Action, Manager, ProcessView},
};
use std::{path::Path, time::Duration};
fn config(auto: bool, extra: &str) -> Config {
    Config::parse(&format!("[processes.api]\nprogram = '/bin/sh'\nargs = ['-c', 'echo ready; sleep 60']\nautostart = {auto}\nstop_timeout_seconds = 0\n{extra}"), Path::new("/tmp")).unwrap()
}
async fn state(manager: &Manager, state: &str) -> ProcessView {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(p) = manager
                .snapshot()
                .await
                .processes
                .into_iter()
                .find(|p| p.state == state)
            {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("expected process state")
}
async fn action(manager: &Manager, action: Action) {
    let snapshot = manager.snapshot().await;
    manager
        .action(
            "api",
            action,
            snapshot.processes[0].run_id,
            snapshot.config_generation,
        )
        .await
        .unwrap();
}
#[tokio::test]
async fn autostart_manual_stop_reload_and_restart_follow_policy() {
    let m = Manager::new(config(true, ""), env!("CARGO_BIN_EXE_procshelf").into());
    let first = state(&m, "running").await;
    let snap = m.snapshot().await;
    assert!(
        m.action("api", Action::Start, first.run_id, snap.config_generation)
            .await
            .is_err()
    );
    action(&m, Action::Stop).await;
    state(&m, "exited").await;
    m.apply(config(true, "name = 'Renamed'")).await;
    assert_eq!(m.snapshot().await.processes[0].state, "exited");
    action(&m, Action::Start).await;
    let second = state(&m, "running").await;
    assert_ne!(first.run_id, second.run_id);
    let generation = m.snapshot().await.config_generation;
    assert!(
        m.action("api", Action::Kill, first.run_id, generation)
            .await
            .is_err()
    );
    action(&m, Action::Restart).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = state(&m, "running").await;
            if p.run_id != second.run_id {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    m.shutdown().await;
}
#[tokio::test]
async fn changed_spec_is_pending_and_removed_running_entry_remains_stoppable() {
    let m = Manager::new(config(false, ""), env!("CARGO_BIN_EXE_procshelf").into());
    state(&m, "stopped").await;
    m.apply(config(true, "")).await;
    assert_eq!(m.snapshot().await.processes[0].state, "stopped");
    action(&m, Action::Start).await;
    state(&m, "running").await;
    m.apply(config(true, "[processes.api.env]\nMODE = 'new'\n"))
        .await;
    assert!(m.snapshot().await.processes[0].config_pending);
    m.apply(Config::parse("", Path::new("/tmp")).unwrap()).await;
    assert!(m.snapshot().await.processes[0].removed);
    action(&m, Action::Stop).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !m.snapshot().await.processes.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    m.shutdown().await;
}
#[tokio::test]
async fn newly_added_autostart_runs_but_unknown_program_reports_failure() {
    let m = Manager::new(
        Config::parse("", Path::new("/tmp")).unwrap(),
        env!("CARGO_BIN_EXE_procshelf").into(),
    );
    m.apply(config(true, "")).await;
    state(&m, "running").await;
    m.shutdown().await;
    let bad = Config::parse(
        "[processes.api]\nprogram = '/does/not/exist'\nautostart = true",
        Path::new("/tmp"),
    )
    .unwrap();
    let m = Manager::new(bad, env!("CARGO_BIN_EXE_procshelf").into());
    state(&m, "failed").await;
    m.shutdown().await;
}
