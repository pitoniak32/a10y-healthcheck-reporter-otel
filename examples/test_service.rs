use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing_log::log;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Serialize)]
struct HealthCheckResult {
    status: HealthCheckStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthCheckStatus {
    Down,
    Up,
    Degraded,
    OutOfService,
    Unknown,
}

impl HealthCheckStatus {
    fn from_str(status: &str) -> Self {
        match status.to_lowercase().as_str() {
            "0" | "down" => Self::Down,
            "1" | "up" => Self::Up,
            "2" | "degraded" => Self::Degraded,
            "3" | "outofservice" => Self::OutOfService,
            _ => Self::Unknown,
        }
    }
}

pub fn health_router(state: HealthCheckStatus) -> Router {
    Router::new()
        .route("/", get(healthcheck))
        .with_state(state)
        .route("/up", get(healthcheck))
        .with_state(HealthCheckStatus::Up)
        .route("/down", get(healthcheck))
        .with_state(HealthCheckStatus::Down)
        .route("/degraded", get(healthcheck))
        .with_state(HealthCheckStatus::Degraded)
        .route("/out", get(healthcheck))
        .with_state(HealthCheckStatus::OutOfService)
        .route("/unknown", get(healthcheck))
        .with_state(HealthCheckStatus::Unknown)
        .route("/400", get(bad_request))
        .route("/500", get(server_error))
}

pub async fn bad_request() -> Result<Response, StatusCode> {
    Ok((StatusCode::BAD_REQUEST, "bad").into_response())
}

pub async fn server_error() -> Result<Response, StatusCode> {
    Ok((StatusCode::INTERNAL_SERVER_ERROR, "error").into_response())
}

pub async fn healthcheck(State(state): State<HealthCheckStatus>) -> Result<Response, StatusCode> {
    let status = state.clone();
    log::info!("{status:?} - healthcheck has been called!");
    Ok((StatusCode::OK, Json(HealthCheckResult { status })).into_response())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .init();

    let health: HealthCheckStatus =
        HealthCheckStatus::from_str(&std::env::var("HEALTH").unwrap_or("unknown".to_string()));

    let listener = tokio::net::TcpListener::bind((
        "0.0.0.0",
        std::env::var("EXAMPLE_SERVICE_PORT")
            .unwrap_or("3000".to_string())
            .parse::<u16>()
            .unwrap(),
    ))
    .await
    .unwrap();

    let app = Router::new().nest("/_manage/health", health_router(health.clone()));

    log::info!(
        "[HEALTH: {:?}] - Listening on: {}",
        health,
        listener
            .local_addr()
            .expect("listener has a valid local address"),
    );

    axum::serve(listener, app).await.unwrap();

    Ok(())
}
