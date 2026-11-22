mod access;

use crate::authentication::{AuthenticatedSession, Authentication, LoginError};
use crate::configuration::PublicOrigin;
use crate::processes::manager::{Action, Manager};
use axum::{
    Extension, Json, Router,
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
    auth: Authentication,
    port: u16,
}
pub fn router(
    manager: Manager,
    listen: SocketAddr,
    public_origin: Option<PublicOrigin>,
    token: String,
    auth: Authentication,
) -> Router {
    let mut hosts = vec![listen.to_string()];
    if listen.ip().is_loopback() {
        hosts.push(format!("localhost:{}", listen.port()));
    }
    let mut csrf = tower_http::csrf::CsrfLayer::new();
    if let Some(origin) = public_origin {
        hosts.push(origin.authority);
        csrf = csrf
            .add_trusted_origin(&origin.origin)
            .expect("PublicOrigin is a canonical HTTPS origin");
    }
    let state = AppState {
        hosts,
        manager,
        token,
        auth,
        port: listen.port(),
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
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/session", get(session))
        .route("/api/state", get(snapshot))
        .route("/api/events", get(events))
        .route("/api/processes/{id}/logs", get(logs))
        .route("/api/processes/{id}/actions", post(action))
        .layer(DefaultBodyLimit::max(8192))
        .layer(middleware::from_fn_with_state(state.clone(), protect))
        .layer(csrf)
        .layer(middleware::map_response(security_headers))
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(access::request_span)
                .on_request(())
                .on_response(access::response)
                .on_failure(()),
        )
        .with_state(state)
}
fn asset(content_type: &'static str, bytes: &'static [u8]) -> Response {
    ([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response()
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
async fn protect(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers();
    let host = headers.get("host").and_then(|value| value.to_str().ok());
    if !state
        .hosts
        .iter()
        .any(|allowed| Some(allowed.as_str()) == host)
    {
        access::record_rejection("host", headers);
        return error(StatusCode::FORBIDDEN, "request host rejected");
    }
    let changes_state = !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    );
    let is_login = request.uri().path() == "/api/login";
    let authenticated = headers
        .get_all("cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find(|(name, _)| *name == format!("procshelf_{}", state.port))
        .and_then(|(_, value)| state.auth.authenticate(value));
    let csrf_ok = if is_login {
        headers.get("x-procshelf-login").is_some_and(|v| v == "1")
    } else {
        !changes_state
            || headers
                .get("x-procshelf-token")
                .is_some_and(|v| v.as_bytes() == state.token.as_bytes())
    };
    if request.uri().path().starts_with("/api/") && !is_login && authenticated.is_none() {
        error(StatusCode::UNAUTHORIZED, "authentication required")
    } else if !csrf_ok {
        error(StatusCode::FORBIDDEN, "request token rejected")
    } else {
        if let Some(session) = authenticated {
            request.extensions_mut().insert(session);
        }
        next.run(request).await
    }
}
async fn security_headers(mut response: Response) -> Response {
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    access_key: String,
}
async fn login(State(state): State<AppState>, Json(request): Json<LoginRequest>) -> Response {
    match state.auth.login(&request.access_key) {
        Ok(session) => (
            StatusCode::NO_CONTENT,
            [("set-cookie", session_cookie(&session, state.port))],
        )
            .into_response(),
        Err(LoginError::Rejected) => error(StatusCode::UNAUTHORIZED, "invalid access key"),
        Err(LoginError::RateLimited) => (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "60")],
            Json(json!({"error":"too many login attempts or active sessions"})),
        )
            .into_response(),
        Err(LoginError::Unavailable) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "authentication unavailable",
        ),
    }
}
fn session_cookie(session: &AuthenticatedSession, port: u16) -> String {
    format!(
        "procshelf_{port}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        session.token(),
        Authentication::SESSION_SECONDS
    )
}
async fn logout(
    State(state): State<AppState>,
    Extension(session): Extension<AuthenticatedSession>,
) -> Response {
    state.auth.logout(&session);
    (
        StatusCode::NO_CONTENT,
        [(
            "set-cookie",
            format!(
                "procshelf_{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
                state.port
            ),
        )],
    )
        .into_response()
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
    Extension(session): Extension<AuthenticatedSession>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Each event is a complete snapshot. A lagging or reconnecting client never needs to replay a partial state history.
    let changes = tokio_stream::StreamExt::throttle(
        WatchStream::new(state.manager.subscribe()),
        Duration::from_millis(150),
    );
    let auth = state.auth.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
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
    // A logged-out or expired session must also lose an already-open event stream.
    tokio::spawn(async move {
        tokio::pin!(snapshots);
        let mut check = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                _ = check.tick() => { if !auth.active(&session) { break; } },
                event = snapshots.next() => {
                    let Some(event) = event else { break; };
                    if !auth.active(&session) { break; }
                    tokio::select! {
                        result = tx.send(event) => { if result.is_err() { break; } },
                        _ = tokio::time::sleep(Duration::from_secs(1)) => break,
                    }
                }
            }
        }
    });
    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
