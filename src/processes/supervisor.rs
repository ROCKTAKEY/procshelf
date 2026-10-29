use crate::configuration::ProcessSpec;
use nix::{
    errno::Errno,
    sys::{
        prctl::set_child_subreaper,
        signal::{Signal, killpg},
        wait::{Id, WaitPidFlag, WaitStatus, waitid, waitpid},
    },
    unistd::Pid,
};
use std::{
    io,
    os::unix::process::ExitStatusExt,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
    time::Instant,
};

use super::protocol::Event;

struct Running {
    child: Child,
    group: Pid,
}
struct Stopping {
    process: Running,
    deadline: Instant,
    forced: bool,
}
enum Execution {
    Running(Running),
    Stopping(Stopping),
}
impl Running {
    fn stop(self, timeout: Duration) -> io::Result<Stopping> {
        signal(self.group, Signal::SIGTERM)?;
        Ok(Stopping {
            process: self,
            deadline: Instant::now() + timeout,
            forced: false,
        })
    }
    fn exited(&self) -> io::Result<bool> {
        // Keeping the leader waitable pins the group ID until the final group-wide kill.
        // https://man7.org/linux/man-pages/man2/waitid.2.html (WNOWAIT)
        match waitid(
            Id::Pid(self.group),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
        ) {
            Ok(WaitStatus::StillAlive) => Ok(false),
            Ok(_) => Ok(true),
            Err(e) => Err(e.into()),
        }
    }
    async fn finish(mut self) -> io::Result<Event> {
        signal(self.group, Signal::SIGKILL)?;
        let status = self.child.wait().await?;
        // Subreaping lets us account for grandchildren orphaned when the leader exits.
        loop {
            match waitpid(
                Pid::from_raw(-self.group.as_raw()),
                Some(WaitPidFlag::WNOHANG),
            ) {
                Err(Errno::ECHILD) => break,
                Ok(WaitStatus::StillAlive) => tokio::time::sleep(Duration::from_millis(20)).await,
                Ok(_) | Err(Errno::EINTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Event::Finished {
            code: status.code(),
            signal: status.signal(),
        })
    }
}
impl Execution {
    fn process(&self) -> &Running {
        match self {
            Self::Running(p) => p,
            Self::Stopping(p) => &p.process,
        }
    }
    fn into_process(self) -> Running {
        match self {
            Self::Running(p) => p,
            Self::Stopping(p) => p.process,
        }
    }
    fn stop(self, timeout: Duration) -> io::Result<Self> {
        Ok(match self {
            Self::Running(p) => Self::Stopping(p.stop(timeout)?),
            stopped => stopped,
        })
    }
}
fn signal(group: Pid, signal: Signal) -> io::Result<()> {
    match killpg(group, signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
async fn read_output(
    mut input: impl AsyncRead + Unpin,
    stream: &'static str,
    tx: mpsc::Sender<Event>,
    dropped: Arc<AtomicU64>,
) {
    let mut buf = [0; 4096];
    loop {
        match input.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let event = Event::Output {
                    stream: stream.into(),
                    text: String::from_utf8_lossy(&buf[..n]).into_owned(),
                };
                match tx.try_send(event) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        }
    }
}

pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    set_child_subreaper(true)?;
    let mut input = BufReader::new(tokio::net::unix::pipe::Receiver::from_owned_fd(
        nix::unistd::dup(io::stdin())?,
    )?);
    let mut first = String::new();
    input.read_line(&mut first).await?;
    let spec: ProcessSpec = serde_json::from_str(&first)?;
    let (tx, mut rx) = mpsc::channel::<Event>(128);
    let writer = tokio::spawn(async move {
        let mut out =
            tokio::net::unix::pipe::Sender::from_owned_fd(nix::unistd::dup(io::stdout())?)?;
        while let Some(event) = rx.recv().await {
            let mut bytes = serde_json::to_vec(&event).map_err(io::Error::other)?;
            bytes.push(b'\n');
            out.write_all(&bytes).await?;
            out.flush().await?;
        }
        Ok::<_, io::Error>(())
    });
    let child = Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            let _ = tx
                .send(Event::Failed {
                    message: e.to_string(),
                })
                .await;
            drop(tx);
            writer.await??;
            return Ok(());
        }
    };
    let pid = child.id().expect("newly spawned process has a PID");
    let _ = tx.send(Event::Started { pid }).await;
    let dropped = Arc::new(AtomicU64::new(0));
    let out = tokio::spawn(read_output(
        child.stdout.take().unwrap(),
        "stdout",
        tx.clone(),
        dropped.clone(),
    ));
    let err = tokio::spawn(read_output(
        child.stderr.take().unwrap(),
        "stderr",
        tx.clone(),
        dropped.clone(),
    ));
    let mut execution = Execution::Running(Running {
        child,
        group: Pid::from_raw(pid as i32),
    });
    let mut commands = input.lines();
    let mut control_open = true;
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    loop {
        tokio::select! {
            command = commands.next_line(), if control_open => {
                match command {
                    Ok(Some(c)) if c == "kill" => { signal(execution.process().group, Signal::SIGKILL)?; },
                    Ok(Some(c)) if c == "stop" => { execution = execution.stop(spec.stop_timeout)?; },
                    Ok(Some(_)) => {},
                    _ => { control_open = false; execution = execution.stop(spec.stop_timeout)?; }
                }
            }
            _ = tick.tick() => {
                if let Execution::Stopping(ref mut stopping) = execution
                    && !stopping.forced && Instant::now() >= stopping.deadline {
                    signal(stopping.process.group, Signal::SIGKILL)?;
                    stopping.forced = true;
                }
                if execution.process().exited()? { break; }
            }
        }
    }
    let finished = execution.into_process().finish().await?;
    // All supported descendants are gone, so both pipes now reach EOF.
    out.await?;
    err.await?;
    let lost = dropped.load(Ordering::Relaxed);
    if lost > 0 {
        let _ = tx.send(Event::Dropped { chunks: lost }).await;
    }
    let _ = tx.send(finished).await;
    drop(tx);
    writer.await??;
    Ok(())
}
