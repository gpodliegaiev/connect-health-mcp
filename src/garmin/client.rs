//! HTTP client for the Garmin personal API.
//!
//! `GarminClient` adds the bearer token to every request, retries once with a
//! new token if Garmin says the current one is no longer accepted, and turns
//! JSON responses into Rust types (see `types.rs`).

use std::time::Duration;

use anyhow::Context;
use reqwest::StatusCode;
// `NaiveDate` is a calendar date with no time zone, e.g. 2026-10-23.
use chrono::NaiveDate;
use serde::Serialize;
// `DeserializeOwned` is the trait for "a type serde can build from JSON
// without borrowing from the input text". Used as a bound on `get_json`.
use serde::de::DeserializeOwned;

// `super::` means "the parent module" (`garmin`), so this is
// `crate::garmin::auth::TokenManager`.
use super::{
    activity::{Activity, ActivitySummary},
    activity_detail::{ActivityDetail, ActivityDetailSummary},
    auth::{KeyCheck, TokenManager},
    body_battery::{BodyBatteryDay, BodyBatteryReport},
    daily_summary::{DailySummaries, DaySummary, UserDailySummary},
    fitness_age::{FitnessAge, FitnessAgeSummary},
    heart_rate_day::{DailyHeartRate, HeartRateDay},
    heart_rate_zones::{HeartRateZones, ZoneSet},
    hrv::{HrvDay, HrvNight, HrvRange},
    sleep::{DailySleep, SleepSummary},
    types::ActivityTypeFilter,
    weight::{WeightGoals, WeightRange, WeightReport},
};
use crate::config::Config;

/// Root of every Garmin API URL. Fixed on purpose: the API key and bearer
/// token are sent here, so it must not be configurable (a `.env` file in a
/// cloned project could otherwise redirect them to another server).
const BASE_URL: &str = "https://personal.connectapi.garmin.com";

/// Query parameters for the activity search endpoint, in Garmin's units.
///
/// `Serialize` lets reqwest turn it into URL parameters. `camelCase` gives
/// Garmin's spelling (`activityType`, `minDistance`), and every `None` filter
/// is left out of the URL entirely instead of being sent as an empty value.
///
/// Numbers are whole (`u64`): Garmin answers 404 to `minDistance=5000.5`.
/// All ranges are inclusive. `Default` gives "no filters" (`start` 0,
/// `limit` 0, every filter `None`), so callers can write
/// `ActivitiesQuery { limit: 5, ..Default::default() }`.
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitiesQuery {
    /// Number of activities to skip (for paging).
    pub start: u32,
    /// Maximum number of activities to return.
    pub limit: u32,
    /// Only this sport group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_type: Option<ActivityTypeFilter>,
    /// Earliest local start date. chrono's `serde` feature writes a
    /// `NaiveDate` as `YYYY-MM-DD`, the format Garmin expects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<NaiveDate>,
    /// Latest local start date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    /// Minimum distance, in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_distance: Option<u64>,
    /// Maximum distance, in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_distance: Option<u64>,
    /// Minimum timer duration (paused time excluded), in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_duration: Option<u64>,
    /// Maximum timer duration, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_duration: Option<u64>,
    /// Minimum total elevation gain, in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_elevation: Option<u64>,
    /// Maximum total elevation gain, in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_elevation: Option<u64>,
}

/// Give up on a request if Garmin does not answer within this time.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Path of the activity search endpoint.
const ACTIVITIES_PATH: &str = "/activitylist-service/activities/search/activities";

/// Path of the daily sleep endpoint. Requires a `date=YYYY-MM-DD` parameter.
const DAILY_SLEEP_PATH: &str = "/sleep-service/sleep/dailySleep";

/// Path of the sleep range endpoint. Requires `startDate` and `endDate`.
const SLEEP_RANGE_PATH: &str = "/sleep-service/sleep/dailySleepSummariesByDate";

