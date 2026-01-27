#[macro_use]
extern crate tracing;

use netcup_offer_bot::config::Config;
use netcup_offer_bot::FeedChecker;
use netcup_offer_bot::Result;
use sentry::ClientInitGuard;
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::time;
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::StreamExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter, Layer};

const ENV_SENTRY_DSN: &str = "SENTRY_DSN";
const ENV_LOG_LEVEL: &str = "LOG_LEVEL";

const DEFAULT_LOG_LEVEL: &str = "info";

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing()?;

    let dns = env::var(ENV_SENTRY_DSN).ok();
    // Prevents the process from exiting until all events are sent
    let _sentry = setup_sentry(dns);

    let config = Config::get_configurations()?;

    setup_metrics(&config.metric_socket)?;

    info!("Starting feed bot");
    let mut checker = FeedChecker::from_config(&config);
    let mut stream = IntervalStream::new(time::interval(config.check_interval));
    while let Some(_ts) = stream.next().await {
        checker.check_feeds().await;
    }

    Ok(())
}

fn setup_tracing() -> Result<()> {
    let level = env::var(ENV_LOG_LEVEL).unwrap_or_else(|_| DEFAULT_LOG_LEVEL.to_string());
    let level = tracing::Level::from_str(&level)?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter::LevelFilter::from_level(level)))
        .with(sentry::integrations::tracing::layer().with_filter(filter::LevelFilter::DEBUG))
        .init();

    Ok(())
}

fn setup_sentry(dns: Option<String>) -> Option<ClientInitGuard> {
    // Only enable sentry if the dns is set
    let dns = match dns {
        Some(dns) => dns,
        None => {
            info!("{ENV_SENTRY_DSN} not set, skipping Sentry setup");
            return None;
        }
    };

    // Sentry innit
    Some(sentry::init((
        dns,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            traces_sample_rate: 0.2,
            attach_stacktrace: true,
            ..Default::default()
        },
    )))
}

fn setup_metrics(socket: &SocketAddr) -> Result<()> {
    prometheus_exporter::start(*socket)?;
    Ok(())
}
