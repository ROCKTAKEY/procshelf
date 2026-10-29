use procshelf::configuration::Config;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn start(script: &str) -> (std::process::Child, BufReader<std::process::ChildStdout>) {
    let config = Config::parse(&format!("[processes.test]\nprogram = '/bin/sh'\nargs = ['-c', {script:?}]\nstop_timeout_seconds = 0\n"), Path::new("/tmp")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_procshelf"))
        .arg("--supervise")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.as_mut().unwrap(),
        "{}",
        serde_json::to_string(&config.processes["test"]).unwrap()
    )
    .unwrap();
    let reader = BufReader::new(child.stdout.take().unwrap());
    (child, reader)
}
fn next(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert!(
        reader.read_line(&mut line).unwrap() > 0,
        "supervisor ended before emitting an event"
    );
    serde_json::from_str(&line).unwrap()
}
fn wait(child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("supervisor did not exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn reports_start_output_and_exit_status() {
    let (mut child, mut reader) = start("echo ready; echo error >&2; exit 7");
    assert_eq!(next(&mut reader)["type"], "started");
    let mut streams = Vec::new();
    loop {
        let event = next(&mut reader);
        if event["type"] == "finished" {
            assert_eq!(event["code"], 7);
            break;
        }
        if event["type"] == "output" {
            streams.push(event["stream"].as_str().unwrap().to_owned());
        }
    }
    assert!(streams.contains(&"stdout".into()) && streams.contains(&"stderr".into()));
    wait(&mut child);
}
#[test]
fn control_pipe_closure_kills_and_reaps_child_and_grandchild() {
    let (mut child, mut reader) = start("sleep 60 & echo $!; wait");
    let started = next(&mut reader);
    assert_eq!(started["type"], "started");
    let leader = started["pid"].as_u64().unwrap();
    let grandchild = loop {
        let event = next(&mut reader);
        if event["type"] == "output" {
            break event["text"]
                .as_str()
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap();
        }
    };
    drop(child.stdin.take());
    while next(&mut reader)["type"] != "finished" {}
    wait(&mut child);
    assert!(!Path::new(&format!("/proc/{leader}")).exists());
    assert!(!Path::new(&format!("/proc/{grandchild}")).exists());
}
#[test]
fn stopping_escalates_when_term_is_ignored() {
    let (mut child, mut reader) = start("trap '' TERM; echo ready; while :; do sleep 1; done");
    assert_eq!(next(&mut reader)["type"], "started");
    while next(&mut reader)["type"] != "output" {}
    child.stdin.as_mut().unwrap().write_all(b"stop\n").unwrap();
    loop {
        let event = next(&mut reader);
        if event["type"] == "finished" {
            assert_eq!(event["signal"], 9);
            break;
        }
    }
    wait(&mut child);
}
