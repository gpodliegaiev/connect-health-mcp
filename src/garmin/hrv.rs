//! Types for heart rate variability (HRV):
//! `GET /hrv-service/hrv/daily/{fromDate}/{toDate}` (one summary per night)
//! and `GET /hrv-service/hrv/{calendarDate}` (one night with its readings).
//!
//! HRV is the variation in time between heartbeats, measured overnight in
//! milliseconds. Higher than your normal usually means recovered; a drop
//! below your baseline is a sign of fatigue, stress or illness. Garmin
//! compares the 7-day average with a personal baseline range to give a
//! status (`BALANCED`, `UNBALANCED`, `LOW`, `POOR`).
//!
//! Same two-step pattern as the other response types: [`HrvRange`] and
//! [`HrvNight`] mirror Garmin's responses, and `into_summary()` turns them
//! into a list of [`HrvDay`] for the tool.
//!
//! Found by probing (the spec is silent):
//! - A range or day without data returns 204 with an empty body.
//! - The range may span at most 366 days between the two dates; a longer
//!   or reversed range returns 400 `{}`. Days without data are just absent.
//! - In the first weeks with a watch, `weeklyAvg` and `baseline` are `null`
//!   and `status` is `NONE` with feedback `ONBOARDING_1`.
//! - The single-day response adds about 100 readings (one per 5 minutes)
//!   and the measured window, with GMT and local copies of every time.
//!
//! Left out on purpose: `userProfilePk`, `createTimeStamp` (when Garmin
//! computed the values), `markerValue` (where the weekly average sits on the
//! app's gauge, 0 to 1; follows from the average and the baseline), GMT
//! copies of local times, and the sleep window (already in `get_sleep`).

use super::types::null_as_default;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// The range response: one summary per night that has data.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HrvRange {
    #[serde(deserialize_with = "null_as_default")]
    hrv_summaries: Vec<RawSummary>,
}

/// The single-day response: the night's summary plus each reading.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HrvNight {
    hrv_summary: Option<RawSummary>,
    #[serde(deserialize_with = "null_as_default")]
    hrv_readings: Vec<RawReading>,
    /// Start of the measured window, local time, "YYYY-MM-DDTHH:MM:SS.0".
    start_timestamp_local: Option<String>,
    /// End of the measured window, local time.
    end_timestamp_local: Option<String>,
}

/// One night's HRV summary, as it arrives. Same shape in both responses.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawSummary {
    /// Day the night is filed under (the wake-up day), "YYYY-MM-DD".
    calendar_date: Option<String>,
    /// Average HRV of the last 7 nights, ms.
    weekly_avg: Option<u32>,
    /// Average HRV of this night, ms.
    last_night_avg: Option<u32>,
    /// Highest 5-minute average of this night, ms.
    #[serde(rename = "lastNight5MinHigh")]
    last_night_5min_high: Option<u32>,
    /// The user's normal range, built from several weeks of nights.
    baseline: Option<RawBaseline>,
    /// `BALANCED`, `UNBALANCED`, `LOW`, `POOR`, `NONE` or `INVALID`.
    status: Option<String>,
    /// Code of Garmin's explanation, e.g. `HRV_BALANCED_7`, `ONBOARDING_1`.
    feedback_phrase: Option<String>,
}

/// The baseline ranges, ms. Below `low_upper` is `LOW`; from `balanced_low`
/// to `balanced_upper` is `BALANCED`; between or above is `UNBALANCED`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawBaseline {
    low_upper: Option<u32>,
    balanced_low: Option<u32>,
    balanced_upper: Option<u32>,
}

/// One 5-minute HRV reading.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawReading {
    hrv_value: Option<u32>,
    /// "YYYY-MM-DDTHH:MM:SS.0", local time.
    reading_time_local: Option<String>,
}

/// One night of HRV, as the `get_hrv` tool returns it. All values in ms.
#[derive(Debug, Serialize)]
pub struct HrvDay {
    /// Day the night is filed under: the day the user woke up.
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_night_avg_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_night_5min_high_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weekly_avg_ms: Option<u32>,
    /// `[low, high]` of the user's normal (balanced) range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_balanced_ms: Option<[u32; 2]>,
    /// Weekly averages below this are `LOW`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_low_upper_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Measured window, local "YYYY-MM-DD HH:MM:SS". Single day only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured_from_local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured_to_local: Option<String>,
    /// `[local time "HH:MM", HRV ms]` every 5 minutes. Single day only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub readings_ms: Vec<(String, u32)>,
}

impl HrvRange {
    /// Converts the range response into one [`HrvDay`] per night.
    pub fn into_summary(self) -> Vec<HrvDay> {
        // `filter_map` converts each summary and drops the ones that come
        // back `None` (a summary without a date).
        self.hrv_summaries
            .into_iter()
            .filter_map(RawSummary::into_day)
            .collect()
    }
}

