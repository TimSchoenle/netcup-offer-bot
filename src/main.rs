//! The process the container runs: a boot sequence, then one round per tick.
//!
//! Boot order is the load-bearing part. The configuration is read before the tracing subscriber
//! exists, because the log level is one of the keys it carries, so a failure there is reported by
//! `main`'s `Termination` and by nothing else. Everything installed after it is installed once: a
//! changed log level or a rotated Sentry DSN reaches the process on the next restart and not
//! before.
//!
//! Nothing after boot ends the process. A round logs and counts whatever it hits, and the interval
//! stream has no end, so the `Ok(())` below is unreachable and the container exits only when it is
//! stopped or when it panics.
//!
//! Ticks keep tokio's default burst behaviour: a round that runs longer than the interval is
//! followed immediately by the next one instead of being skipped.

#[macro_use]
extern crate tracing;

use netcup_offer_bot::FeedChecker;
use netcup_offer_bot::Result;
use netcup_offer_bot::config::{self, Config};
use secrecy::{ExposeSecret, SecretString};
use sentry::ClientInitGuard;
use std::net::SocketAddr;
use tokio::time;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::IntervalStream;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, filter};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;

    setup_tracing(config.telemetry.log_level);

    log_configuration_layers(config.telemetry.log_level);

    // Bound for the whole of `main`: the guard flushes queued events when it drops, and a `_`
    // binding would drop it at the end of this statement.
    let _sentry = setup_sentry(config.telemetry.sentry_dsn.as_ref());

    setup_metrics(&config.metrics.socket())?;

    info!("Starting feed bot");
    let mut checker = FeedChecker::from_config(&config);
    let mut stream = IntervalStream::new(time::interval(config.feed.check_interval()));
    while let Some(_ts) = stream.next().await {
        checker.check_feeds().await;
    }

    Ok(())
}

/// Installs the subscriber: stdout at `level`, Sentry at `DEBUG` and above.
///
/// The two filters are independent on purpose. Sentry's layer keeps collecting `DEBUG` events as
/// breadcrumbs however quiet stdout is, so an issue raised from an `ERROR` still arrives with the
/// round that led to it attached.
fn setup_tracing(level: tracing::Level) {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter::LevelFilter::from_level(level)))
        .with(sentry::integrations::tracing::layer().with_filter(filter::LevelFilter::DEBUG))
        .init();
}

/// Reports which layer supplied each configuration key.
///
/// The answer to "the `Secret` is mounted and the bot is still posting to the old webhook": the
/// mount is listed, and so is the stale environment variable sitting on top of it. The report
/// holds no configuration value — never recorded, rather than redacted on the way out — so
/// there is nothing in it a log should not have.
///
/// Assembled only at the levels that would print it, because it re-reads every layer to build
/// it. A failure is not fatal: the process has already loaded the configuration it needs, and
/// losing the explanation of it is not a reason to refuse to start.
fn log_configuration_layers(level: tracing::Level) {
    if level < tracing::Level::DEBUG {
        return;
    }

    match config::explain() {
        Ok(layers) => debug!("Configuration layers:\n{}", layers),
        Err(e) => warn!("Could not explain the configuration layers: {}", e),
    }
}

/// Initialises Sentry, or returns `None` when no DSN is configured.
///
/// The guard has to outlive everything worth reporting: dropping it flushes what is queued, and
/// that is the only flush this process performs. One transaction in five is sampled, and the
/// release is whatever `sentry::release_name!` reads out of the build.
fn setup_sentry(dsn: Option<&SecretString>) -> Option<ClientInitGuard> {
    let Some(dsn) = dsn else {
        info!("telemetry.sentry_dsn not set, skipping Sentry setup");
        return None;
    };

    let mut options = sentry::ClientOptions::new()
        .traces_sample_rate(0.2)
        .attach_stacktrace(true);
    if let Some(release) = sentry::release_name!() {
        options = options.release(release);
    }

    Some(sentry::init((dsn.expose_secret(), options)))
}

/// Starts the Prometheus exporter on `socket`.
///
/// # Errors
/// Fails if the address is already in use or cannot be bound. `main` propagates it, so a port
/// clash stops the boot rather than leaving the process running without metrics.
fn setup_metrics(socket: &SocketAddr) -> Result<()> {
    prometheus_exporter::start(*socket)?;
    Ok(())
}
