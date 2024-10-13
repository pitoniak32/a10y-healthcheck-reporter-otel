use std::time::Duration;

use axum::{body::Body, extract::Request, response::Response};
use opentelemetry_semantic_conventions::trace::{
    HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, NETWORK_PROTOCOL_NAME,
    NETWORK_PROTOCOL_VERSION, URL_PATH, URL_SCHEME,
};
use tracing::Span;

pub(crate) fn trace_layer_make_span_with(request: &Request<Body>) -> Span {
    let version_clone = format!("{:?}", request.version());
    let version: Vec<_> = version_clone.split("/").collect();

    tracing::info_span!("request",
        { URL_PATH } = %request.uri(),
        { URL_SCHEME } = request.uri().scheme_str(),
        { NETWORK_PROTOCOL_NAME } = version.first().map(|v| v.to_lowercase()),
        { NETWORK_PROTOCOL_VERSION } = version.get(1),
        { HTTP_REQUEST_METHOD } = %request.method(),
        // source = request.extensions()
        //     .get::<ConnectInfo<SocketAddr>>()
        //     .map(|connect_info|
        //         tracing::field::display(connect_info.ip().to_string()),
        //     ).unwrap_or_else(||
        //         tracing::field::display(String::from("<unknown>"))
        //     ),
        // Fields must be defined to be used, define them as empty if they populate later
        { HTTP_RESPONSE_STATUS_CODE } = tracing::field::Empty,
    )
}

pub(crate) fn trace_layer_on_request(_request: &Request<Body>, _span: &Span) {
    tracing::info!("started processing request");
}

pub(crate) fn trace_layer_on_response(
    response: &Response<axum::body::Body>,
    latency: Duration,
    span: &Span,
) {
    span.record(HTTP_RESPONSE_STATUS_CODE, response.status().as_u16() as i64);
    tracing::info!(
        latency = format!("{} ms", latency.as_millis()),
        status = response.status().as_u16() as i64,
        "finished processing request"
    );
}
