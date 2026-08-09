#[macro_use]
extern crate tracing;

use netcup_offer_bot::FeedChecker;
use netcup_offer_bot::Result;
use netcup_offer_bot::config::Config;
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
    // Loaded before anything is installed: every knob below, the log level included, is part of
    // the same layered configuration, so a failure here is reported by `main`'s `Termination`
    // rather than by a subscriber that does not exist yet.
    let config = Config::load()?;

    setup_tracing(config.telemetry.log_level);

    // Prevents the process from exiting until all events are sent
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

fn setup_tracing(level: tracing::Level) {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter::LevelFilter::from_level(level)))
        .with(sentry::integrations::tracing::layer().with_filter(filter::LevelFilter::DEBUG))
        .init();
}

fn setup_sentry(dsn: Option<&SecretString>) -> Option<ClientInitGuard> {
    // Only enable sentry if the dsn is set
    let dsn = match dsn {
        Some(dsn) => dsn,
        None => {
            info!("telemetry.sentry_dsn not set, skipping Sentry setup");
            return None;
        }
    };

    // Sentry innit
    let mut options = sentry::ClientOptions::new()
        .traces_sample_rate(0.2)
        .attach_stacktrace(true);
    if let Some(release) = sentry::release_name!() {
        options = options.release(release);
    }

    Some(sentry::init((dsn.expose_secret(), options)))
}

fn setup_metrics(socket: &SocketAddr) -> Result<()> {
    prometheus_exporter::start(*socket)?;
    Ok(())
}
