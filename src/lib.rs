//! Watches the netcup deals RSS feed and posts new offers to a Discord webhook.
//!
//! The library half of the binary in `src/main.rs`. One round of work is
//! [`FeedChecker::check_feeds`] and the process is that call on an interval, so a round can be
//! driven from a test without a process around it.
//!
//! # What an item's identity is
//!
//! Its publication date. Each feed carries a watermark, the newest `pubDate` posted so far, and an
//! item dated at or before it is skipped. The `guid` is never read: an offer republished under a
//! new date is posted a second time, and an offer edited in place is not posted again.
//!
//! # Failure posture
//!
//! Nothing that happens inside a round stops the process. A feed that will not fetch and a webhook
//! that refuses delivery are logged and counted, in `feed_fetch_errors_total` and
//! `webhook_errors_total`. The payload netcup returns for an empty deals list and an item carrying
//! no usable date are logged and counted nowhere. The failures that end the process are all at
//! boot: the configuration load, the watermark file read, and the exporter binding its port.
//!
//! The watermark advances when an item is selected, not when it is delivered. An item whose five
//! delivery attempts all fail is counted in `webhook_errors_total` and never tried again.
//!
//! # One round
//!
//! ```no_run
//! # use netcup_offer_bot::{FeedChecker, config::Config};
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = Config::load()?;
//! let mut checker = FeedChecker::from_config(&config);
//! checker.check_feeds().await;
//! # Ok(())
//! # }
//! ```

#[macro_use]
extern crate tracing;

use std::fmt::Debug;

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_tracing::{SpanBackendWithUrl, TracingMiddleware};

use crate::config::Config;
use crate::discord_webhook::DiscordWebhook;
use crate::error::Error;
use crate::feed::Feed;
use crate::feed_state::FeedStates;

pub mod config;
pub mod discord_webhook;
mod error;
pub mod feed;
mod feed_state;
mod metrics;
pub mod telemetry;

/// The outcome of anything in this crate that can fail.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything one poll round needs, carried between rounds.
///
/// The watermarks live in memory here and reach disk at the end of [`check_feeds`](Self::check_feeds),
/// so a checker dropped mid-round leaves the file holding the previous round's dates.
#[derive(Debug)]
pub struct FeedChecker {
    client: ClientWithMiddleware,
    states: FeedStates,
    hook: DiscordWebhook,
}

impl FeedChecker {
    /// Builds a checker over an already-built client, watermark set and webhook.
    #[must_use]
    pub fn new(client: ClientWithMiddleware, states: FeedStates, webhook: DiscordWebhook) -> Self {
        Self {
            client,
            states,
            hook: webhook,
        }
    }

    /// Builds a checker whose watermarks come from `./data/feed_state.json` and whose webhook
    /// comes from the configuration.
    ///
    /// The file not existing is the first-run case and not a failure: the directory is created and
    /// the checker starts with no watermark.
    ///
    /// # Panics
    /// Panics if the file exists and cannot be read or holds JSON that is not a watermark set, or
    /// if `./data` does not exist and cannot be created.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(TracingMiddleware::<SpanBackendWithUrl>::new())
            .build();
        // Unwrapped deliberately. The image runs as `1001:1001`, and a volume owned by another
        // user should fail the boot here, not start a round with no watermark and repost the
        // whole feed.
        let states = FeedStates::load().unwrap();
        let hook = DiscordWebhook::new(config.discord.webhook_url.clone());

        FeedChecker::new(client, states, hook)
    }

    /// Polls every feed once, then writes the watermarks back.
    ///
    /// Every failure a round can produce is logged and counted where it happens, except the save,
    /// which is logged only. A round that fails to save keeps its dates in memory and tries again
    /// at the end of the next one.
    #[tracing::instrument]
    pub async fn check_feeds(&mut self) {
        trace!("Run feed check");

        for feed in Feed::iter() {
            self.check_feed(feed).await;
        }

        if let Err(e) = self.states.save().await {
            error!("Error saving feed states: {}", e);
        }
    }

    /// Polls one feed and posts every item newer than its watermark, in the order the feed listed
    /// them.
    ///
    /// The watermark advances past every item that was selected, so an item that then fails all
    /// its delivery attempts is counted in `webhook_errors_total` and not seen again.
    ///
    /// A payload that does not begin with an `rss` tag is logged at `WARN` and left out of
    /// `feed_fetch_errors_total`. netcup answers with one when the deals list is empty, and at
    /// `ERROR` the tracing layer would raise a Sentry issue for it every time.
    #[tracing::instrument]
    pub async fn check_feed(&mut self, feed: Feed) {
        debug!("Checking feed {}", feed.name());

        let _timer = metrics::get_feed_fetch_duration()
            .with_label_values(&[feed.name()])
            .start_timer();

        match feed.fetch(&self.client).await {
            Ok(feed_result) => {
                trace!("Found {} items for feed", feed_result.items.len());
                let items = self.states.get_new_feed(feed, feed_result.items);
                if items.is_empty() {
                    debug!("No new items found");
                    return;
                }

                debug!("Found {} new items", items.len());

                let counter = metrics::get_feed_counter().with_label_values(&[feed.name()]);
                counter.inc_by(items.len() as u64);

                for item in items {
                    if let Err(e) = self.hook.send_discord_message(&feed, item).await {
                        error!("Error sending message for feed {}: {}", feed.name(), e);
                        metrics::get_webhook_errors()
                            .with_label_values(&[feed.name()])
                            .inc();
                    }
                }
            }
            Err(e) => {
                if e.is_expected_feed_parse_error() {
                    warn!("Skipping malformed feed payload for {}: {}", feed.name(), e);
                } else {
                    error!("Error fetching feed for {}: {}", feed.name(), e);
                    metrics::get_feed_fetch_errors()
                        .with_label_values(&[feed.name()])
                        .inc();
                }
            }
        }
    }
}
