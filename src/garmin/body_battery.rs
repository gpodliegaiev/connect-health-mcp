//! Types for the daily Body Battery report
//! (`GET /wellness-service/wellness/bodyBattery/reports/daily?startDate&endDate&chunks`).
//!
//! Same two-step pattern as the other response types: [`BodyBatteryReport`]
//! mirrors one day of Garmin's response, and
//! [`BodyBatteryReport::into_summary`] turns it into the [`BodyBatteryDay`]
//! the tool returns.
//!
//! Body Battery is Garmin's 5–100 energy estimate. The daily summary already
//! has the day's totals and events; this report adds the level through the
//! day, so an AI can see when the user recharged and what drained them.
//!
//! Found by probing (the spec differs or is silent):
//! - `chunks=N` splits the day into N equal intervals and returns the lowest
//!   and the highest reading of each, with the time it happened: 2N
//!   `[timestamp, level]` pairs, in time order. Without `chunks` (or with 0)
//!   Garmin uses 3. `chunks=1000` still works (2000 pairs).
//! - The pair timestamps are real UTC epoch ms (unlike sleep's
//!   `...TimestampLocal`); the day's offset is `startTimestampLocal −
//!   startTimestampGMT`. On a daylight-saving day the offset at the end of
//!   the day differs, so the end offset is used from 03:00 local on.
//! - At most 31 days (`startDate` to `endDate` inclusive); 32 or more,
//!   reversed or missing dates give 400 `{}`, a malformed date 404 `{}`.
//! - A day without data is still listed, with `charged: null`, `null`
//!   levels and fake timestamps (`1262304000001`, ...): dropped here.
//! - Today ends at the last sync (`endTimestampLocal` is not midnight), and
//!   its intervals split only the hours so far.
//! - Events have a GMT start plus `timezoneOffset` (ms), the same shape as
//!   the single-day daily summary's events, so that code is reused.
//!
//! Left out: GMT copies of the start/end times, the value descriptor list
//! (always `timestamp`, `bodyBatteryLevel`), the feedback events'
//! `eventTimestampGmt` (when Garmin wrote the comment) and
//! `feedbackShortType` (a shorter copy of `feedbackLongType`).

use chrono::{NaiveDateTime, NaiveTime, TimeDelta};
use serde::{Deserialize, Serialize};

use super::daily_summary::{BodyBatteryEvent, RawEventV1, RawFeedback};

/// One day of Garmin's Body Battery report, as it arrives. The response is
/// a list of these, one per day.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BodyBatteryReport {
    /// The day, "YYYY-MM-DD".
    date: Option<String>,
    /// Total Body Battery gained during the day.
    charged: Option<u32>,
    /// Total Body Battery used during the day.
    drained: Option<u32>,
    /// Start of the day, UTC, "YYYY-MM-DDTHH:MM:SS.0".
    #[serde(rename = "startTimestampGMT")]
    start_timestamp_gmt: Option<String>,
    /// End of the day (or the last sync, for today), UTC.
    #[serde(rename = "endTimestampGMT")]
    end_timestamp_gmt: Option<String>,
    /// Start of the day, local time.
    start_timestamp_local: Option<String>,
    /// End of the day (or the last sync), local time.
    end_timestamp_local: Option<String>,
    /// `[utc_epoch_ms, level]` pairs: the low and the high of each interval.
    /// The level is `null` on days without data, so it is an `Option`.
    body_battery_values_array: Option<Vec<(i64, Option<u32>)>>,
    /// Garmin's latest comment on the day, e.g. before sleep.
    body_battery_dynamic_feedback_event: Option<RawFeedback>,
    /// Garmin's comment at the end of the previous night's sleep window.
    end_of_day_body_battery_dynamic_feedback_event: Option<RawFeedback>,
    /// What charged or drained Body Battery: sleep, activities, rest, stress.
    body_battery_activity_event: Option<Vec<RawEventV1>>,
}

