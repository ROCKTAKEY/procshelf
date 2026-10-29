use super::protocol::Event;
use crate::{
    configuration::{Config, ProcessSpec},
    logs::{LogBuffer, LogSnapshot},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap, net::SocketAddr, path::PathBuf, process::Stdio, sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Mutex, mpsc, watch},
};

#[derive(Clone)]
pub struct Manager {
    core: Arc<Mutex<Core>>,
    changes: watch::Receiver<u64>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Kill,
    Restart,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProcessView {
    pub id: String,
    pub name: String,
    pub state: String,
    pub run_id: Option<u64>,
    pub pid: Option<u32>,
    pub autostart: bool,
    pub config_pending: bool,
    pub removed: bool,
    pub outcome: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub config_generation: u64,
    pub config_error: Option<String>,
    pub listen: String,
    pub configured_listen: String,
    pub shutting_down: bool,
    pub processes: Vec<ProcessView>,
}
struct ActiveRun {
    id: u64,
    spec: ProcessSpec,
    control: mpsc::Sender<&'static str>,
    pid: Option<u32>,
    phase: Phase,
    restart: bool,
}
#[derive(Clone, Copy)]
enum Phase {
    Starting,
    Running,
    Stopping,
}
enum Execution {
    Idle,
    Active(ActiveRun),
    Lost,
}
struct Entry {
    desired: Option<ProcessSpec>,
    last_spec: ProcessSpec,
    execution: Execution,
    last_run: Option<u64>,
    outcome: Option<String>,
    failed: bool,
    logs: LogBuffer,
}
impl Entry {
    fn view(&self, id: &str) -> ProcessView {
        let spec = self.desired.as_ref().unwrap_or(&self.last_spec);
        let (state, pid, pending) = match &self.execution {
            Execution::Active(run) => (
                match run.phase {
                    Phase::Starting => "starting",
                    Phase::Running => "running",
                    Phase::Stopping => "stopping",
                },
                run.pid,
                self.desired
                    .as_ref()
                    .is_some_and(|s| !s.execution_eq(&run.spec)),
            ),
            Execution::Lost => ("unknown", None, false),
            Execution::Idle => (
                if self.failed {
                    "failed"
                } else if self.last_run.is_some() {
                    "exited"
                } else {
                    "stopped"
                },
                None,
                false,
            ),
        };
        ProcessView {
            id: id.into(),
            name: spec.name.clone(),
            state: state.into(),
            run_id: self.last_run,
            pid,
            autostart: spec.autostart,
            config_pending: pending,
            removed: self.desired.is_none(),
            outcome: self.outcome.clone(),
            program: spec.program.display().to_string(),
            args: spec.args.clone(),
            cwd: spec.cwd.display().to_string(),
        }
    }
}
struct Core {
    entries: BTreeMap<String, Entry>,
    executable: PathBuf,
    events: mpsc::Sender<RunEvent>,
    changes: watch::Sender<u64>,
    revision: u64,
    generation: u64,
    next_run: u64,
    config_error: Option<String>,
    listen: SocketAddr,
    configured_listen: SocketAddr,
    shutting_down: bool,
}
struct RunEvent {
    id: String,
    run: u64,
    event: DriverEvent,
}
enum DriverEvent {
    Message(Event),
    Disconnected(String),
}
impl Core {
    fn changed(&mut self) {
        self.revision += 1;
        self.changes.send_replace(self.revision);
    }
    fn apply(&mut self, config: Config) {
        self.generation += 1;
        self.config_error = None;
        self.configured_listen = config.listen;
        for (id, entry) in &mut self.entries {
            if !config.processes.contains_key(id) {
                entry.desired = None;
            }
        }
        self.entries
            .retain(|_, e| e.desired.is_some() || !matches!(e.execution, Execution::Idle));
        let mut start = Vec::new();
        for (id, spec) in config.processes {
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.desired = Some(spec);
            } else {
                if spec.autostart {
                    start.push(id.clone());
                }
                self.entries.insert(
                    id,
                    Entry {
                        desired: Some(spec.clone()),
                        last_spec: spec,
                        execution: Execution::Idle,
                        last_run: None,
                        outcome: None,
                        failed: false,
                        logs: LogBuffer::default(),
                    },
                );
            }
        }
        for id in start {
            let _ = self.start(&id);
        }
        self.changed();
    }
    fn start(&mut self, id: &str) -> Result<(), String> {
        let entry = self.entries.get_mut(id).ok_or("process not found")?;
        if !matches!(entry.execution, Execution::Idle) {
            return Err("process is already active or its ownership is unknown".into());
        }
        let spec = entry
            .desired
            .clone()
            .ok_or("process was removed from configuration")?;
        self.next_run += 1;
        let run = self.next_run;
        entry.last_run = Some(run);
        entry.last_spec = spec.clone();
        entry.outcome = None;
        entry.failed = false;
        match launch(&self.executable, id.into(), run, &spec, self.events.clone()) {
            Ok(control) => {
                entry.execution = Execution::Active(ActiveRun {
                    id: run,
                    spec,
                    control,
                    pid: None,
                    phase: Phase::Starting,
                    restart: false,
                });
                Ok(())
            }
            Err(error) => {
                entry.failed = true;
                entry.outcome = Some(error.clone());
                Err(error)
            }
        }
    }
    fn action(
        &mut self,
        id: &str,
        action: Action,
        expected_run: Option<u64>,
        generation: u64,
    ) -> Result<(), String> {
        if self.shutting_down {
            return Err("service is shutting down".into());
        }
        if self.generation != generation {
            return Err("configuration changed; refresh before retrying".into());
        }
        let entry = self.entries.get_mut(id).ok_or("process not found")?;
        if entry.last_run != expected_run {
            return Err("execution changed; refresh before retrying".into());
        }
        if matches!(action, Action::Start) {
            return self.start(id);
        }
        match &mut entry.execution {
            Execution::Active(run) => {
                if matches!(action, Action::Restart) && entry.desired.is_none() {
                    return Err("removed process cannot restart".into());
                }
                run.control
                    .try_send(if matches!(action, Action::Kill) {
                        "kill"
                    } else {
                        "stop"
                    })
                    .map_err(|_| "supervisor is busy or disconnected")?;
                run.restart = matches!(action, Action::Restart);
                run.phase = Phase::Stopping;
                Ok(())
            }
            _ => Err("process has no controllable execution".into()),
        }
    }
    fn event(&mut self, message: RunEvent) {
        let Some(entry) = self.entries.get_mut(&message.id) else {
            return;
        };
        let Execution::Active(run) = &mut entry.execution else {
            return;
        };
        if run.id != message.run {
            return;
        }
        let mut finished = false;
        let mut restart = false;
        match message.event {
            DriverEvent::Message(Event::Started { pid }) => {
                run.pid = Some(pid);
                if matches!(run.phase, Phase::Starting) {
                    run.phase = Phase::Running;
                }
            }
            DriverEvent::Message(Event::Output { stream, text }) => {
                entry.logs.push(run.id, stream, text)
            }
            DriverEvent::Message(Event::Dropped { chunks }) => entry.logs.push(
                run.id,
                "system".into(),
                format!("{chunks} output chunks omitted because the log queue was full\n"),
            ),
            DriverEvent::Message(Event::Finished { code, signal }) => {
                entry.outcome = Some(match (code, signal) {
                    (Some(c), _) => format!("exit {c}"),
                    (_, Some(s)) => format!("signal {s}"),
                    _ => "exited".into(),
                });
                restart = run.restart && entry.desired.is_some() && !self.shutting_down;
                finished = true;
            }
            DriverEvent::Message(Event::Failed { message }) => {
                entry.failed = true;
                entry.outcome = Some(message);
                finished = true;
            }
            DriverEvent::Disconnected(error) => {
                entry.outcome = Some(error);
                entry.execution = Execution::Lost;
            }
        }
        if finished {
            entry.execution = Execution::Idle;
            if entry.desired.is_none() {
                self.entries.remove(&message.id);
            } else if restart {
                let _ = self.start(&message.id);
            }
        }
        self.changed();
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            revision: self.revision,
            config_generation: self.generation,
            config_error: self.config_error.clone(),
            listen: self.listen.to_string(),
            configured_listen: self.configured_listen.to_string(),
            shutting_down: self.shutting_down,
            processes: self.entries.iter().map(|(id, e)| e.view(id)).collect(),
        }
    }
}
impl Manager {
    pub fn new(config: Config, executable: PathBuf) -> Self {
        let (events, mut incoming) = mpsc::channel(128);
        let (changes, receiver) = watch::channel(0);
        let mut core = Core {
            entries: BTreeMap::new(),
            executable,
            events,
            changes,
            revision: 0,
            generation: 0,
            next_run: 0,
            config_error: None,
            listen: config.listen,
            configured_listen: config.listen,
            shutting_down: false,
        };
        core.apply(config);
        let core = Arc::new(Mutex::new(core));
        let weak = Arc::downgrade(&core);
        tokio::spawn(async move {
            while let Some(event) = incoming.recv().await {
                let Some(core) = weak.upgrade() else {
                    break;
                };
                core.lock().await.event(event);
            }
        });
        Self {
            core,
            changes: receiver,
        }
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.clone()
    }
    pub async fn snapshot(&self) -> Snapshot {
        self.core.lock().await.snapshot()
    }
    pub async fn logs(&self, id: &str) -> Option<LogSnapshot> {
        self.core
            .lock()
            .await
            .entries
            .get(id)
            .map(|e| e.logs.snapshot())
    }
    pub async fn apply(&self, config: Config) {
        let mut c = self.core.lock().await;
        if !c.shutting_down {
            c.apply(config);
        }
    }
    pub async fn config_error(&self, error: String) {
        let mut c = self.core.lock().await;
        c.config_error = Some(error);
        c.changed();
    }
    pub async fn action(
        &self,
        id: &str,
        action: Action,
        run: Option<u64>,
        generation: u64,
    ) -> Result<(), String> {
        let mut core = self.core.lock().await;
        let result = core.action(id, action, run, generation);
        core.changed();
        result
    }
    pub async fn shutdown(&self) {
        {
            let mut core = self.core.lock().await;
            core.shutting_down = true;
            for entry in core.entries.values_mut() {
                if let Execution::Active(run) = &mut entry.execution {
                    run.restart = false;
                    let _ = run.control.try_send("stop");
                    run.phase = Phase::Stopping;
                }
            }
            core.changed();
        }
        loop {
            if self
                .core
                .lock()
                .await
                .entries
                .values()
                .all(|e| !matches!(e.execution, Execution::Active(_)))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
fn launch(
    executable: &PathBuf,
    id: String,
    run: u64,
    spec: &ProcessSpec,
    events: mpsc::Sender<RunEvent>,
) -> Result<mpsc::Sender<&'static str>, String> {
    let mut child = Command::new(executable)
        .arg("--supervise")
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
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
