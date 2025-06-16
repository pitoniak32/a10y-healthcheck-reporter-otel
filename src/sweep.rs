use std::sync::Arc;

use anyhow::Result;
use apalis::prelude::*;
use apalis_cron::CronContext;
use chrono::Local;
use opentelemetry::{metrics::Gauge, KeyValue};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use tracing_log::log;

use crate::config::CONFIG;

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

impl HealthCheckStatus {
    fn as_u64(&self) -> u64 {
        match self {
            HealthCheckStatus::Down => 0,
            HealthCheckStatus::Up => 1,
            HealthCheckStatus::Degraded => 2,
            HealthCheckStatus::OutOfService => 3,
            HealthCheckStatus::Unknown => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthCheckSweepService {
    gauge: Gauge<u64>,
    checks: Vec<Arc<HealthCheck>>,
}

impl HealthCheckSweepService {
    pub fn new(checks: Vec<Arc<HealthCheck>>, gauge: Gauge<u64>) -> Self {
        Self { gauge, checks }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let mut join_set = JoinSet::<HealthCheckResult>::new();

        self.checks.iter().for_each(|hc| {
            let gauge = self.gauge.clone();
            let hc = hc.clone();

            join_set.spawn(async move {
                let result = acquire_hc_result(reqwest::get(&hc.url).await, &hc.metadata).await;
                let status = result.clone().status;
                gauge.record(
                    status.as_u64(),
                    &[
                        KeyValue::new("team", hc.metadata.team.clone()),
                        KeyValue::new("feature", hc.metadata.feature.clone()),
                    ],
                );
                result.clone()
            });
        });

        // join all async check tasks
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(hc_res) => if matches!(hc_res.status, HealthCheckStatus::Unknown) {},
                Err(err) => {
                    tracing::error!("encountered error during join: {err}")
                }
            }
        }

        Ok(())
    }
}

async fn acquire_hc_result(
    response_result: Result<reqwest::Response, reqwest::Error>,
    metadata: &HealthCheckMetaData,
) -> HealthCheckResult {
    match response_result {
        Ok(res) => {
            let status = res.status();
            let body = res.json::<HealthCheckResult>().await;
            let result = match body {
                Ok(hc_result) => hc_result,
                Err(_) => HealthCheckResult {
                    status: match status.as_u16() {
                        200..299 => HealthCheckStatus::Up,
                        _ => HealthCheckStatus::Down,
                    },
                },
            };
            log::info!(
                "[{}]: {} - {:?}",
                metadata.feature,
                result.status.as_u64(),
                &result.status,
            );
            result
        }
        Err(err) => {
            match err.status() {
                Some(status) => {
                    log::debug!("error was generated from a Response, status was {}", status)
                }
                None => log::debug!("error was not generated from a Response"),
            };
            let result = HealthCheckResult {
                status: HealthCheckStatus::Unknown,
            };
            log::error!(
                "[{}]: {} - {:?}",
                metadata.feature,
                result.status.as_u64(),
                result.status,
            );
            result
        }
    }
}

#[derive(Debug, Default)]
pub struct Sweep;

pub async fn sweep_healthchecks(
    _: Sweep,
    svc: Data<HealthCheckSweepService>,
    ctx: CronContext<Local>,
) -> anyhow::Result<()> {
    svc.run().await?;

    log::info!(
        "sweep completed at [{}], next scheduled for [{}]",
        ctx.get_timestamp(),
        CONFIG
            .sweep_schedule
            .upcoming(Local)
            .next()
            .expect("there should be an upcoming sweep scheduled")
    );

    Ok(())
}