impl HrvNight {
    /// Converts the single-day response; `None` if it has no summary.
    pub fn into_summary(self) -> Option<HrvDay> {
        // `?` on an `Option`: return `None` right away if there is no
        // summary (or it has no date).
        let mut day = self.hrv_summary?.into_day()?;
        day.measured_from_local = self.start_timestamp_local.map(local_time);
        day.measured_to_local = self.end_timestamp_local.map(local_time);
        day.readings_ms = self
            .hrv_readings
            .into_iter()
            // A reading is kept only if it has both a value and a time.
            // `?` inside the closure works because the closure returns an
            // `Option`.
            .filter_map(|r| Some((clock(&r.reading_time_local?), r.hrv_value?)))
            .collect();
        Some(day)
    }
}

impl RawSummary {
    /// Converts one summary. Values are whole milliseconds, so nothing needs
    /// rounding. `None` if the summary has no date.
    fn into_day(self) -> Option<HrvDay> {
        // `unwrap_or_default()`: no baseline object behaves like one with
        // every field `None`.
        let baseline = self.baseline.unwrap_or_default();
        Some(HrvDay {
            date: self.calendar_date?,
            last_night_avg_ms: self.last_night_avg,
            last_night_5min_high_ms: self.last_night_5min_high,
            weekly_avg_ms: self.weekly_avg,
            // `zip` combines two `Option`s into `Some((a, b))` only when
            // both are present.
            baseline_balanced_ms: baseline
                .balanced_low
                .zip(baseline.balanced_upper)
                .map(|(low, high)| [low, high]),
            baseline_low_upper_ms: baseline.low_upper,
            // `NONE` means "no status yet", which says nothing.
            status: self.status.filter(|s| s != "NONE"),
            feedback: self.feedback_phrase.filter(|f| f != "NONE"),
            measured_from_local: None,
            measured_to_local: None,
            readings_ms: Vec::new(),
        })
    }
}

/// Rewrites "2026-10-24T00:34:09.0" as "2026-10-24 00:34:09".
/// Unexpected input is returned unchanged.
fn local_time(text: String) -> String {
    match NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S%.f") {
        Ok(time) => time.format("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => text,
    }
}

/// Keeps only the time of day: "2026-10-24T00:37:10.0" → "00:37". Readings
/// are 5 minutes apart, so seconds add nothing. `&str` borrows the text,
/// since a new `String` is built anyway.
fn clock(text: &str) -> String {
    match NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        Ok(time) => time.format("%H:%M").to_string(),
        Err(_) => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_summaries_are_converted() {
        // Shape of real entries (values made up): a normal night and an
        // onboarding night.
        let raw: HrvRange = serde_json::from_value(serde_json::json!({
            "userProfilePk": 1,
            "hrvSummaries": [
                {"calendarDate": "2024-09-15", "weeklyAvg": null, "lastNightAvg": 38,
                 "lastNight5MinHigh": 61, "baseline": null, "status": "NONE",
                 "feedbackPhrase": "ONBOARDING_1", "createTimeStamp": "2024-09-15T06:55:41.873"},
                {"calendarDate": "2026-10-24", "weeklyAvg": 52, "lastNightAvg": 55,
                 "lastNight5MinHigh": 84,
                 "baseline": {"lowUpper": 40, "balancedLow": 44, "balancedUpper": 63,
                              "markerValue": 0.4296875},
                 "status": "BALANCED", "feedbackPhrase": "HRV_BALANCED_7",
                 "createTimeStamp": "2026-10-24T07:57:41.742"}
            ]
        }))
        .unwrap();
        let out = serde_json::to_value(raw.into_summary()).unwrap();
        assert_eq!(
            out,
            serde_json::json!([
                {"date": "2024-09-15", "last_night_avg_ms": 38, "last_night_5min_high_ms": 61,
                 "feedback": "ONBOARDING_1"},
                {"date": "2026-10-24", "last_night_avg_ms": 55, "last_night_5min_high_ms": 84,
                 "weekly_avg_ms": 52, "baseline_balanced_ms": [44, 63],
                 "baseline_low_upper_ms": 40, "status": "BALANCED",
                 "feedback": "HRV_BALANCED_7"}
            ])
        );
    }

    #[test]
    fn single_night_adds_readings() {
        let raw: HrvNight = serde_json::from_value(serde_json::json!({
            "hrvSummary": {"calendarDate": "2026-10-24", "lastNightAvg": 55, "status": "BALANCED"},
            "hrvReadings": [
                {"hrvValue": 47, "readingTimeGMT": "2026-10-23T22:37:10.0",
                 "readingTimeLocal": "2026-10-24T00:37:10.0"},
                {"hrvValue": 66, "readingTimeGMT": "2026-10-23T22:42:10.0",
                 "readingTimeLocal": "2026-10-24T00:42:10.0"}
            ],
            "startTimestampGMT": "2026-10-23T22:34:09.0",
            "startTimestampLocal": "2026-10-24T00:34:09.0",
            "endTimestampLocal": "2026-10-24T09:27:10.0"
        }))
        .unwrap();
        let out = serde_json::to_value(raw.into_summary()).unwrap();
        assert_eq!(
            out,
            serde_json::json!({
                "date": "2026-10-24", "last_night_avg_ms": 55, "status": "BALANCED",
                "measured_from_local": "2026-10-24 00:34:09",
                "measured_to_local": "2026-10-24 09:27:10",
                "readings_ms": [["00:37", 47], ["00:42", 66]]
            })
        );
    }
}
