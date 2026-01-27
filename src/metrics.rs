use std::sync::OnceLock;

use prometheus::{HistogramVec, IntCounterVec, register_histogram_vec, register_int_counter_vec};

pub static FEED_COUNTER: OnceLock<IntCounterVec> = OnceLock::new();
pub static FEED_FETCH_DURATION: OnceLock<HistogramVec> = OnceLock::new();
pub static FEED_FETCH_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();
pub static WEBHOOK_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();

pub fn get_feed_counter() -> &'static IntCounterVec {
    FEED_COUNTER.get_or_init(|| {
        register_int_counter_vec!("feed_counter", "Number of send feeds", &["feed"])
            .expect("Failed to register feed counter metric")
    })
}

pub fn get_feed_fetch_duration() -> &'static HistogramVec {
    FEED_FETCH_DURATION.get_or_init(|| {
        register_histogram_vec!(
            "feed_fetch_duration_seconds",
            "Duration of feed fetch in seconds",
            &["feed"]
        )
        .expect("Failed to register feed fetch duration metric")
    })
}

pub fn get_feed_fetch_errors() -> &'static IntCounterVec {
    FEED_FETCH_ERRORS.get_or_init(|| {
        register_int_counter_vec!(
            "feed_fetch_errors_total",
            "Number of failed feed fetches",
            &["feed"]
        )
        .expect("Failed to register feed fetch errors metric")
    })
}

pub fn get_webhook_errors() -> &'static IntCounterVec {
    WEBHOOK_ERRORS.get_or_init(|| {
        register_int_counter_vec!(
            "webhook_errors_total",
            "Number of failed webhook sends",
            &["feed"]
        )
        .expect("Failed to register webhook errors metric")
    })
}
