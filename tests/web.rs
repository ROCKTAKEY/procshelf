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
        procshelf::authentication::Authentication::new(
            procshelf::authentication::AccessKey::parse(&"a".repeat(64)).unwrap(),
        ),
    )
}
#[tokio::test]
async fn state_and_gui_are_available_on_loopback_with_security_headers() {
    let app = app();
    let cookie = login(&app).await;
    for path in ["/api/state", "/"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("Cookie", &cookie)
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
    let app = app();
    let cookie = login(&app).await;
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
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/processes/api/actions")
                    .header("Host", host)
                    .header("Origin", origin)
                    .header("Cookie", &cookie)
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
    let app = app();
    let cookie = login(&app).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/processes/api/actions")
                .header("Host", "127.0.0.1:8787")
                .header("Origin", "http://127.0.0.1:8787")
                .header("Cookie", cookie)
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

#[tokio::test]
async fn unauthenticated_clients_cannot_read_process_data_or_obtain_operation_tokens() {
    for path in [
        "/api/session",
        "/api/state",
        "/api/events",
        "/api/processes/api/logs",
    ] {
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

async fn login(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("Host", "localhost:8787")
                .header("X-Procshelf-Login", "1")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({"access_key":"a".repeat(64)}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    cookie.split(';').next().unwrap().to_owned()
}

#[tokio::test]
async fn login_rejects_invalid_keys_and_cross_site_requests_and_logout_revokes_cookie() {
    let app = app();
    for (key, origin, status) in [
        (
            "b".repeat(64),
            "http://localhost:8787",
            StatusCode::UNAUTHORIZED,
        ),
        ("a".repeat(64), "http://evil.example", StatusCode::FORBIDDEN),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("Host", "localhost:8787")
                    .header("Origin", origin)
                    .header("X-Procshelf-Login", "1")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"access_key":key}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(!response.headers().contains_key("set-cookie"));
    }
    let cookie = login(&app).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/logout")
                .header("Host", "localhost:8787")
                .header("Cookie", &cookie)
                .header("X-Procshelf-Token", "unit-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/state")
                .header("Host", "localhost:8787")
                .header("Cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn neither_csrf_tokens_nor_forged_cookies_grant_access() {
    for cookie in ["", "procshelf_8787=forged"] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/processes/api/actions")
                    .header("Host", "localhost:8787")
                    .header("Cookie", cookie)
                    .header("X-Procshelf-Token", "unit-test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"action":"start","expected_run_id":null,"config_generation":1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn logout_closes_existing_event_stream_and_service_restart_rejects_old_session() {
    let app = app();
    let cookie = login(&app).await;
    let stream = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/events")
                .header("Host", "localhost:8787")
                .header("Cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    let logout = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/logout")
                .header("Host", "localhost:8787")
                .header("Cookie", &cookie)
                .header("X-Procshelf-Token", "unit-test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        to_bytes(stream.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let response = self::app()
        .oneshot(
            Request::builder()
                .uri("/api/state")
                .header("Host", "localhost:8787")
                .header("Cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_requires_its_request_header_and_limits_attempts() {
    let app = app();
    let request = |header| {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/login")
            .header("Host", "localhost:8787")
            .header("Content-Type", "application/json");
        if header {
            builder = builder.header("X-Procshelf-Login", "1");
        }
        builder
            .body(Body::from(r#"{"access_key":"incorrect"}"#))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(request(false)).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    for _ in 0..20 {
        assert_eq!(
            app.clone().oneshot(request(true)).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        app.oneshot(request(true)).await.unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}
