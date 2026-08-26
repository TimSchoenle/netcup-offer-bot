//! The subscriber this process logs through, and the Sentry client that shares its record stream.
//!
//! One call, [`init`], because the two are one installation: the layer reports onto the client,
//! and the client has to exist before the subscriber that feeds it. Both are process-global and
//! installed once, which is why `telemetry.*` is the block a running process cannot be told to
//! re-read.
//!
//! # Two filters, on purpose
//!
//! stdout is filtered at [`log_level`](crate::config::TelemetryConfig::log_level) and Sentry at
//! its own thresholds, and neither governs the other. The layer goes on collecting `INFO`
//! breadcrumbs however quiet stdout has been told to be, so an issue raised from an `ERROR`
//! still arrives with the round that led to it attached — which is the whole value of a
//! breadcrumb trail and would be lost the moment one filter sat above both.

mod sentry;

use std::fmt::{self, Debug, Formatter};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, filter};

use crate::Result;
use crate::config::TelemetryConfig;
use crate::error::Error;

/// Keeps the Sentry client alive, and flushes what it has queued when it drops.
///
/// Returned rather than stashed in a static because a static is never dropped: the flush that
/// gets a shutting-down process's last events out happens here, bounded by
/// `telemetry.sentry.shutdown_timeout_secs`. Bind it for the lifetime of `main` — `let _ = …`
/// drops it at the end of the statement and closes the client before the process has done
/// anything worth reporting.
#[must_use = "dropping the guard closes the Sentry client and stops reporting"]
pub struct TelemetryGuard(sentry::Guard);

/// Reports whether a client is installed, and nothing else.
///
/// Written by hand because `sentry::ClientInitGuard` has no `Debug` of its own, and because the
/// one thing worth printing about this type is the answer to "is this process reporting".
impl Debug for TelemetryGuard {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TelemetryGuard")
            .field(&sentry::is_active(&self.0))
            .finish()
    }
}

/// Installs the subscriber, and the Sentry client when one is configured.
///
/// Order is load-bearing: the client is installed first, so the SDK's panic hook is in place
/// before the subscriber is built and so the layer below has something to report onto.
///
/// # Errors
/// Fails if `telemetry.sentry` is switched on but unusable — no DSN, a DSN that does not parse,
/// a sample rate outside `0.0..=1.0`, or a binary built without the `sentry` feature — and if a
/// subscriber is already installed. All of them are boot failures rather than warnings: each
/// one's only other outcome is a process that runs while reporting nothing.
pub fn init(config: &TelemetryConfig) -> Result<TelemetryGuard> {
    let guard = sentry::init(&config.sentry)?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(filter::LevelFilter::from_level(config.log_level)),
        )
        .with(sentry::tracing_layer(&config.sentry))
        .try_init()
        .map_err(|e| Error::Tracing(e.to_string()))?;

    // After `try_init`, not beside the client: a record emitted before the subscriber exists
    // goes nowhere, and "is this deployment actually reporting" is the first question asked of
    // a process that has stopped raising issues.
    if sentry::is_active(&guard) {
        info!(
            traces_sample_rate = config.sentry.traces_sample_rate,
            capture_level = ?config.sentry.capture_level,
            breadcrumb_level = ?config.sentry.breadcrumb_level,
            "Sentry reporting enabled"
        );
    } else {
        info!("telemetry.sentry.enabled is not set, nothing is reported to Sentry");
    }

    Ok(TelemetryGuard(guard))
}
