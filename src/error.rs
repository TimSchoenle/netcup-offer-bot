use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Tracing error")]
    Logger(#[from] tracing::metadata::ParseLevelError),
    #[error("Config error: {0}")]
    ConfigVar(#[from] std::env::VarError),
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
    #[error("Custom: {0}")]
    Custom(String),
}

fn is_expected_feed_parse_message(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("did not begin with an rss tag")
}

impl Error {
    pub fn custom<T: ToString>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }

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
