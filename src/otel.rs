use std::time::Duration;

use opentelemetry::{
    global,
    trace::TracerProvider,
    KeyValue,
};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    metrics::{MeterProviderBuilder, PeriodicReader, SdkMeterProvider, Temporality},
    runtime,
    trace::{self as sdktrace, RandomIdGenerator, Sampler},
    Resource,
};
use opentelemetry_semantic_conventions::{
    resource::{DEPLOYMENT_ENVIRONMENT_NAME, SERVICE_NAME, SERVICE_VERSION},
    SCHEMA_URL,
};
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::CONFIG;

// Create a Resource that captures information about the entity for which telemetry is recorded.
fn resource() -> Resource {
    Resource::from_schema_url(
        [
            KeyValue::new(SERVICE_NAME, env!("CARGO_PKG_NAME")),
            KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
            KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, "develop"),
        ],
        SCHEMA_URL,
    )
}

pub fn setup_tracing_subscriber() -> OtelGuard {
    let tracer_provider = init_tracer_provider();
    let meter_provider = init_meter_provider();

    let tracer = tracer_provider.tracer("tracing-otel-subscriber");

    let filter = if std::env::var("RUST_LOG").is_ok() {
        EnvFilter::builder().from_env_lossy()
    } else {
        "info"
            .to_string()
            .parse()
            .expect("valid EnvFilter value can be parsed")
    };

    tracing_subscriber::registry()
        .with(filter) // Read global subscriber filter from `RUST_LOG`
        .with(tracing_subscriber::fmt::layer()) // Setup logging layer
        .with(MetricsLayer::new(meter_provider.clone()))
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    OtelGuard {
        tracer_provider,
        meter_provider,
    }
}

// Construct Tracer for OpenTelemetryLayer
fn init_tracer_provider() -> sdktrace::TracerProvider {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(CONFIG.collector_uri.clone())
        .build()
        .unwrap();
    let config = opentelemetry_sdk::trace::Config::default()
        // Customize sampling strategy
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            1.0,
        ))))
        // If export trace to AWS X-Ray, you can use XrayIdGenerator
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource());
    // let exporter = opentelemetry_stdout::SpanExporter::default();
    let tracer_provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_batch_exporter(exporter, runtime::Tokio)
        .with_config(config)
        // .with_config(opentelemetry_sdk::trace::Config::default().with_resource(resource()))
        .build();

    global::set_tracer_provider(tracer_provider.clone());

    return tracer_provider;
}

// Construct MeterProvider for MetricsLayer
fn init_meter_provider() -> SdkMeterProvider {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(CONFIG.collector_uri.clone())
        .with_temporality(Temporality::Delta)
        .build()
        .unwrap();

    let reader = PeriodicReader::builder(exporter, runtime::Tokio)
        .with_interval(CONFIG.metrics_interval)
        .with_timeout(Duration::from_secs(10))
        .build();

    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(reader)
        .build();

    global::set_meter_provider(meter_provider.clone());

    meter_provider
}

pub struct OtelGuard {
    tracer_provider: sdktrace::TracerProvider,
    meter_provider: SdkMeterProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        tracing::debug!("shutdown meter_provider!");
        self.meter_provider
            .shutdown()
            .expect("SdkMeterProvider should shutdown properly!");
        tracing::debug!("shutdown tracer_provider!");
        self.tracer_provider
            .shutdown()
            .expect("TracerProvider should shutdown properly!");
    }
}
