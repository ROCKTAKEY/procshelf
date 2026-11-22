use axum::{
    body::Body,
    http::{HeaderMap, Request},
    response::Response,
};
use std::time::Duration;
use tracing::{Span, field};

pub(super) fn request_span(request: &Request<Body>) -> Span {
    tracing::info_span!(
        "http.request",
        method = %request.method(),
        path = request.uri().path(),
        reason = field::Empty,
        host = field::Empty,
        origin = field::Empty,
        "sec-fetch-site" = field::Empty,
    )
}

pub(super) fn record_rejection(reason: &str, headers: &HeaderMap) {
    let span = Span::current();
    span.record("reason", reason);
    for name in ["host", "origin", "sec-fetch-site"] {
        let values: Vec<_> = headers.get_all(name).iter().collect();
        span.record(name, field::debug(values));
    }
}

pub(super) fn response(response: &Response, _latency: Duration, span: &Span) {
    if let Some(reason) = response
        .extensions()
        .get::<tower_http::csrf::ProtectionError>()
    {
        span.record("reason", field::display(reason));
    }
    tracing::info!(status = response.status().as_u16(), "HTTP response");
}
