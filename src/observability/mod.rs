//! Observability module for OpenTelemetry integration
//!
//! This module provides distributed tracing and metrics export using OpenTelemetry.
//! It integrates with the `tracing` crate to automatically export spans and metrics.
//!
//! # Example
//!
//! ```no_run
//! use velocity_mcp::observability::init_observability;
//!
//! // Initialize OpenTelemetry with OTLP export
//! let _guard = init_observability("http://localhost:4317").expect("Failed to initialize observability");
//!
//! // Now all tracing spans will be exported to OpenTelemetry
//! ```

#[cfg(feature = "observability")]
use opentelemetry::global;
#[cfg(feature = "observability")]
use opentelemetry::trace::TracerProvider as _;
#[cfg(feature = "observability")]
use opentelemetry_otlp::WithExportConfig;
#[cfg(feature = "observability")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "observability")]
use tracing_subscriber::util::SubscriberInitExt;

/// Guard that shuts down OpenTelemetry when dropped
#[cfg(feature = "observability")]
pub struct ObservabilityGuard;

#[cfg(feature = "observability")]
impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        global::shutdown_tracer_provider();
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
pub fn init_observability(otlp_endpoint: &str) -> Result<ObservabilityGuard, Box<dyn std::error::Error>> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(otlp_endpoint),
        )
        .install_simple()?;

    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(telemetry_layer)
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    Ok(ObservabilityGuard)
}

/// Stub for when observability feature is not enabled
#[cfg(not(feature = "observability"))]
pub fn init_observability(_otlp_endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err("Observability feature not enabled. Compile with --features observability".into())
}
