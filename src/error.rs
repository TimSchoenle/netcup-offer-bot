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

impl Error {
    pub fn custom<T: ToString>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

impl From<&str> for Error {
    fn from(err: &str) -> Self {
        Self::Custom(err.to_string())
    }
}
