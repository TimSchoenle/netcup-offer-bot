//! The feeds this process watches, and the fetch that turns one into a parsed channel.
//!
//! The set is compiled in. No configuration key names a feed, so a second source is a code change
//! that also decides what the new variant is called in the metrics and in Discord.

use std::fmt;

use reqwest_middleware::ClientWithMiddleware;
use rss::Channel;
use rss::validation::Validate;
use serde::{Deserialize, Serialize};

/// One RSS source, with its address.
///
/// Serialised as the key of the watermark file, so renaming a variant leaves a file that no
/// longer parses and panics the boot in
/// [`FeedChecker::from_config`](crate::FeedChecker::from_config).
#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize, Eq, Hash)]
pub enum Feed {
    /// The netcup deals feed, in German.
    Netcup,
}

impl Feed {
    /// Returns the feed's label, used as the `feed` value on every metric and as the name
    /// Discord shows the post under.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Feed::Netcup => "Netcup",
        }
    }

    /// Returns the address the feed is fetched from.
    #[must_use]
    pub fn url(self) -> &'static str {
        match self {
            Feed::Netcup => "https://www.netcup.com/rss/deals/de",
        }
    }

    /// Iterates every feed, in declaration order.
    pub fn iter() -> impl Iterator<Item = Self> {
        [Feed::Netcup].into_iter()
    }

    /// Fetches the feed and returns it parsed and validated.
    ///
    /// The whole body is read into memory before parsing starts, and nothing here sets a timeout,
    /// so a stalled connection stalls the round it is in for as long as `client` allows.
    ///
    /// # Errors
    /// Fails if the request does not complete, if the body does not parse as RSS, or if the
    /// document fails RSS validation. netcup answers an empty deals list with a body that does not
    /// begin with an `rss` tag, and [`FeedChecker::check_feed`](crate::FeedChecker::check_feed) is
    /// the one place that tells that apart from a fetch worth counting.
    #[tracing::instrument]
    pub async fn fetch(&self, client: &ClientWithMiddleware) -> crate::Result<Channel> {
        let content = client.get(self.url()).send().await?.bytes().await?;
        let channel = Channel::read_from(&content[..])?;
        channel.validate()?;
        Ok(channel)
    }
}

impl fmt::Display for Feed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
