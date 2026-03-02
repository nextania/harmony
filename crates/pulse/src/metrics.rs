//! Lazy-initialised OpenTelemetry instruments for the pulse service.
//!
//! All instruments are created from the *global* MeterProvider, so they will
//! export data only after `common::telemetry::init_telemetry` has registered
//! the provider (which happens in `main()` before any connection is accepted).

use once_cell::sync::Lazy;
use opentelemetry::{
    global,
    metrics::{Counter, UpDownCounter},
};

fn meter() -> opentelemetry::metrics::Meter {
    global::meter("pulse")
}

/// Raw bytes received from a WebTransport datagram (before reassembly).
pub static DATAGRAM_BYTES_RECEIVED: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.datagram.bytes_received")
        .with_description("Total bytes received from producers via WebTransport datagrams")
        .with_unit("By")
        .build()
});

/// Bytes forwarded to each consuming session.
pub static DATAGRAM_BYTES_SENT: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.datagram.bytes_sent")
        .with_description("Total bytes forwarded to consumers via WebTransport datagrams")
        .with_unit("By")
        .build()
});

/// Number of complete (reassembled) datagrams received from producers.
pub static DATAGRAM_RECEIVED: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.datagram.received")
        .with_description("Number of reassembled datagrams received from producers")
        .build()
});

/// Number of datagram deliveries to individual consumer sessions.
pub static DATAGRAM_SENT: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.datagram.sent")
        .with_description("Number of datagram fan-out deliveries to consumer sessions")
        .build()
});

/// Number of datagrams dropped (failed to forward).
pub static DATAGRAM_DROPPED: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.datagram.dropped")
        .with_description("Number of datagram fan-out failures (send error or oversized)")
        .build()
});

/// Successfully reassembled fragmented payloads.
pub static FRAGMENT_ASSEMBLED: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.fragment.assembled")
        .with_description("Number of fragmented payloads successfully reassembled")
        .build()
});

/// Fragments discarded by the FragmentAssembler (TTL expiry or decode error).
pub static FRAGMENT_DROPPED: Lazy<Counter<u64>> = Lazy::new(|| {
    meter()
        .u64_counter("pulse.fragment.dropped")
        .with_description("Number of fragments discarded (TTL expiry or deserialise error)")
        .build()
});

/// Number of live WebTransport sessions (connections).
pub static CONNECTIONS_ACTIVE: Lazy<UpDownCounter<i64>> = Lazy::new(|| {
    meter()
        .i64_up_down_counter("pulse.connections.active")
        .with_description("Number of live WebTransport sessions")
        .build()
});

/// Number of calls with at least one participant.
pub static CALLS_ACTIVE: Lazy<UpDownCounter<i64>> = Lazy::new(|| {
    meter()
        .i64_up_down_counter("pulse.calls.active")
        .with_description("Number of active voice/video calls")
        .build()
});
