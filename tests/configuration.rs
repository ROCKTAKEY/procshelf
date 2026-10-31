use procshelf::configuration::Config;
use std::{path::Path, time::Duration};

#[test]
fn manual_by_default_loopback_and_relative_paths_resolved_from_config() {
    let config = Config::parse(
        "[processes.api]\nprogram = './bin/api'\n",
        Path::new("/project"),
    )
    .unwrap();
    assert_eq!(config.listen.to_string(), "127.0.0.1:8787");
    let api = &config.processes["api"];
    assert!(!api.autostart());
    assert_eq!(api.name(), "api");
    assert_eq!(api.program(), Path::new("/project/bin/api"));
    assert_eq!(api.cwd(), Path::new("/project"));
    assert_eq!(api.stop_timeout(), Duration::from_secs(10));
}

#[test]
fn explicit_vpn_address_and_autostart_are_preserved() {
    let config = Config::parse(
        r#"
[web]
listen = "100.64.0.4:8787"
[processes.api]
program = "/bin/sh"
autostart = true
args = ["-c", "echo hello"]
cwd = "work"
stop_timeout_seconds = 2
[processes.api.env]
MODE = "development"
"#,
        Path::new("/project"),
    )
    .unwrap();
    assert_eq!(config.listen.to_string(), "100.64.0.4:8787");
    let api = &config.processes["api"];
    assert!(api.autostart());
    assert_eq!(api.cwd(), Path::new("/project/work"));
    assert_eq!(api.env()["MODE"], "development");
}

#[test]
fn rejects_invalid_inputs_and_misspelled_fields_before_apply() {
    for input in [
        "[processes.api]\nprogram = ''",
        "[processes.api]\nprogram = '/bin/sh'\nauto_start = true",
        "[processes.api]\nprogram = '/bin/sh'\nstop_timeout_seconds = -1",
        "[processes.'bad/id']\nprogram = '/bin/sh'",
        "[web]\nlisten = 'not-an-address'",
        "[web]\nlisten = '0.0.0.0:8787'",
        "[processes.api]\nprogram = '/bin/sh'\n[processes.api.env]\n'A=B' = 'x'",
    ] {
        assert!(
            Config::parse(input, Path::new("/project")).is_err(),
            "{input}"
        );
    }
}

#[test]
fn supervisor_wire_format_preserves_validation() {
    let config = Config::parse("[processes.api]\nprogram='/bin/sh'", Path::new("/tmp")).unwrap();
    let mut wire = serde_json::to_value(&config.processes["api"]).unwrap();
    wire["program"] = serde_json::Value::String(String::new());
    assert!(serde_json::from_value::<procshelf::configuration::ProcessSpec>(wire).is_err());
}

#[test]
fn description_is_optional_and_preserves_multiline_plain_text() {
    let config = Config::parse(
        "[processes.api]\nprogram='/bin/sh'\ndescription='''API の開発サーバー。\n<local> の動作確認に使用。'''",
        Path::new("/tmp"),
    ).unwrap();
    let wire = serde_json::to_value(&config.processes["api"]).unwrap();
    assert_eq!(
        wire["description"],
        "API の開発サーバー。\n<local> の動作確認に使用。"
    );
    let config = Config::parse("[processes.api]\nprogram='/bin/sh'", Path::new("/tmp")).unwrap();
    let wire = serde_json::to_value(&config.processes["api"]).unwrap();
    assert_eq!(wire["description"], "");
}
