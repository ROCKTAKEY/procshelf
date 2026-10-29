use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub(crate) name: String,
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) autostart: bool,
    pub(crate) stop_timeout: Duration,
}
impl ProcessSpec {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn program(&self) -> &Path {
        &self.program
    }
    pub fn args(&self) -> &[String] {
        &self.args
    }
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }
    pub fn autostart(&self) -> bool {
        self.autostart
    }
    pub fn stop_timeout(&self) -> Duration {
        self.stop_timeout
    }
}
#[derive(Debug, Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub processes: BTreeMap<String, ProcessSpec>,
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    web: Web,
    #[serde(default)]
    processes: BTreeMap<String, RawProcess>,
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Web {
    listen: SocketAddr,
}
impl Default for Web {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8787)),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProcess {
    name: Option<String>,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<PathBuf>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    autostart: bool,
    stop_timeout_seconds: Option<u64>,
}
impl Config {
    pub fn parse(input: &str, base: &Path) -> Result<Self, String> {
        if input.len() > 1024 * 1024 {
            return Err("configuration exceeds 1 MiB".into());
        }
        let raw: RawConfig = toml::from_str(input).map_err(|e| {
            // TOML's rendered error includes source text, which can contain environment secrets.
            let line = e
                .span()
                .map(|s| input[..s.start].bytes().filter(|b| *b == b'\n').count() + 1);
            format!(
                "invalid TOML or unknown field near line {}",
                line.unwrap_or(1)
            )
        })?;
        if raw.web.listen.ip().is_unspecified() || raw.web.listen.port() == 0 {
            return Err("web.listen must specify a concrete IP and nonzero port".into());
        }
        let mut processes = BTreeMap::new();
        for (id, p) in raw.processes {
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            {
                return Err(
                    "process IDs must contain 1–128 ASCII letters, digits, underscores or hyphens"
                        .into(),
                );
            }
            let name = p.name.unwrap_or_else(|| id.clone());
            let timeout = p.stop_timeout_seconds.unwrap_or(10);
            if name.trim().is_empty()
                || p.program.trim().is_empty()
                || timeout > 3600
                || p.program.contains('\0')
                || p.args.iter().any(|a| a.contains('\0'))
                || p.cwd
                    .as_ref()
                    .is_some_and(|p| p.as_os_str().as_encoded_bytes().contains(&0))
                || p.env
                    .iter()
                    .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
            {
                return Err(format!("invalid process specification for {id}"));
            }
            let resolve = |p: &Path| -> PathBuf { base.join(p).components().collect() };
            processes.insert(
                id,
                ProcessSpec {
                    name,
                    program: resolve(Path::new(&p.program)),
                    args: p.args,
                    cwd: resolve(p.cwd.as_deref().unwrap_or(Path::new("."))),
                    env: p.env,
                    autostart: p.autostart,
                    stop_timeout: Duration::from_secs(timeout),
                },
            );
        }
        Ok(Self {
            listen: raw.web.listen,
            processes,
        })
    }
}
