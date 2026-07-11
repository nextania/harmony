//! Lazy-initialised OpenTelemetry instruments for the pulse service.
//!
//! All instruments are created from the *global* MeterProvider, so they will
//! export data only after `common::telemetry::init_telemetry` has registered
//! the provider (which happens in `main()` before any connection is accepted).

use std::sync::LazyLock;

use opentelemetry::{global, metrics::UpDownCounter};

fn meter() -> opentelemetry::metrics::Meter {
    global::meter("pulse")
}

// TODO: `StatsHandle` in moq-net

/// Number of live MoQ sessions (connections).
pub static CONNECTIONS_ACTIVE: LazyLock<UpDownCounter<i64>> = LazyLock::new(|| {
    meter()
        .i64_up_down_counter("pulse.connections.active")
        .with_description("Number of live MoQ sessions")
        .build()
});

/// Number of calls with at least one participant.
pub static CALLS_ACTIVE: LazyLock<UpDownCounter<i64>> = LazyLock::new(|| {
    meter()
        .i64_up_down_counter("pulse.calls.active")
        .with_description("Number of active voice/video calls")
        .build()
});
