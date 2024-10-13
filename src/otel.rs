use opentelemetry::{
    global::{self},
    trace::TracerProvider,
    KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    metrics::{
        data::Temporality, reader::TemporalitySelector, InstrumentKind, MeterProviderBuilder,
        PeriodicReader, SdkMeterProvider,
    },
    runtime,
    trace::{BatchConfig, RandomIdGenerator, Sampler, Tracer},
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
    let tracer = init_tracer();
    let meter_provider = init_meter_provider();

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

    OtelGuard { meter_provider }
}

// Construct Tracer for OpenTelemetryLayer
fn init_tracer() -> Tracer {
    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                // Customize sampling strategy
                .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
                    1.0,
                ))))
                // If export trace to AWS X-Ray, you can use XrayIdGenerator
                .with_id_generator(RandomIdGenerator::default())
                .with_resource(resource()),
        )
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(CONFIG.collector_uri.clone()),
        )
        .with_batch_config(BatchConfig::default())
        .install_batch(runtime::Tokio)
        .expect("opentelemetry tracer to configure correctly");

    global::set_tracer_provider(tracer_provider.clone());

    tracer_provider.tracer("tracing-otel-subscriber")
}

// Construct MeterProvider for MetricsLayer
fn init_meter_provider() -> SdkMeterProvider {
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(CONFIG.collector_uri.clone())
        .build_metrics_exporter(Box::new(DeltaTemporalitySelector::new()))
        .unwrap();

    let reader = PeriodicReader::builder(exporter, runtime::Tokio)
        .with_interval(CONFIG.metrics_interval)
        .build();

    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(reader)
        .build();

    global::set_meter_provider(meter_provider.clone());

    meter_provider
}

pub struct OtelGuard {
    meter_provider: SdkMeterProvider,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        println!("dropping otel guard!");
        if let Err(err) = self.meter_provider.shutdown() {
            eprintln!("{err:?}");
        }
        opentelemetry::global::shutdown_tracer_provider();
    }
}

/// A temporality selector that returns [`Delta`][Temporality::Delta] for all
/// instruments except `UpDownCounter` and `ObservableUpDownCounter`.
///
/// This temporality selector is equivalent to OTLP Metrics Exporter's
/// `Delta` temporality preference (see [its documentation][exporter-docs]).
///
/// [exporter-docs]: https://github.com/open-telemetry/opentelemetry-specification/blob/a1c13d59bb7d0fb086df2b3e1eaec9df9efef6cc/specification/metrics/sdk_exporters/otlp.md#additional-configuration
#[derive(Clone, Default, Debug)]
pub struct DeltaTemporalitySelector {
    pub(crate) _private: (),
}

impl DeltaTemporalitySelector {
    /// Create a new default temporality selector.
    pub fn new() -> Self {
        Self::default()
    }
}

impl TemporalitySelector for DeltaTemporalitySelector {
    #[rustfmt::skip]
    fn temporality(&self, kind: InstrumentKind) -> Temporality {
        match kind {
            InstrumentKind::Counter
            | InstrumentKind::Histogram
            | InstrumentKind::ObservableCounter
            | InstrumentKind::Gauge
            | InstrumentKind::ObservableGauge => {
                Temporality::Delta
            }
            InstrumentKind::UpDownCounter
            | InstrumentKind::ObservableUpDownCounter => {
                Temporality::Cumulative
            }
        }
    }
}