/// One day of Body Battery, as the tool returns it.
#[derive(Debug, Serialize)]
pub struct BodyBatteryDay {
    pub date: String,
    /// Body Battery gained during the day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charged: Option<u32>,
    /// Body Battery used during the day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drained: Option<u32>,
    /// Lowest level of the day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowest: Option<u32>,
    /// Highest level of the day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highest: Option<u32>,
    /// The low and the high of each interval as `["HH:MM" local, level]`,
    /// in time order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<(String, u32)>,
    /// Only for a day still in progress: data ends here, local
    /// "YYYY-MM-DD HH:MM".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracked_until_local: Option<String>,
    /// Garmin's latest comment, e.g. `"HIGH: SLEEP_PREPARATION_..."`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Garmin's comment at the end of the previous night's sleep window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_of_day_feedback: Option<String>,
    /// What charged or drained Body Battery, in time order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<BodyBatteryEvent>,
}

impl BodyBatteryReport {
    /// Converts one day. `None` for a day without data.
    pub fn into_summary(self) -> Option<BodyBatteryDay> {
        // Local minus UTC at the start and at the end of the day: the
        // time-zone offset, which the level timestamps don't carry.
        let start_offset = offset(&self.start_timestamp_local, &self.start_timestamp_gmt);
        let end_offset = offset(&self.end_timestamp_local, &self.end_timestamp_gmt);

        let levels: Vec<(String, u32)> = self
            .body_battery_values_array
            .unwrap_or_default()
            .into_iter()
            // `filter_map` drops pairs whose level is `null` (`?` on the
            // level returns `None` for them) or whose time can't be read.
            .filter_map(|(millis, level)| {
                let time = local_clock(millis, start_offset?, end_offset)?;
                Some((time, level?))
            })
            .collect();

        // No level and no totals: the day has no data.
        if levels.is_empty() && self.charged.is_none() && self.drained.is_none() {
            return None;
        }

        // `iter()` borrows the pairs; `map` picks the level; `min`/`max`
        // give `None` for an empty list. With one low and one high per
        // interval, these are the day's true lowest and highest.
        let lowest = levels.iter().map(|(_, level)| *level).min();
        let highest = levels.iter().map(|(_, level)| *level).max();

        // A finished day ends at local midnight; anything else is the last
        // sync of a day still in progress.
        let tracked_until_local = self
            .end_timestamp_local
            .as_deref()
            .and_then(parse_time)
            .filter(|end| end.time() != NaiveTime::MIN)
            .map(|end| end.format("%Y-%m-%d %H:%M").to_string());

        Some(BodyBatteryDay {
            date: self.date.unwrap_or_default(),
            charged: self.charged,
            drained: self.drained,
            lowest,
            highest,
            levels,
            tracked_until_local,
            feedback: self
                .body_battery_dynamic_feedback_event
                .and_then(RawFeedback::text),
            end_of_day_feedback: self
                .end_of_day_body_battery_dynamic_feedback_event
                .and_then(RawFeedback::text),
            events: self
                .body_battery_activity_event
                .unwrap_or_default()
                .into_iter()
                .map(RawEventV1::into_event)
                .collect(),
        })
    }
}

/// Parses "2026-10-24T00:34:09.0" or "2026-10-24T00:34:09".
fn parse_time(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
}

/// Local minus UTC, from the same moment written both ways.
///
/// `&Option<String>` borrows the field; `as_deref` turns it into
/// `Option<&str>` so it can be parsed without copying the text.
fn offset(local: &Option<String>, gmt: &Option<String>) -> Option<TimeDelta> {
    Some(parse_time(local.as_deref()?)? - parse_time(gmt.as_deref()?)?)
}

