use std::time::Duration;

use reqwest::StatusCode;
use rss::Item;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use tokio::time::sleep;

use crate::error::Error;
use crate::feed::Feed;
use crate::Result;

#[derive(Debug)]
pub struct DiscordWebhook {
    url: SecretString,
    client: reqwest::Client,
}

impl DiscordWebhook {
    pub fn new(url: SecretString) -> Self {
        DiscordWebhook {
            url,
            client: reqwest::Client::new(),
        }
    }

    #[tracing::instrument(skip(self, feed, item))]
    pub async fn send_discord_message(&self, feed: &Feed, item: Item) -> Result<bool> {
        info!(
            "Sending message for feed {} with title \"{}\"",
            feed.name(),
            item.title().unwrap_or("No title")
        );

        let embed = self.build_embed(&item);
        let payload = self.build_payload(feed, embed);

        self.send_with_retry(&payload).await
    }

    fn build_embed(&self, item: &Item) -> serde_json::Value {
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

    fn build_payload(&self, feed: &Feed, embed: serde_json::Value) -> serde_json::Value {
        json!({
            "username": format!("Feed - {}", feed.name()),
            "embeds": [embed]
        })
    }

    async fn send_with_retry(&self, payload: &serde_json::Value) -> Result<bool> {
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 5;

        loop {
            attempts += 1;
            let response = self
                .client
                .post(self.url.expose_secret())
                .json(payload)
                .send()
                .await;

            match response {
                Ok(res) => {
                    if res.status().is_success() {
                        return Ok(true);
                    } else if res.status() == StatusCode::TOO_MANY_REQUESTS {
                        if attempts >= MAX_ATTEMPTS {
                            return Err(Error::custom(
                                "Max retries exceeded for rate limit".to_string(),
                            ));
                        }

                        let wait_seconds = if let Some(h) = res.headers().get("retry-after") {
                            if let Ok(s) = h.to_str() {
                                s.parse::<f64>().unwrap_or(1.0) as u64
                            } else {
                                1
                            }
                        } else {
                            1
                        };

                        warn!(
                            "Rate limited. Waiting for {} seconds before retry {}/{}",
                            wait_seconds, attempts, MAX_ATTEMPTS
                        );
                        sleep(Duration::from_secs(wait_seconds + 1)).await; // Add buffer
                        continue;
                    } else if res.status().is_server_error() {
                        if attempts >= MAX_ATTEMPTS {
                            return Err(Error::custom(format!("Server error: {}", res.status())));
                        }
                        warn!(
                            "Server error {}. Retrying {}/{}",
                            res.status(),
                            attempts,
                            MAX_ATTEMPTS
                        );
                        sleep(Duration::from_secs(2u64.pow(attempts))).await; // Exponential backoff
                        continue;
                    } else {
                        return Err(Error::custom(format!(
                            "Failed to send webhook: {}",
                            res.status()
                        )));
                    }
                }
                Err(e) => {
                    if attempts >= MAX_ATTEMPTS {
                        return Err(Error::custom(e.to_string()));
                    }
                    warn!(
                        "Network error: {}. Retrying {}/{}",
                        e, attempts, MAX_ATTEMPTS
                    );
                    sleep(Duration::from_secs(2u64.pow(attempts))).await;
                }
            }
        }
    }
}
