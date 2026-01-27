use std::collections::HashMap;
use std::path::Path;

use chrono::serde::ts_seconds_option;
use chrono::{DateTime, FixedOffset, Utc};
use rss::Item;
use serde::{Deserialize, Serialize};

use crate::feed::Feed;

const FEED_STATE_FILE: &str = "./data/feed_state.json";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct FeedStates {
    feeds: HashMap<Feed, FeedState>,
}

impl FeedStates {
    pub fn load() -> crate::Result<Self> {
        let path = Path::new(FEED_STATE_FILE);
        FeedStates::load_from_path(path)
    }

    #[tracing::instrument]
    fn load_from_path(file: &Path) -> crate::Result<Self> {
        if file.exists() {
            info!("Loading feed state from file");

            let content = std::fs::read_to_string(file)?;
            serde_json::from_str(&content).map_err(|e| e.into())
        } else {
            // Ensure that the path exists
            let prefix = file.parent().ok_or("Invalid FEED_SATE_FILE path")?;
            std::fs::create_dir_all(prefix)?;

            Ok(Self {
                feeds: HashMap::new(),
            })
        }
    }

    fn un_dirty(&mut self) {
        for state in self.feeds.values_mut() {
            state.dirty = false;
        }
    }

    pub fn get_feed_or_create(&mut self, feed: &Feed) -> &mut FeedState {
        self.feeds.entry(*feed).or_default()
    }

    #[tracing::instrument]
    pub fn get_new_feed(&mut self, feed: &Feed, items: Vec<Item>) -> Vec<Item> {
        if items.is_empty() {
            return items;
        }

        let mut last_date = None;
        let mut sorted = Vec::new();

        let feed_state = self.get_feed_or_create(feed);

        for item in items {
            match &item.pub_date {
                Some(date) => {
                    let date = DateTime::parse_from_rfc2822(date);
                    match date {
                        Ok(date) => {
                            if feed_state.is_before(&date) {
                                trace!("Skipping item, already seen {:?}", date);
                                continue;
                            }

                            trace!("Found new item {:?}", date);

                            if last_date.is_none() || date > last_date.unwrap() {
                                last_date = Some(date);
                            }
                            sorted.push(item);
                        }
                        Err(e) => {
                            error!("Error parsing date for feed {}: {}", feed.name(), e);
                        }
                    }
                }
                None => {
                    info!("Skipping item without date on feed {}", feed.name());
                }
            }
        }

        // Store new last date if found
        if let Some(date) = last_date {
            // Convert to UTC
            let date = date.with_timezone(&Utc);
            feed_state.set_last_update(date);
        }

        sorted
    }

    pub fn is_dirty(&self) -> bool {
        self.feeds.values().any(|state| state.dirty)
    }

    #[tracing::instrument]
    pub async fn save(&mut self) -> crate::Result<()> {
        self.save_to_path(Path::new(FEED_STATE_FILE)).await
    }

