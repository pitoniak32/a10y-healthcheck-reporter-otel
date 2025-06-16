use std::{net::IpAddr, str::FromStr, sync::Arc, time::Duration};

use apalis_cron::Schedule;
use once_cell::sync::Lazy;

use crate::sweep::HealthCheck;

const SERVICE_IP_ENV_KEY: &str = "SERVICE_IP";
const PORT_ENV_KEY: &str = "PORT";
const A10Y_HEALTH_CHECKS_ENV_KEY: &str = "A10Y_HEALTH_CHECKS";

const SWEEP_SCHEDULE_CRON_ENV_KEY: &str = "SWEEP_SCHEDULE_CRON";

const OTEL_METRIC_READER_INTERVAL_SECS_ENV_KEY: &str = "OTEL_METRIC_READER_INTERVAL_SECS";
const GAUGE_MEASUREMENT_NAME_ENV_KEY: &str = "GAUGE_MEASUREMENT_NAME";

pub struct Secret {
    pub value: String,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("value", &"<REDACTED>")
            .finish()
    }
}

#[derive(Debug)]
pub struct Config {
    pub sweep_schedule: Schedule,
    pub service_address: (IpAddr, u16),
    pub collector_uri: String,
    pub metrics_interval: Duration,
    pub gauge_name: String,
    pub health_checks: Vec<Arc<HealthCheck>>,
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let metrics_interval = Duration::from_secs(
        std::env::var(OTEL_METRIC_READER_INTERVAL_SECS_ENV_KEY)
            .map(|interval| {
                interval
                    .parse::<u64>()
                    .expect("provided interval to be a valid u64")
            })
            .unwrap_or(5),
    );

    let gauge_name =
        std::env::var(GAUGE_MEASUREMENT_NAME_ENV_KEY).unwrap_or("a10y.status_code".to_string());
    tracing::trace!("{}={}", GAUGE_MEASUREMENT_NAME_ENV_KEY, gauge_name);

    let a10y_hc_str = std::env::var(A10Y_HEALTH_CHECKS_ENV_KEY).unwrap_or("[]".to_string());
    tracing::trace!("{}={}", A10Y_HEALTH_CHECKS_ENV_KEY, a10y_hc_str);

    let health_checks: Vec<HealthCheck> =
        serde_json::from_str(&a10y_hc_str).expect("Provided HealthChecks should be valid");

    let service_ip = std::env::var(SERVICE_IP_ENV_KEY)
        .map(|addr| IpAddr::from_str(&addr).expect("provided ip address to be valid"))
        .unwrap_or(IpAddr::from_str("0.0.0.0").expect("should be a vaild default ip address"));

    let port = std::env::var(PORT_ENV_KEY)
        .map(|port| port.parse::<u16>().expect("provided port to be valid"))
        .unwrap_or(3000);

    let sweep_schedule = Schedule::from_str(
        &std::env::var(SWEEP_SCHEDULE_CRON_ENV_KEY).unwrap_or("*/15 * * * * *".to_string()),
    )
    .expect("sweep schedule should be a valid cron expression");

    Config {
        service_address: (service_ip, port),
        collector_uri: "grpc://localhost:4317".to_string(),
        metrics_interval,
        gauge_name,
        health_checks: health_checks.into_iter().map(Arc::new).collect(),
        sweep_schedule,
    }
});
