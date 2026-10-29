use super::Config;
use crate::processes::manager::Manager;
use notify::{RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::mpsc, task::JoinHandle};

pub async fn read(path: &Path) -> Result<String, String> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("cannot open configuration: {e}"))?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| format!("cannot read configuration: {e}"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("configuration exceeds 1 MiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "configuration must be UTF-8".into())
}
pub struct ConfigWatch {
    task: JoinHandle<()>,
}
impl Drop for ConfigWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl ConfigWatch {
    pub fn start(path: PathBuf, manager: Manager, initial: String) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or("configuration has no parent directory")?
            .to_owned();
        let (tx, mut notifications) = mpsc::channel(1);
        let mut watcher =
            notify::recommended_watcher(move |_event: notify::Result<notify::Event>| {
                let _ = tx.try_send(());
            })
            .map_err(|e| e.to_string())?;
        // Editors commonly replace the inode on save. Watching the parent keeps observing the path.
        // https://docs.rs/notify/latest/notify/#editor-behaviour
        watcher
            .watch(&parent, RecursiveMode::NonRecursive)
            .map_err(|e| e.to_string())?;
        let task = tokio::spawn(async move {
            let _watcher = watcher;
            let mut previous = Some(initial);
            let mut last_error = None;
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                tokio::select! {
                    _=interval.tick()=>{},
                    _=notifications.recv()=>{
                        tokio::time::sleep(Duration::from_millis(80)).await;
                        while notifications.try_recv().is_ok() {}
                    },
                }
                match read(&path).await {
                    Ok(source) => {
                        if previous.as_ref() == Some(&source) {
                            continue;
                        }
                        match Config::parse(&source, &parent) {
                            Ok(config) => {
                                manager.apply(config).await;
                                last_error = None;
                            }
                            Err(error) => {
                                manager.config_error(error.clone()).await;
                                last_error = Some(error);
                            }
                        }
                        previous = Some(source);
                    }
                    Err(error) => {
                        previous = None;
                        if last_error.as_ref() != Some(&error) {
                            manager.config_error(error.clone()).await;
                            last_error = Some(error);
                        }
                    }
                }
            }
        });
        Ok(Self { task })
    }
}
