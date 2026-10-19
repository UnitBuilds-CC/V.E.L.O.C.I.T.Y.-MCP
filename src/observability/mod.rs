//! Observability module for OpenTelemetry integration
//!
//! This module exports distributed tracing spans over OTLP using OpenTelemetry.
//! It bridges the `tracing` crate to a collector; there is no metrics pipeline.
//!
//! # Example
//!
//! ```no_run
//! use velocity_mcp::observability::init_observability;
//!
//! // Initialize OpenTelemetry with OTLP export
//! let _guard = init_observability("http://localhost:4317").expect("Failed to initialize observability");
//!
//! // Tracing spans recorded after this point are exported to the collector.
//! // Keep `_guard` alive for as long as spans should be exported.
//! ```

#[cfg(feature = "observability")]
use opentelemetry::global;
#[cfg(feature = "observability")]
use opentelemetry::trace::TracerProvider as _;
#[cfg(feature = "observability")]
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
#[cfg(feature = "observability")]
use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};
#[cfg(feature = "observability")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "observability")]
use tracing_subscriber::util::SubscriberInitExt;

/// Guard that shuts down OpenTelemetry when dropped
///
/// Holds the provider so the exporter is flushed on drop: `global` no longer
/// owns a shutdown entry point, and a cloned `SdkTracerProvider` keeps span
/// processing alive until every reference goes away.
#[cfg(feature = "observability")]
pub struct ObservabilityGuard {
    provider: SdkTracerProvider,
}

#[cfg(feature = "observability")]
impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        let _ = self.provider.shutdown();
    }
}

/// Initialize OpenTelemetry with OTLP export
///
/// # Arguments
///
/// * `otlp_endpoint` - The OTLP endpoint URL (e.g., "http://localhost:4317")
///
/// # Returns
///
/// A guard that will shut down OpenTelemetry when dropped
///
/// # Example
///
/// ```no_run
/// use velocity_mcp::observability::init_observability;
///
/// let _guard = init_observability("http://localhost:4317").expect("Failed to initialize");
/// ```
#[cfg(feature = "observability")]
pub fn init_observability(
    otlp_endpoint: &str,
) -> Result<ObservabilityGuard, Box<dyn std::error::Error>> {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name(env!("CARGO_PKG_NAME"))
                .build(),
        )
        .build();

    // Registered globally so libraries that resolve a tracer through `global`
    // export through this pipeline too.
    global::set_tracer_provider(provider.clone());

    let telemetry_layer =
        tracing_opentelemetry::layer().with_tracer(provider.tracer(env!("CARGO_PKG_NAME")));

    // `init()` panics when a global subscriber already exists; this returns
    // `Result`, so the failure has to travel back through it instead.
    tracing_subscriber::registry()
        .with(telemetry_layer)
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()?;

    Ok(ObservabilityGuard { provider })
}
