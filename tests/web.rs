use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use procshelf::{configuration::Config, processes::manager::Manager, web::router};
use std::path::Path;
use tower::ServiceExt;
fn app() -> axum::Router {
    let config = Config::parse("[processes.api]\nprogram='/bin/sh'", Path::new("/tmp")).unwrap();
    let listen = config.listen;
    router(
        Manager::new(config, env!("CARGO_BIN_EXE_procshelf").into()),
        listen,
        "unit-test-token".into(),
    )
}
#[tokio::test]
async fn state_and_gui_are_available_on_loopback_with_security_headers() {
    for path in ["/api/state", "/"] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("Host", "localhost:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(response.headers().contains_key("content-security-policy"));
        if path == "/api/state" {
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(state["processes"][0]["state"], "stopped");
            assert!(state["processes"][0].get("env").is_none());
        }
    }
}
#[tokio::test]
async fn rejects_dns_rebinding_cross_origin_and_missing_csrf_token() {
    for (host, origin, token) in [
        (
            "attacker.example:8787",
            "http://attacker.example:8787",
            "unit-test-token",
        ),
        (
            "localhost:8787",
            "http://attacker.example",
            "unit-test-token",
        ),
        ("localhost:8787", "http://localhost:8787", ""),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/processes/api/actions")
                    .header("Host", host)
                    .header("Origin", origin)
                    .header("X-Procshelf-Token", token)
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"action":"start","expected_run_id":null,"config_generation":1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
#[tokio::test]
async fn rejects_stale_generation_even_with_valid_csrf() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/processes/api/actions")
                .header("Host", "127.0.0.1:8787")
                .header("Origin", "http://127.0.0.1:8787")
                .header("X-Procshelf-Token", "unit-test-token")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"action":"start","expected_run_id":null,"config_generation":0}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