    #[tracing::instrument]
    async fn save_to_path(&mut self, file: &Path) -> crate::Result<()> {
        if !self.is_dirty() {
            debug!("Feed state is not dirty, skipping save");
            return Ok(());
        }

        debug!("Saving feed state to file");

        // Save to file
        tokio::fs::write(file, serde_json::to_string_pretty(self)?).await?;

        self.un_dirty();

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct FeedState {
    #[serde(with = "ts_seconds_option")]
    last_update: Option<DateTime<Utc>>,
    #[serde(skip_serializing, default)]
    dirty: bool,
}

impl FeedState {
    fn new(last_update: Option<DateTime<Utc>>, dirty: bool) -> Self {
        Self { last_update, dirty }
    }

    pub fn is_before(&self, date: &DateTime<FixedOffset>) -> bool {
        self.last_update
            .is_some_and(|last_update| *date <= last_update)
    }

    pub fn set_last_update(&mut self, date: DateTime<Utc>) {
        self.last_update = Some(date);
        self.dirty = true;
    }
}

impl Default for FeedState {
    fn default() -> Self {
        Self::new(None, false)
    }
}

#[cfg(test)]
mod tests_feed_states {
    use std::path::PathBuf;

    use chrono::Duration;
    use tempfile::{tempdir, TempDir};

    use super::*;

    struct TestFile {
        // Prevent it from being dropped
        #[allow(dead_code)]
        dir: TempDir,
        path: PathBuf,
    }

    fn create_temp_file() -> TestFile {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("file.json");

        TestFile {
            dir,
            path: file_path,
        }
    }

    fn create_empty_feed_states() -> FeedStates {
        FeedStates {
            feeds: HashMap::new(),
        }
    }

    fn create_feed_states(dirty: bool) -> FeedStates {
        let mut map = HashMap::new();
        map.insert(
            Feed::Netcup,
            FeedState::new(Some(get_current_utc_time()), dirty),
        );
        FeedStates { feeds: map }
    }

    fn create_rss_item(date: DateTime<Utc>) -> Item {
        Item {
            pub_date: Some(date.to_rfc2822()),
            ..Default::default()
        }
    }

    // Returns the current UTC time based of rfc2822
    // This is required format for our RSS items
    fn get_current_utc_time() -> DateTime<Utc> {
        let now = Utc::now();
        let rfc2822 = now.to_rfc2822();
        DateTime::parse_from_rfc2822(&rfc2822)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn test_load_no_file() {
        let test_file = create_temp_file();
        let config = FeedStates::load_from_path(&test_file.path).unwrap();
        assert!(config.feeds.is_empty());
    }

    #[test]
    fn test_load_with_file() {
        let test_file = create_temp_file();

        let mut map = HashMap::new();
        // Don't use UTC::now here since nano seconds are not saved during the serialization
        map.insert(
            Feed::Netcup,
            FeedState::new(Some(get_current_utc_time()), false),
        );
        let expected = FeedStates { feeds: map };
        std::fs::write(
            &test_file.path,
            serde_json::to_string_pretty(&expected).unwrap(),
        )
        .unwrap();

        let config = FeedStates::load_from_path(&test_file.path).unwrap();
        assert_eq!(config, expected);
    }

    #[test]
    fn test_un_dirty() {
        let mut feed_states = create_feed_states(true);

        feed_states.un_dirty();

        assert!(!feed_states.feeds[&Feed::Netcup].dirty);
    }

    #[test]
    fn test_get_feed_or_create() {
        let mut feed_states = create_empty_feed_states();

        let feed = Feed::Netcup;
        let new_time = get_current_utc_time();

        // Initial state
        {
            let state = feed_states.get_feed_or_create(&feed);
            assert_eq!(state, &FeedState::default());

            state.set_last_update(new_time);
        }

        // Check if state is updated
        {
            let state = feed_states.get_feed_or_create(&feed);
            assert_eq!(state.last_update, Some(new_time));
        }
    }

    #[test]
    fn test_get_new_feed_empty() {
        let mut feed_states = create_empty_feed_states();

        let items = feed_states.get_new_feed(&Feed::Netcup, Vec::new());
        assert!(items.is_empty());
    }

    #[test]
    fn test_get_new_feed_first_run() {
        let mut feed_states = create_empty_feed_states();

        let feed = Feed::Netcup;
        let highest_time = get_current_utc_time() + Duration::hours(1);
        let items = vec![
            create_rss_item(get_current_utc_time()),
            create_rss_item(highest_time),
        ];
        let filtered_items = feed_states.get_new_feed(&feed, items.clone());

        assert_eq!(items, filtered_items);
        assert_eq!(feed_states.feeds[&feed].last_update, Some(highest_time));
        assert!(feed_states.is_dirty());
    }

    #[test]
    fn test_get_new_feed_all_before() {
        let mut feed_states = create_feed_states(false);

        let feed = Feed::Netcup;
        let expected_time = feed_states.feeds[&feed].last_update;

        let mut items = Vec::new();
        for i in 1..10 {
            let time = get_current_utc_time() - Duration::hours(i);
            items.push(create_rss_item(time));
        }

        let filtered_items = feed_states.get_new_feed(&feed, items);

        assert!(filtered_items.is_empty());
        assert!(!feed_states.is_dirty());
        assert_eq!(feed_states.feeds[&feed].last_update, expected_time);
    }

    #[test]
    fn test_get_new_feed_all_after() {
        let mut feed_states = create_feed_states(false);

        let feed = Feed::Netcup;

        let mut items = Vec::new();
        let mut expected_time = None;
        for i in 1..10 {
            let time = get_current_utc_time() + Duration::hours(i);
            if expected_time.is_none() || time > expected_time.unwrap() {
                expected_time = Some(time);
            }
            items.push(create_rss_item(time));
        }

        let filtered_items = feed_states.get_new_feed(&feed, items.clone());

        assert!(feed_states.is_dirty());
        assert_eq!(filtered_items.len(), items.len());
        assert_eq!(filtered_items, items);
        assert_eq!(feed_states.feeds[&feed].last_update, expected_time);
    }

    #[test]
    fn test_get_new_feed_same_time() {
        let mut feed_states = create_feed_states(false);

        let feed = Feed::Netcup;

        let time = feed_states.feeds[&feed].last_update.unwrap();
        let items = vec![create_rss_item(time), create_rss_item(time)];

        let filtered_items = feed_states.get_new_feed(&feed, items.clone());

        assert!(!feed_states.is_dirty());
        assert!(filtered_items.is_empty());
    }

    #[test]
    fn test_get_new_feed() {
        let mut feed_states = create_feed_states(false);

        let feed = Feed::Netcup;
        let mut before = Vec::new();
        for i in 1..10 {
            let time = get_current_utc_time() - Duration::hours(i);
            before.push(create_rss_item(time));
        }

        let mut after = Vec::new();
        for i in 1..10 {
            let time = get_current_utc_time() + Duration::hours(i);
            after.push(create_rss_item(time));
        }

        let mut items = before.clone();
        items.append(&mut after.clone());

        let filtered_items = feed_states.get_new_feed(&feed, items.clone());

        assert!(feed_states.is_dirty());
        assert_eq!(filtered_items.len(), after.len());
        assert_eq!(filtered_items, after);
    }

    #[tokio::test]
    async fn test_save_no_dirty_empty() {
        let test_file = create_temp_file();

        let mut feed_states = create_empty_feed_states();

        feed_states.save_to_path(&test_file.path).await.unwrap();

        assert!(!test_file.path.exists());
    }

    #[tokio::test]
    async fn test_save_no_dirty() {
        let test_file = create_temp_file();

        let mut feed_states = create_feed_states(false);

        feed_states.save_to_path(&test_file.path).await.unwrap();

        assert!(!test_file.path.exists());
    }

    #[tokio::test]
    async fn test_save_dirty() {
        let test_file = create_temp_file();

        let mut feed_states = create_feed_states(true);

        feed_states.save_to_path(&test_file.path).await.unwrap();

        assert!(test_file.path.exists());
    }
}

#[cfg(test)]
mod tests_feed_state {
    use chrono::Duration;

    use super::*;

    fn get_current_time() -> DateTime<FixedOffset> {
        let now = Utc::now();
        let rfc2822 = now.to_rfc2822();
        DateTime::parse_from_rfc2822(&rfc2822).unwrap()
    }

    #[test]
    fn test_default() {
        let state = FeedState::default();

        assert_eq!(state.last_update, None);
        assert!(!state.dirty);
    }

    #[test]
    fn test_set_last_update() {
        let mut state = FeedState::default();

        let time = Utc::now();
        state.set_last_update(time);

        assert_eq!(state.last_update, Some(time));
        assert!(state.dirty);
    }

    #[test]
    fn test_is_before_none() {
        let state = FeedState::default();

        let time = get_current_time();
        assert!(!state.is_before(&time));
    }

    #[test]
    fn test_is_before_same_time() {
        let mut state = FeedState::default();

        let time = get_current_time();
        state.set_last_update(time.with_timezone(&Utc));

        assert!(state.is_before(&time));
    }

    #[test]
    fn test_is_before_before_time() {
        let mut state = FeedState::default();

        let time = get_current_time();
        state.set_last_update(time.with_timezone(&Utc));

        let check_time = time - Duration::hours(1);
        assert!(state.is_before(&check_time));
    }

    #[test]
    fn test_is_before_after_time() {
        let mut state = FeedState::default();

        let time = get_current_time();
        state.set_last_update(time.with_timezone(&Utc));

        let check_time = time + Duration::hours(1);
        assert!(!state.is_before(&check_time));
    }
}
