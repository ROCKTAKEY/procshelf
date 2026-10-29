use crate::processes::manager::{Action, Manager};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::{convert::Infallible, net::SocketAddr, time::Duration};
use tokio_stream::wrappers::WatchStream;

#[derive(Clone)]
struct AppState {
    manager: Manager,
    hosts: Vec<String>,
    token: String,
}
pub fn router(manager: Manager, listen: SocketAddr, token: String) -> Router {
    let mut hosts = vec![listen.to_string()];
    if listen.ip().is_loopback() {
        hosts.push(format!("localhost:{}", listen.port()));
    }
    let state = AppState {
        manager,
        hosts,
        token,
    };
    Router::new()
        .route(
            "/",
            get(|| async {
                asset(
                    "text/html; charset=utf-8",
                    include_bytes!("../../web/dist/index.html"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_bytes!("../../web/dist/app.js"),
                )
            }),
        )
        .route(
            "/app.css",
            get(|| async {
                asset(
                    "text/css; charset=utf-8",
                    include_bytes!("../../web/dist/app.css"),
                )
            }),
        )
        .route("/api/session", get(session))
        .route("/api/state", get(snapshot))
        .route("/api/events", get(events))
        .route("/api/processes/{id}/logs", get(logs))
        .route("/api/processes/{id}/actions", post(action))
        .layer(DefaultBodyLimit::max(8192))
        .layer(middleware::from_fn_with_state(state.clone(), protect))
        .with_state(state)
}
fn asset(content_type: &'static str, bytes: &'static [u8]) -> Response {
    ([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response()
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
async fn protect(State(state): State<AppState>, request: Request<Body>, next: Next) -> Response {
    let headers = request.headers();
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let allowed_host = state.hosts.iter().any(|h| h == host);
    let allowed_origin = headers
        .get("origin")
        .is_none_or(|v| v.to_str().is_ok_and(|s| s == format!("http://{host}")));
    let same_site = headers
        .get("sec-fetch-site")
        .is_none_or(|v| v != "cross-site");
    let changes_state = !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    );
    let csrf_ok = !changes_state
        || headers
            .get("x-procshelf-token")
            .is_some_and(|v| v.as_bytes() == state.token.as_bytes());
    let mut response = if !allowed_host || !allowed_origin || !same_site || !csrf_ok {
        error(
            StatusCode::FORBIDDEN,
            "request origin or session token rejected",
        )
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"));
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}
async fn session(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({"token":state.token}))
}
async fn snapshot(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.manager.snapshot().await)
}
async fn logs(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.manager.logs(&id).await {
        Some(logs) => Json(logs).into_response(),
        None => error(StatusCode::NOT_FOUND, "process not found"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionRequest {
    action: Action,
    expected_run_id: Option<u64>,
    config_generation: u64,
}
async fn action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ActionRequest>,
) -> Response {
    match state
        .manager
        .action(
            &id,
            request.action,
            request.expected_run_id,
            request.config_generation,
        )
        .await
    {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({"accepted":true}))).into_response(),
        Err(message) => error(StatusCode::CONFLICT, &message),
    }
}
async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Each event is a complete snapshot. A lagging or reconnecting client never needs to replay a partial state history.
    let changes = tokio_stream::StreamExt::throttle(
        WatchStream::new(state.manager.subscribe()),
        Duration::from_millis(150),
    );
    let snapshots = changes
        .then(move |_| {
            let manager = state.manager.clone();
            async move { manager.snapshot().await }
        })
        .take_while(|s| std::future::ready(!s.shutting_down))
        .map(|snapshot| {
            Ok(Event::default()
                .event("state")
                .id(snapshot.revision.to_string())
                .json_data(snapshot)
                .expect("snapshot is serializable"))
        });
    Sse::new(snapshots).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
