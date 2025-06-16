use std::{env, time::Duration};

use anyhow::Result;
use apalis::prelude::*;
use apalis_cron::CronStream;
use axum::Router;
use chrono::Local;
use config::CONFIG;
use opentelemetry::global;
use tower_http::trace::TraceLayer;

use crate::sweep::{sweep_healthchecks, HealthCheckSweepService};

pub mod config;
mod otel;
mod sweep;
mod trace_layer;
mod util;

#[tokio::main]
async fn main() -> Result<()> {
    let _guard = otel::setup_tracing_subscriber();

    let meter = global::meter(env!("CARGO_PKG_NAME"));
    let gauge = meter
        .u64_gauge(CONFIG.gauge_name.clone())
        .with_description("")
        .build();

    let app = Router::new()
        .layer(
            TraceLayer::new_for_http()
                .on_request(trace_layer::trace_layer_on_request)
                .on_response(trace_layer::trace_layer_on_response)
                .make_span_with(trace_layer::trace_layer_make_span_with),
        )
        .nest("/health", util::health_router())
        .fallback(util::not_found);

    let checks = CONFIG.health_checks.clone();
    tracing::debug!("{:#?}", *CONFIG);
    tracing::info!(
        "Sweeping [{}] healthchecks on schedule [{}], first sweep is scheduled for [{}]",
        checks.len(),
        CONFIG.sweep_schedule,
        CONFIG
            .sweep_schedule
            .upcoming(Local)
            .next()
            .expect("there should be an upcoming sweep scheduled")
    );
    tracing::debug!("{:#?}", checks);
    let worker = WorkerBuilder::new("healthchecks")
        .enable_tracing()
        .data(HealthCheckSweepService::new(checks, gauge))
        .rate_limit(1, Duration::from_secs(1))
        .timeout(Duration::from_secs(10))
        .backend(CronStream::new_with_timezone(
            CONFIG.sweep_schedule.clone(),
            Local,
        ))
        .build_fn(sweep_healthchecks);

    let listener = tokio::net::TcpListener::bind(CONFIG.service_address)
        .await
        .unwrap();

    tracing::info!(
        "Listening on: {}",
        &listener
            .local_addr()
            .expect("listener has a valid local address")
    );

    let monitor = async { worker.run().await };
    let serve = async { axum::serve(listener, app).await.unwrap() };

    let _res = tokio::join!(serve, monitor);

    Ok(())
}
