use super::{
    manager::{DriverEvent, RunEvent, RunLauncher},
    protocol::Event,
};
use crate::configuration::ProcessSpec;
use std::{fs::File, os::fd::AsRawFd, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

pub(super) struct SupervisorLauncher {
    pub executable: PathBuf,
    pub lease: Option<File>,
    pub events: mpsc::Sender<RunEvent>,
}
impl RunLauncher for SupervisorLauncher {
    fn launch(
        &self,
        id: String,
        run: u64,
        spec: &ProcessSpec,
    ) -> Result<mpsc::Sender<&'static str>, String> {
        launch(
            &self.executable,
            self.lease.as_ref(),
            id,
            run,
            spec,
            self.events.clone(),
        )
    }
}
fn launch(
    executable: &PathBuf,
    lease: Option<&File>,
    id: String,
    run: u64,
    spec: &ProcessSpec,
    events: mpsc::Sender<RunEvent>,
) -> Result<mpsc::Sender<&'static str>, String> {
    let mut command = Command::new(executable);
    command
        .arg("--supervise")
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(lease) = lease {
        let fd = lease.as_raw_fd();
        command.arg(fd.to_string());
        // SAFETY: the borrowed lease remains open through spawn. Only the async-signal-safe
        // fcntl runs after fork; changing flags in the child does not alter the parent's flags.
        // The inherited open file description keeps flock held if the manager crashes.
        unsafe {
            command.pre_exec(move || {
                if nix::libc::fcntl(fd, nix::libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut initial = serde_json::to_vec(spec).map_err(|e| e.to_string())?;
    initial.push(b'\n');
    let (control, mut requests) = mpsc::channel::<&'static str>(8);
    tokio::spawn(async move {
        if input.write_all(&initial).await.is_err() {
            return;
        }
        while let Some(request) = requests.recv().await {
            if input
                .write_all(format!("{request}\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(async move {
        let mut completed = false;
        while let Ok(Some(line)) = output.next_line().await {
            match serde_json::from_str::<Event>(&line) {
                Ok(event) => {
                    completed |= matches!(event, Event::Finished { .. } | Event::Failed { .. });
                    if events
                        .send(RunEvent {
                            id: id.clone(),
                            run,
                            event: DriverEvent::Message(event),
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let status = child.wait().await;
        if !completed {
            let _ = events
                .send(RunEvent {
                    id,
                    run,
                    event: DriverEvent::Disconnected(format!(
                        "supervisor disconnected; ownership unknown ({status:?})"
                    )),
                })
                .await;
        }
    });
    Ok(control)
}
