//! Delivery to Discord. One RSS item becomes one embed, and the retry policy wraps the POST.
//!
//! A `429` waits out the `retry-after` header, a `5xx` or a dropped connection backs off
//! exponentially, and any other unsuccessful status fails the item without a retry.

use std::time::Duration;

use reqwest::{Response, StatusCode};
use rss::Item;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tokio::time::sleep;

use crate::Result;
use crate::error::Error;
use crate::feed::Feed;

/// Attempts one item gets before it is given up on.
///
/// Five spends 2, 4, 8 and 16 seconds of backoff, so half a minute of Discord being unavailable
/// costs the item that was in flight.
const MAX_ATTEMPTS: u32 = 5;

/// Waited out when Discord rate limits without saying for how long.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(1);

/// Added to whatever `retry-after` asks for, so a wait rounded down by either side does not land
/// the retry back inside the window.
const RETRY_AFTER_BUFFER: Duration = Duration::from_secs(1);

/// One Discord webhook, ready to post to.
#[derive(Debug)]
pub struct DiscordWebhook {
    url: SecretString,
    // A plain client rather than the checker's traced one: `SpanBackendWithUrl` records the
    // request URL on the span, and here the URL is the credential.
    client: reqwest::Client,
}

impl DiscordWebhook {
    /// Builds a webhook that posts to `url`.
    #[must_use]
    pub fn new(url: SecretString) -> Self {
        DiscordWebhook {
            url,
            client: reqwest::Client::new(),
        }
    }

    /// Posts one item as an embed carrying its title, description, link, publication date and
    /// categories.
    ///
    /// Returns `true`, always: an undelivered item is an error rather than `false`. Sleeps
    /// between attempts, so a Discord that keeps answering `5xx` holds this for thirty seconds
    /// before it gives up, and a `429` holds it for whatever `retry-after` asks for, uncapped.
    ///
    /// # Errors
    /// Fails when all five attempts have been spent on rate limits, server errors or connection
    /// failures, and immediately on any other unsuccessful status. A `400` from a malformed embed
    /// is in the second group and is not retried.
    #[tracing::instrument(skip(self, feed, item))]
    pub async fn send_discord_message(&self, feed: &Feed, item: Item) -> Result<bool> {
        info!(
            "Sending message for feed {} with title \"{}\"",
            feed.name(),
            item.title().unwrap_or("No title")
        );

        let embed = build_embed(&item);
        let payload = build_payload(*feed, &embed);

        self.send_with_retry(&payload).await
    }

    /// Posts the payload until it lands or the attempts run out.
    async fn send_with_retry(&self, payload: &serde_json::Value) -> Result<bool> {
        let mut attempts = 0;

        loop {
            attempts += 1;
            let backoff = match self
                .client
                // codeql[rust/cleartext-transmission]
                .post(self.url.expose_secret())
                .json(payload)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(true),
                Ok(response) if response.status() == StatusCode::TOO_MANY_REQUESTS => {
                    if attempts >= MAX_ATTEMPTS {
                        return Err(Error::custom("Max retries exceeded for rate limit"));
                    }

                    let wait = retry_after(&response) + RETRY_AFTER_BUFFER;
                    warn!(
                        "Rate limited. Waiting for {:?} before retry {}/{}",
                        wait, attempts, MAX_ATTEMPTS
                    );
                    wait
                }
                Ok(response) if response.status().is_server_error() => {
                    if attempts >= MAX_ATTEMPTS {
                        return Err(Error::custom(format!(
                            "Server error: {}",
                            response.status()
                        )));
                    }

                    warn!(
                        "Server error {}. Retrying {}/{}",
                        response.status(),
                        attempts,
                        MAX_ATTEMPTS
                    );
                    backoff(attempts)
                }
                Ok(response) => {
                    return Err(Error::custom(format!(
                        "Failed to send webhook: {}",
                        response.status()
                    )));
                }
                Err(e) => {
                    if attempts >= MAX_ATTEMPTS {
                        return Err(Error::custom(e.to_string()));
                    }

                    warn!(
                        "Network error: {}. Retrying {}/{}",
                        e, attempts, MAX_ATTEMPTS
                    );
                    backoff(attempts)
                }
            };

            sleep(backoff).await;
        }
    }
}

/// Builds the embed for one item, with `No title` and `No description` standing in for what the
/// item left out.
fn build_embed(item: &Item) -> serde_json::Value {
    let mut embed = json!({
        "title": item.title().unwrap_or("No title"),
        "description": item.description().unwrap_or("No description"),
    });

    if let Some(url) = item.link() {
        embed["url"] = json!(url);
    }

    let mut fields = Vec::new();

    if let Some(date) = item.pub_date() {
        fields.push(json!({
            "name": "Date",
            "value": date,
            "inline": false
        }));
    }

    let categories = item
        .categories()
        .iter()
        .map(|category| category.name.clone())
        .collect::<Vec<String>>()
        .join(", ");
    if !categories.is_empty() {
        fields.push(json!({
            "name": "Categories",
            "value": categories,
            "inline": false
        }));
    }

    if !fields.is_empty() {
        embed["fields"] = json!(fields);
    }

    embed
}

/// Wraps the embed in a payload posted under the feed's name, not the webhook's own.
fn build_payload(feed: Feed, embed: &serde_json::Value) -> serde_json::Value {
    json!({
        "username": format!("Feed - {}", feed.name()),
        "embeds": [embed]
    })
}

/// Reads `retry-after`, or [`DEFAULT_RETRY_AFTER`] when the header is missing or is not a number
/// of seconds.
///
/// Fractions are kept. Truncating a `0.75` to zero would retry inside the window the header
/// describes.
fn retry_after(response: &Response) -> Duration {
    response
        .headers()
        .get("retry-after")
        .and_then(|header| header.to_str().ok())
        .and_then(|value| value.parse::<f64>().ok())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .unwrap_or(DEFAULT_RETRY_AFTER)
}

/// Doubles from two seconds: two on the first retry, then four, eight and sixteen.
fn backoff(attempts: u32) -> Duration {
    Duration::from_secs(2u64.pow(attempts))
}
