use serde::Serialize;
use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize)]
pub struct LogEntry {
    pub seq: u64,
    pub run_id: u64,
    pub time_ms: u64,
    pub stream: String,
    pub text: String,
}
#[derive(Clone, Serialize)]
pub struct LogSnapshot {
    pub entries: Vec<LogEntry>,
    pub discarded: u64,
}
#[derive(Default)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    bytes: usize,
    next: u64,
    discarded: u64,
}
impl LogBuffer {
    pub fn push(&mut self, run_id: u64, stream: String, text: String) {
        self.next += 1;
        self.bytes += text.len();
        self.entries.push_back(LogEntry {
            seq: self.next,
            run_id,
            time_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            stream,
            text,
        });
        while self.bytes > 512 * 1024 || self.entries.len() > 2000 {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= old.text.len();
                self.discarded += 1;
            }
        }
    }
    pub fn snapshot(&self) -> LogSnapshot {
        LogSnapshot {
            entries: self.entries.iter().cloned().collect(),
            discarded: self.discarded,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unbroken_output_is_bounded_and_truncation_is_visible() {
        let mut buffer = LogBuffer::default();
        for _ in 0..300 {
            buffer.push(1, "stdout".into(), "x".repeat(4096));
        }
        let snapshot = buffer.snapshot();
        assert!(snapshot.entries.iter().map(|e| e.text.len()).sum::<usize>() <= 512 * 1024);
        assert!(snapshot.discarded > 0);
        assert_eq!(snapshot.entries.last().unwrap().seq, 300);
    }
}
