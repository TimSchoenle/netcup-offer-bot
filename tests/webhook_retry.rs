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
