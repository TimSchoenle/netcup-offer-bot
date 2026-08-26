//! One error type over everything a round can hit, so this crate matches on one type and not on
//! five libraries' error types.

use thiserror::Error;

/// Anything that went wrong between reading the configuration and posting an item.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Config error: {0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("Parser error")]
    Parse(#[from] std::num::ParseIntError),
    #[error("Rss error: {0}")]
    Rss(#[from] rss::Error),
    #[error("Rss validation error: {0}")]
    RssValidation(#[from] rss::validation::ValidationError),
    #[error("Reqwest error")]
    Reqwest(#[from] reqwest::Error),
    #[error("Reqwest middleware error")]
    ReqwestMiddleware(#[from] reqwest_middleware::Error),
    #[error("Tokio error")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("IO error")]
    IO(#[from] std::io::Error),
    #[error("Serde error")]
    Serde(#[from] serde_json::Error),
    #[error("Prometheus error")]
    Prometheus(#[from] prometheus::Error),
    #[error("Prometheus exporter error")]
    PrometheusExport(#[from] prometheus_exporter::Error),
    /// `telemetry.sentry` is switched on and unusable: no DSN, a DSN that does not parse, a rate
    /// outside `0.0..=1.0`, or a binary built without the `sentry` feature. A boot failure rather
    /// than a warning, because the alternative is a process nobody knows has stopped reporting.
    #[error("Sentry error: {0}")]
    Sentry(String),
    /// A `tracing` subscriber was already installed. Only reachable from a second boot inside one
    /// process, which is a test harness rather than a deployment.
    #[error("Tracing error: {0}")]
    Tracing(String),
    /// An unsuccessful HTTP status, or retries running out.
    #[error("Custom: {0}")]
    Custom(String),
}

/// Reports whether a parse failure is the one netcup produces when the deals list is empty.
///
/// Matched on the rendered message rather than on the `rss::Error::InvalidStartTag` behind it, so
/// a release of `rss` that rewords the sentence turns this into `false` and puts the empty-feed
/// case back into Sentry. Case-folded because the wording belongs to that crate and is not part of
/// its API.
fn is_expected_feed_parse_message(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("did not begin with an rss tag")
}

impl Error {
    /// Builds an error carrying a message and nothing else.
    pub fn custom(msg: impl Into<String>) -> Self {
        Self::Custom(msg.into())
    }

    /// Reports whether this is the empty-feed payload rather than a fetch worth counting.
    ///
    /// [`FeedChecker::check_feed`](crate::FeedChecker::check_feed) logs a `true` at `WARN`, which
    /// keeps it out of Sentry and out of `feed_fetch_errors_total`.
    pub fn is_expected_feed_parse_error(&self) -> bool {
        match self {
            Self::Rss(err) => is_expected_feed_parse_message(&err.to_string()),
            _ => false,
        }
    }
}

impl From<&str> for Error {
    fn from(err: &str) -> Self {
        Self::Custom(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, is_expected_feed_parse_message};

    #[test]
    fn identifies_expected_rss_parse_message() {
        assert!(is_expected_feed_parse_message(
            "the input did not begin with an rss tag"
        ));
    }

    #[test]
    fn ignores_unrelated_rss_parse_message() {
        assert!(!is_expected_feed_parse_message("invalid date"));
    }

    #[test]
    fn does_not_mark_custom_error_as_expected_parse_error() {
        assert!(!Error::custom("boom").is_expected_feed_parse_error());
    }
}
