use std::env;

use anyhow::Result;
use axum::Router;
use config::CONFIG;
use opentelemetry::{global, metrics::ObservableGauge, KeyValue};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

pub mod config;
mod otel;
mod trace_layer;
mod util;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub url: String,
    pub metadata: HealthCheckMetaData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckMetaData {
    pub component: String,
    pub datacenter: String,
    pub environment: String,
    pub feature: String,
    pub system: String,
    pub team: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub status: HealthCheckStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthCheckStatus {
    Down,
    Up,
    Degraded,
    OutOfService,
    Unknown,
}

impl From<HealthCheckStatus> for u64 {
    fn from(val: HealthCheckStatus) -> Self {
        match val {
            HealthCheckStatus::Down => 0,
            HealthCheckStatus::Up => 1,
            HealthCheckStatus::Degraded => 2,
            HealthCheckStatus::OutOfService => 3,
            HealthCheckStatus::Unknown => 4,
        }
    }
}

fn register_healthchecks() -> Vec<ObservableGauge<u64>> {
    let meter = global::meter(env!("CARGO_PKG_NAME"));

    let checks = CONFIG.health_checks.clone();
    tracing::info!("Registering {} healthchecks", checks.len());
    tracing::debug!("{:#?}", checks);

    checks
        .clone()
        .into_iter()
        .map(|hc| {
            meter
                .u64_observable_gauge(CONFIG.gauge_name.clone())
                .with_description("")
                .with_callback(move |observer| {
                    let code = run_check(hc.clone());
                    observer.observe(
                        code,
                        &[
                            KeyValue::new("team", hc.metadata.team.clone()),
                            KeyValue::new("feature", hc.metadata.feature.clone()),
                        ],
                    );
                })
                .build()
        })
        .collect::<Vec<_>>()
}

fn run_check(hc: HealthCheck) -> u64 {
    let check = hc.clone();
    let url = hc.url.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    tokio::task::spawn_blocking(move || {
        let result = reqwest::blocking::get(url);
        let hc_result = aquire_hc_result(result);
        tx.send(hc_result).unwrap();
    });
    let status = rx.recv().unwrap().status;
    let code = status.clone().into();
    println!(
        "recording [{status:?} - {code}] - {} : {}",
        check.metadata.feature, check.url
    );
    code
}

fn aquire_hc_result(
    response_result: Result<reqwest::blocking::Response, reqwest::Error>,
) -> HealthCheckResult {
    match response_result {
        Ok(res) => {
            let status = res.status();
            let body = res.json::<HealthCheckResult>();
            match body {
                Ok(hc_result) => hc_result,
                Err(_) => HealthCheckResult {
                    status: match status.as_u16() {
                        200..299 => HealthCheckStatus::Up,
                        _ => HealthCheckStatus::Down,
                    },
                },
            }
        }
        Err(err) => {
            dbg!(&err);
            match err.status() {
                Some(status) => {
                    println!("{}", status);
                }
                None => {
                    println!("not status related");
                }
            };
            HealthCheckResult {
                status: HealthCheckStatus::Unknown,
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().expect("dotenv to work");

    let _guard = otel::setup_tracing_subscriber();

    tracing::debug!("{:#?}", *CONFIG);

    let _gauges = register_healthchecks();

    let app = Router::new()
        .layer(
            TraceLayer::new_for_http()
                .on_request(trace_layer::trace_layer_on_request)
                .on_response(trace_layer::trace_layer_on_response)
                .make_span_with(trace_layer::trace_layer_make_span_with),
        )
        .nest("/health", util::health_router())
        .fallback(util::not_found);

    let listener = tokio::net::TcpListener::bind(CONFIG.service_address)
        .await
        .unwrap();

    tracing::info!(
        "Listening on: {}",
        listener
            .local_addr()
            .expect("listener has a valid local address")
    );
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