/// Authenticated access to the Garmin API.
pub struct GarminClient {
    /// Connection pool used for every request.
    http: reqwest::Client,
    /// Provides (and caches) the bearer token.
    tokens: TokenManager,
}

impl GarminClient {
    /// Builds a client from the loaded configuration.
    ///
    /// Takes `&Config` (a borrow) because it only needs to read the config,
    /// not take ownership of it.
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        // One `reqwest::Client` for the whole program, so connections are
        // reused between requests.
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // Garmin's API never redirects, so a redirect is refused rather
            // than followed: the key and token then can't be sent anywhere
            // but `BASE_URL`. (reqwest would drop the `Authorization` header
            // on a redirect to another host anyway; this also covers a
            // redirect to plain HTTP on the same host.)
            .redirect(reqwest::redirect::Policy::none())
            // Refuse any `http://` URL, so the key and token only travel
            // encrypted.
            .https_only(true)
            .build()
            .context("could not create HTTP client")?;

        Ok(Self {
            // `http.clone()` gives the token manager its own handle to the
            // same connection pool.
            tokens: TokenManager::new(http.clone(), BASE_URL, config.api_key.clone()),
            http,
        })
    }

    /// Asks Garmin whether the configured API key works (used by
    /// `connect-health-mcp setup`). See [`TokenManager::check_key`].
    pub async fn check_key(&self) -> anyhow::Result<KeyCheck> {
        self.tokens.check_key().await
    }

    /// Lists activities matching `query`, newest first.
    ///
    /// The parameters have different types (numbers, dates, an enum), so
    /// they can't share one `[(&str, u32)]` slice. The `ActivitiesQuery`
    /// struct holds them instead; reqwest turns it into
    /// `?start=0&limit=5&activityType=running&minDistance=10000`.
    pub async fn list_activities(
        &self,
        query: &ActivitiesQuery,
    ) -> anyhow::Result<Vec<ActivitySummary>> {
        // Garmin returns a JSON array, which serde turns into a
        // `Vec<Activity>` (a growable list).
        let activities: Vec<Activity> = self.get_json(ACTIVITIES_PATH, query).await?;
        // Convert each raw activity into the tool's output shape:
        // `.into_iter()` walks the list, taking ownership of each item;
        // `.map(...)` converts each one; `.collect()` gathers the results
        // into a new `Vec` (the type comes from the function's return type).
        Ok(activities.into_iter().map(Activity::into_summary).collect())
    }

    /// Gets the full details of one activity by its id (as returned by
    /// `list_activities`). An unknown id fails with Garmin's HTTP 404.
    pub async fn activity_detail(&self, activity_id: u64) -> anyhow::Result<ActivityDetailSummary> {
        // The id is part of the path, not a query parameter, so the path is
        // built with `format!` and the query is an empty slice. `&[] as
        // &[(&str, &str)]` tells the compiler what type the empty slice has.
        let path = format!("/activity-service/activity/{activity_id}");
        let raw: ActivityDetail = self.get_json(&path, &[] as &[(&str, &str)]).await?;
        Ok(raw.into_summary())
    }

    /// Gets the sleep summary for one night.
    ///
    /// `date` is the calendar day Garmin files the sleep under (the day you
    /// woke up). Returns `Ok(None)` when nothing was recorded for that day.
    pub async fn daily_sleep(&self, date: NaiveDate) -> anyhow::Result<Option<SleepSummary>> {
        // `NaiveDate`'s `Display` format is `YYYY-MM-DD`, exactly what Garmin
        // expects, so `.to_string()` gives the query value.
        let date = date.to_string();
        // `date.as_str()` borrows the `String` as a `&str` for the query.
        let raw: DailySleep = self
            .get_json(DAILY_SLEEP_PATH, &[("date", date.as_str())])
            .await?;
        // Convert Garmin's nested format into our flat summary (or `None`).
        Ok(raw.into_summary())
    }

    /// Gets the sleep summary of every night from `start` to `end`
    /// (inclusive), oldest first. Garmin allows at most 31 days between the
    /// dates. Nights without data are left out, so the list may be empty.
    pub async fn sleep_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> anyhow::Result<Vec<SleepSummary>> {
        let (start, end) = (start.to_string(), end.to_string());
        // Each entry has the same shape as a `dailySleep` response.
        let raw: Vec<DailySleep> = self
            .get_json(
                SLEEP_RANGE_PATH,
                &[("startDate", start.as_str()), ("endDate", end.as_str())],
            )
            .await?;
        // Garmin includes some all-`null` entries for nights without data;
        // `into_summary` returns `None` for those and `filter_map` drops them.
        Ok(raw
            .into_iter()
            .filter_map(DailySleep::into_summary)
            .collect())
    }

    /// Gets the latest Fitness Age as of `date` (a later date than the last
    /// update returns that update's values).
    pub async fn fitness_age(&self, date: NaiveDate) -> anyhow::Result<FitnessAgeSummary> {
        // The date is part of the path. `{date}` uses `NaiveDate`'s
        // `Display` format, `YYYY-MM-DD`.
        let path = format!("/fitnessage-service/fitnessage/v2/{date}");
        let raw: FitnessAge = self.get_json(&path, &[] as &[(&str, &str)]).await?;
        Ok(raw.into_summary())
    }

    /// Gets one day's heart rate readings, grouped into intervals of
    /// `interval_min` minutes (2 keeps every reading). `Ok(None)` means no
    /// data for that day.
    pub async fn heart_rate_day(
        &self,
        date: NaiveDate,
        interval_min: u32,
    ) -> anyhow::Result<Option<HeartRateDay>> {
        let raw: DailyHeartRate = self
            .get_json(
                "/wellness-service/wellness/dailyHeartRate",
                &[("date", date.to_string())],
            )
            .await?;
        Ok(raw.into_summary(interval_min))
    }

    /// Gets the user's heart rate zones, one set per configured sport.
    pub async fn heart_rate_zones(&self) -> anyhow::Result<Vec<ZoneSet>> {
        let raw: Vec<HeartRateZones> = self
            .get_json(
                "/biometric-service/heartRateZones/v2",
                &[] as &[(&str, &str)],
            )
            .await?;
        Ok(raw.into_iter().map(HeartRateZones::into_summary).collect())
    }

    /// Gets the Body Battery report per day from `start` to `end`
    /// (inclusive, at most 31 days). `chunks` splits each day into that many
    /// intervals, each giving its lowest and highest level. Days without data
    /// are dropped.
    pub async fn body_battery(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        chunks: u32,
    ) -> anyhow::Result<Vec<BodyBatteryDay>> {
        // Query values of different types (dates and a number) can't share
        // one array type, so they are all turned into `String`s first.
        let query = [
            ("startDate", start.to_string()),
            ("endDate", end.to_string()),
            ("chunks", chunks.to_string()),
        ];
        let raw: Vec<BodyBatteryReport> = self
            .get_json(
                "/wellness-service/wellness/bodyBattery/reports/daily",
                &query,
            )
            .await?;
        // `filter_map` converts each day and drops the `None`s (no data).
        Ok(raw
            .into_iter()
            .filter_map(BodyBatteryReport::into_summary)
            .collect())
    }

    /// Gets weigh-ins and body composition from `start` to `end`
    /// (inclusive), with the range average, the closest weigh-ins outside
    /// the range, and the user's weight goal.
    pub async fn weight(&self, start: NaiveDate, end: NaiveDate) -> anyhow::Result<WeightReport> {
        let range_path = format!("/weight-service/weight/range/{start}/{end}");
        let goal_path = format!("/goal-service/goal/user/effective/weightgoal/{start}/{end}");
        // `tokio::try_join!` runs both requests at the same time and waits
        // for both. If either fails, it returns that error (and `?` passes
        // it on); otherwise it gives a tuple with both results.
        // `includeAll=true` adds every weigh-in of a day, not only the last.
        let (range, goals): (WeightRange, WeightGoals) = tokio::try_join!(
            self.get_json(&range_path, &[("includeAll", "true")]),
            self.get_json(&goal_path, &[] as &[(&str, &str)]),
        )?;
        Ok(range.into_summary(goals))
    }

    /// Gets one HRV summary per night from `start` to `end` (inclusive).
    /// Garmin allows at most 366 days between the dates. `Ok(None)` means
    /// no night in the range has data.
    pub async fn hrv_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> anyhow::Result<Option<Vec<HrvDay>>> {
        let path = format!("/hrv-service/hrv/daily/{start}/{end}");
        let raw: Option<HrvRange> = self
            .get_optional_json(&path, &[] as &[(&str, &str)])
            .await?;
        // `Option::map` converts the response only if there is one.
        Ok(raw.map(HrvRange::into_summary))
    }

    /// Gets one night's HRV with its 5-minute readings. `Ok(None)` means no
    /// data for that night.
    pub async fn hrv_night(&self, date: NaiveDate) -> anyhow::Result<Option<HrvDay>> {
        let path = format!("/hrv-service/hrv/{date}");
        let raw: Option<HrvNight> = self
            .get_optional_json(&path, &[] as &[(&str, &str)])
            .await?;
        // `and_then` (not `map`) because `into_summary` itself returns an
        // `Option`; this avoids an `Option<Option<_>>`.
        Ok(raw.and_then(HrvNight::into_summary))
    }

    /// Gets one wellness summary per day from `start` to `end` (inclusive).
    /// Garmin allows at most 365 days between the dates. Days without data
    /// are left out, so the list may be empty.
    pub async fn daily_summaries(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> anyhow::Result<Vec<DaySummary>> {
        // Two query values of the same type, so a plain slice of pairs works.
        let (start, end) = (start.to_string(), end.to_string());
        let raw: DailySummaries = self
            .get_json(
                "/usersummary-service/v2/dailysummaries",
                &[("startDate", start.as_str()), ("endDate", end.as_str())],
            )
            .await?;
        Ok(raw.into_summary())
    }

    /// Gets one day's wellness summary, with a few fields the range version
    /// lacks. `Ok(None)` means no data for that day.
    pub async fn daily_summary(&self, date: NaiveDate) -> anyhow::Result<Option<DaySummary>> {
        let date = date.to_string();
        let raw: UserDailySummary = self
            .get_json(
                "/usersummary-service/usersummary/daily",
                &[("calendarDate", date.as_str())],
            )
            .await?;
        Ok(raw.into_summary())
    }

    /// Like [`Self::get_optional_json`], for endpoints that always return
    /// data: an empty (204) response is an error here.
    async fn get_json<T: DeserializeOwned, Q: Serialize + ?Sized>(
        &self,
        path: &str,
        query: &Q,
    ) -> anyhow::Result<T> {
        // `.with_context(...)` (from `anyhow::Context`) turns an `Option`
        // into a `Result`: `None` becomes an error with this message.
        self.get_optional_json(path, query)
            .await?
            .with_context(|| format!("GET {path}: unexpected empty response"))
    }

    /// Sends an authenticated `GET` request and parses the JSON response
    /// into `T`, or returns `None` if Garmin answers 204 No Content (some
    /// endpoints, like HRV, use it for "no data").
    ///
    /// `<T: DeserializeOwned>` makes this function *generic*: the caller picks
    /// the result type (e.g. `Vec<Activity>`), and the compiler checks that
    /// serde knows how to build it from JSON.
    ///
    /// `query` can be anything serde can turn into URL parameters, such as
    /// `&[("start", 0), ("limit", 5)]` or `&[("date", "2026-10-23")]`. That is
    /// what the second generic parameter `Q: Serialize` expresses. `?Sized`
    /// relaxes a default rule so `Q` may be a slice like `[(&str, u32)]`,
    /// whose size isn't known at compile time (we only use it via `&Q`).
    async fn get_optional_json<T: DeserializeOwned, Q: Serialize + ?Sized>(
        &self,
        path: &str,
        query: &Q,
    ) -> anyhow::Result<Option<T>> {
        let token = self.tokens.get_token().await?;
        let mut response = self.send_get(path, query, &token).await?;

        // 401 means Garmin no longer accepts this token. Drop it, get a new
        // one, and try exactly once more.
        if response.status() == StatusCode::UNAUTHORIZED {
            eprintln!("garmin: token rejected (401), refreshing and retrying");
            self.tokens.invalidate(&token).await;
            let token = self.tokens.get_token().await?;
            response = self.send_get(path, query, &token).await?;
        }

        let status = response.status();
        if !status.is_success() {
            // Include part of the body: Garmin's error message is often there.
            // `.unwrap_or_default()` uses an empty string if reading fails.
            let body = response.text().await.unwrap_or_default();
            // `.chars().take(500)` keeps at most 500 characters (not bytes,
            // so we never cut a multi-byte character in half).
            let snippet: String = body.chars().take(500).collect();
            anyhow::bail!("GET {path} failed: HTTP {status}: {snippet}");
        }

        // 204 has no body at all, so there is nothing to parse.
        if status == StatusCode::NO_CONTENT {
            return Ok(None);
        }

        // Parse the body as JSON into `T`. `::<T>` (the "turbofish") tells
        // `.json` which type to produce. This fails if the JSON doesn't match
        // `T`, e.g. a field has an unexpected type.
        let value = response
            .json::<T>()
            .await
            .with_context(|| format!("GET {path}: unexpected response format"))?;
        Ok(Some(value))
    }

    /// Sends one `GET` request with the given bearer token.
    async fn send_get<Q: Serialize + ?Sized>(
        &self,
        path: &str,
        query: &Q,
        token: &str,
    ) -> anyhow::Result<reqwest::Response> {
        self.http
            .get(format!("{BASE_URL}{path}"))
            .query(query)
            // `bearer_auth` sets `Authorization: Bearer <token>`.
            .bearer_auth(token)
            .send()
            .await
            // `with_context` takes a closure, so the message is only built
            // if there actually is an error.
            .with_context(|| format!("GET {path} failed (network error)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds (but does not send) a request with `query` and returns its
    /// query string, so we can check what Garmin would receive.
    fn query_string(query: &ActivitiesQuery) -> String {
        let request = reqwest::Client::new()
            .get("https://example.com/")
            .query(query)
            .build()
            .unwrap();
        // `.query()` on a URL returns `Option<&str>`; `unwrap_or("")` gives
        // an empty string if there is none. `.to_string()` makes it owned.
        request.url().query().unwrap_or("").to_string()
    }

    #[test]
    fn activity_type_is_sent_in_garmin_spelling() {
        let query = ActivitiesQuery {
            start: 0,
            limit: 5,
            activity_type: Some(ActivityTypeFilter::FitnessEquipment),
            ..Default::default()
        };
        assert_eq!(
            query_string(&query),
            "start=0&limit=5&activityType=fitness_equipment"
        );
    }

    #[test]
    fn missing_filters_are_left_out() {
        // `..Default::default()` fills every field not written out here
        // with its default (`None` for all the filters).
        let query = ActivitiesQuery {
            start: 10,
            limit: 3,
            ..Default::default()
        };
        assert_eq!(query_string(&query), "start=10&limit=3");
    }

    #[test]
    fn range_filters_are_sent_in_garmin_spelling() {
        let query = ActivitiesQuery {
            start: 0,
            limit: 5,
            start_date: NaiveDate::from_ymd_opt(2026, 10, 1),
            end_date: NaiveDate::from_ymd_opt(2026, 10, 15),
            min_distance: Some(10_000),
            max_duration: Some(5400),
            min_elevation: Some(200),
            ..Default::default()
        };
        assert_eq!(
            query_string(&query),
            "start=0&limit=5&startDate=2026-10-01&endDate=2026-10-15\
             &minDistance=10000&maxDuration=5400&minElevation=200"
        );
    }
}