/// Turns a UTC epoch-ms timestamp into a local "HH:MM".
///
/// On a daylight-saving day `start` and `end` differ; clocks change at
/// night, so `end` is used from 03:00 local (by the start offset) on.
fn local_clock(millis: i64, start: TimeDelta, end: Option<TimeDelta>) -> Option<String> {
    let utc = chrono::DateTime::from_timestamp_millis(millis)?.naive_utc();
    let mut local = utc + start;
    // A let chain (Rust 2024): `if let` plus extra `&&` conditions. Only
    // when the end offset is known, differs, and it's 03:00 or later.
    if let Some(end) = end
        && end != start
        && local.time() >= NaiveTime::from_hms_opt(3, 0, 0)?
    {
        local = utc + end;
    }
    Some(local.format("%H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_levels_and_events() {
        // The shape of a real day (values made up; Europe/Paris, UTC+1), `chunks=1`.
        let raw: BodyBatteryReport = serde_json::from_value(serde_json::json!({
            "date": "2026-10-26", "charged": 55, "drained": 71,
            "startTimestampGMT": "2026-10-25T23:00:00.0",
            "endTimestampGMT": "2026-10-26T23:00:00.0",
            "startTimestampLocal": "2026-10-26T00:00:00.0",
            "endTimestampLocal": "2026-10-27T00:00:00.0",
            "bodyBatteryValuesArray": [[1792996560000_i64, 100], [1793055420000_i64, 30]],
            "bodyBatteryValueDescriptorDTOList": [],
            "bodyBatteryDynamicFeedbackEvent": {
                "eventTimestampGmt": "2026-10-26T21:00:11.0", "bodyBatteryLevel": "HIGH",
                "feedbackShortType": "SLEEP_PREPARATION_RECOVERING_AND_INTENSIVE_EXERCISE",
                "feedbackLongType": "SLEEP_PREPARATION_RECOVERING_AND_INTENSIVE_EXERCISE"
            },
            "bodyBatteryActivityEvent": [{
                "eventType": "ACTIVITY", "eventStartTimeGmt": "2026-10-26T17:29:33.0",
                "timezoneOffset": 3600000, "durationInMilliseconds": 4800000,
                "bodyBatteryImpact": -11, "feedbackType": "EXERCISE_TRAINING_EFFECT_2",
                "shortFeedback": "MAINTAINING_ANAEROBIC_FITNESS"
            }]
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(raw.into_summary()).unwrap(),
            serde_json::json!({
                "date": "2026-10-26", "charged": 55, "drained": 71,
                "lowest": 30, "highest": 100,
                "levels": [["07:36", 100], ["23:57", 30]],
                "feedback": "HIGH: SLEEP_PREPARATION_RECOVERING_AND_INTENSIVE_EXERCISE",
                "events": [{
                    "type": "ACTIVITY", "start_local": "2026-10-26 18:29",
                    "duration_min": 80.0, "impact": -11,
                    "feedback": "EXERCISE_TRAINING_EFFECT_2",
                    "short_feedback": "MAINTAINING_ANAEROBIC_FITNESS"
                }]
            })
        );
    }

    #[test]
    fn daylight_saving_day_and_partial_day() {
        // 2026-10-25: Paris goes from UTC+2 to UTC+1 at 03:00 local.
        let raw: BodyBatteryReport = serde_json::from_value(serde_json::json!({
            "date": "2026-10-25", "charged": 10,
            "startTimestampGMT": "2026-10-24T22:00:00.0",
            "endTimestampGMT": "2026-10-25T10:00:00.0",
            "startTimestampLocal": "2026-10-25T00:00:00.0",
            "endTimestampLocal": "2026-10-25T11:00:00.0",
            // 22:03 UTC = 00:03 local (UTC+2); 09:00 UTC = 10:00 (UTC+1).
            "bodyBatteryValuesArray": [[1792879380000_i64, 34], [1792918800000_i64, 99]]
        }))
        .unwrap();
        let day = raw.into_summary().unwrap();
        assert_eq!(
            day.levels,
            vec![("00:03".to_string(), 34), ("10:00".to_string(), 99)]
        );
        assert_eq!(day.tracked_until_local.as_deref(), Some("2026-10-25 11:00"));
    }

    #[test]
    fn day_without_data_is_dropped() {
        let raw: BodyBatteryReport = serde_json::from_value(serde_json::json!({
            "date": "2010-01-01", "charged": null, "drained": null,
            "startTimestampGMT": null, "endTimestampGMT": null,
            "startTimestampLocal": null, "endTimestampLocal": null,
            "bodyBatteryValuesArray": [[1262304000001_i64, null], [1262304000002_i64, null]],
            "bodyBatteryValueDescriptorDTOList": null
        }))
        .unwrap();
        assert!(raw.into_summary().is_none());
    }
}
