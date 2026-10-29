use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Started {
        pid: u32,
    },
    Output {
        stream: String,
        text: String,
    },
    Dropped {
        chunks: u64,
    },
    Finished {
        code: Option<i32>,
        signal: Option<i32>,
    },
    Failed {
        message: String,
    },
}
