//! Delivery, driven against a mock Discord.
//!
//! What is pinned here is the retry loop, one branch per test: a `429` carrying `retry-after` is
//! waited out, a `5xx` backs off and retries, a `429` that never clears gives up after
//! `MAX_ATTEMPTS`, and any other `4xx` fails on the first response without sleeping. The webhook
//! client is `reqwest`'s own, so a mock server is the only way to reach the loop at all.
//!
//! The waits are real. `retry-after: 0` keeps the rate-limit tests to the one-second buffer per
//! attempt, and the backoff test allows a single retry because the second would sleep four seconds.

use netcup_offer_bot::discord_webhook::DiscordWebhook;
use rss::Item;
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_webhook_retry_on_429() {
    let mock_server = MockServer::start().await;

    // Expect 1st request to fail with 429, 2nd to succeed
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let webhook = DiscordWebhook::new(SecretString::from(mock_server.uri()));

    let mut item = Item::default();
    item.set_title(Some("Test Item".to_string()));
    item.set_description(Some("Test Description".to_string()));

    let feed = netcup_offer_bot::feed::Feed::Netcup;

    let result = webhook.send_discord_message(&feed, item).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Builds the item the tests post. The webhook reads the title and the description and nothing
/// else, so the values only have to be present.
fn test_item() -> Item {
    let mut item = Item::default();
    item.set_title(Some("Test Item".to_string()));
    item.set_description(Some("Test Description".to_string()));
    item
}

#[tokio::test]
async fn test_webhook_retries_server_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let webhook = DiscordWebhook::new(SecretString::from(mock_server.uri()));
    let result = webhook
        .send_discord_message(&netcup_offer_bot::feed::Feed::Netcup, test_item())
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_webhook_gives_up_when_rate_limit_never_clears() {
    let mock_server = MockServer::start().await;

    // Every attempt is rate limited, so the loop runs to MAX_ATTEMPTS and returns the error
    // rather than posting. `retry-after: 0` leaves only the one-second buffer per wait.
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .mount(&mock_server)
        .await;

    let webhook = DiscordWebhook::new(SecretString::from(mock_server.uri()));
    let result = webhook
        .send_discord_message(&netcup_offer_bot::feed::Feed::Netcup, test_item())
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_webhook_does_not_retry_client_error() {
    let mock_server = MockServer::start().await;

    // A 404 is the webhook URL being wrong, which no number of retries fixes. It has to fail on
    // the first response, so this test also proves the loop does not sleep on it.
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&mock_server)
        .await;

    let webhook = DiscordWebhook::new(SecretString::from(mock_server.uri()));
    let result = webhook
        .send_discord_message(&netcup_offer_bot::feed::Feed::Netcup, test_item())
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_webhook_rate_limited_without_retry_after_header() {
    let mock_server = MockServer::start().await;

    // No `retry-after`, so the wait falls back to DEFAULT_RETRY_AFTER plus the buffer instead of
    // retrying immediately inside the window Discord is describing.
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let webhook = DiscordWebhook::new(SecretString::from(mock_server.uri()));
    let result = webhook
        .send_discord_message(&netcup_offer_bot::feed::Feed::Netcup, test_item())
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap());
}
