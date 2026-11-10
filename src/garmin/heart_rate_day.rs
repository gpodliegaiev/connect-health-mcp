//! Types for one day's heart rate
//! (`GET /wellness-service/wellness/dailyHeartRate?date=YYYY-MM-DD`).
//!
//! Same two-step pattern as the other response types: [`DailyHeartRate`]
//! mirrors Garmin's response, and [`DailyHeartRate::into_summary`] turns it
//! into the [`HeartRateDay`] the tool returns.
//!
//! The watch reads heart rate all day; Garmin sends a reading every 2
//! minutes (about 720 per day, ~13 KB). The tool groups them into intervals
//! (average, lowest, highest) so a day fits in ~1.7 KB at 15 minutes, or
//! returns them as they are with `interval_min = 2`.
//!
//! Found by probing (the spec differs or is silent):
//! - Only one `date` per call (`startDate`/`endDate` give 400 `{}`, as does
//!   a missing date); a malformed date gives 404 `{}`.
//! - A day without data (before the watch, or in the future) still answers
//!   200 with every field `null`.
//! - Reading timestamps are real UTC epoch ms. The local offset is
//!   `startTimestampLocal − startTimestampGMT`; Garmin uses that one offset
//!   for the whole day, even across a daylight-saving change.
//! - For today, `endTimestampGMT` is the last sync but `endTimestampLocal`
//!   still says midnight, so the end is computed from the GMT one.
//! - A `null` reading marks the start of a gap (watch off): the next reading
//!   comes later than 2 minutes.
//! - `minHeartRate` / `maxHeartRate` equal the lowest / highest reading.
//! - `abnormalHrThresholdValue`, `lowAbnormalHrThresholdValue` and
//!   `abnormalHRValuesArray` are in the spec but weren't seen in real data.
//!
//! Left out: `userProfilePK`, GMT copies of the start/end times and the
//! value descriptor list (always `timestamp`, `heartrate`).

use chrono::{NaiveDateTime, TimeDelta};
use serde::{Deserialize, Serialize};

/// Garmin's daily heart rate response, as it arrives.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DailyHeartRate {
    /// The day, "YYYY-MM-DD".
    calendar_date: Option<String>,
    /// Start of the day (local midnight), UTC.
    #[serde(rename = "startTimestampGMT")]
    start_timestamp_gmt: Option<String>,
    /// End of the day, or the last sync for today, UTC.
    #[serde(rename = "endTimestampGMT")]
    end_timestamp_gmt: Option<String>,
    /// Start of the day, local time.
    start_timestamp_local: Option<String>,
    max_heart_rate: Option<u32>,
    min_heart_rate: Option<u32>,
    resting_heart_rate: Option<u32>,
    last_seven_days_avg_resting_heart_rate: Option<u32>,
    /// Readings above this are flagged as abnormal (from the spec).
    abnormal_hr_threshold_value: Option<u32>,
    /// Readings below this are flagged as abnormal (from the spec).
    low_abnormal_hr_threshold_value: Option<u32>,
    /// Flagged readings, `[utc_epoch_ms, bpm]` (from the spec).
    #[serde(rename = "abnormalHRValuesArray")]
    abnormal_hr_values_array: Option<Vec<(i64, Option<u32>)>>,
    /// `[utc_epoch_ms, bpm]` every 2 minutes; `bpm` is `null` at a gap.
    heart_rate_values: Option<Vec<(i64, Option<u32>)>>,
}

/// One day's heart rate, as the tool returns it. All values in bpm.
#[derive(Debug, Serialize)]
pub struct HeartRateDay {
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr_bpm: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr_7day_avg_bpm: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_hr_bpm: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr_bpm: Option<u32>,
    /// Readings above this are flagged as abnormal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abnormal_high_threshold_bpm: Option<u32>,
    /// Readings below this are flagged as abnormal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abnormal_low_threshold_bpm: Option<u32>,
    /// Flagged readings as `["HH:MM" local, bpm]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub abnormal_readings_bpm: Vec<(String, u32)>,
    /// Only for a day still in progress: data ends here, local
    /// "YYYY-MM-DD HH:MM".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracked_until_local: Option<String>,
    /// Every reading as `["HH:MM" local, bpm]`; only with `interval_min` 2.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub readings_bpm: Vec<(String, u32)>,
    /// Length of each interval in `intervals_bpm`, minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_min: Option<u32>,
    /// One entry per interval with readings, as `["HH:MM" local start,
    /// average, lowest, highest]`. Intervals without readings (watch off)
    /// are missing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub intervals_bpm: Vec<(String, u32, u32, u32)>,
}

/// Running totals for one interval while grouping readings.
struct Bucket {
    /// Index of the interval since local midnight (0 = first interval).
    index: i64,
    sum: u32,
    count: u32,
    min: u32,
    max: u32,
}

impl DailyHeartRate {
    /// Converts the response. `interval_min` 2 keeps every reading; a
    /// larger value groups them into intervals of that many minutes.
    /// `None` when the day has no data.
    pub fn into_summary(self, interval_min: u32) -> Option<HeartRateDay> {
        let readings: Vec<(i64, u32)> = self
            .heart_rate_values
            .unwrap_or_default()
            .into_iter()
            // Drop the `null` gap markers: `bpm?` returns `None` for them.
            .filter_map(|(millis, bpm)| Some((millis, bpm?)))
            .collect();
        if readings.is_empty() && self.resting_heart_rate.is_none() {
            return None;
        }

        // The day's start in both clocks gives the local offset.
        let start_gmt = self.start_timestamp_gmt.as_deref().and_then(parse_time);
        let start_local = self.start_timestamp_local.as_deref().and_then(parse_time);
        // `zip` pairs two `Option`s into one `Option` of a tuple.
        let offset = start_local.zip(start_gmt).map(|(local, gmt)| local - gmt);

        // A finished day is 24 h long; a shorter one is still in progress.
        let end_gmt = self.end_timestamp_gmt.as_deref().and_then(parse_time);
        let tracked_until_local = match (start_gmt, end_gmt, offset) {
            (Some(start), Some(end), Some(offset)) if end - start < TimeDelta::hours(24) => {
                Some((end + offset).format("%Y-%m-%d %H:%M").to_string())
            }
            _ => None,
        };

        // A closure (an inline function, `|args| body`) that formats a UTC
        // epoch-ms timestamp as local "HH:MM". It captures `offset` from
        // the surrounding code.
        let clock = |millis: i64| -> Option<String> {
            let utc = chrono::DateTime::from_timestamp_millis(millis)?.naive_utc();
            Some((utc + offset?).format("%H:%M").to_string())
        };

        let abnormal_readings_bpm = self
            .abnormal_hr_values_array
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(millis, bpm)| Some((clock(millis)?, bpm?)))
            .collect();

        let (readings_bpm, intervals_bpm) = if interval_min <= 2 {
            let raw = readings
                .iter()
                .filter_map(|&(millis, bpm)| Some((clock(millis)?, bpm)))
                .collect();
            (raw, Vec::new())
        } else {
            // Without a start time, intervals can't be aligned to midnight.
            let intervals = match (start_gmt, start_local) {
                (Some(start_gmt), Some(start_local)) => {
                    group(&readings, start_gmt, start_local, interval_min)
                }
                _ => Vec::new(),
            };
            (Vec::new(), intervals)
        };

        Some(HeartRateDay {
            date: self.calendar_date.unwrap_or_default(),
            resting_hr_bpm: self.resting_heart_rate,
            resting_hr_7day_avg_bpm: self.last_seven_days_avg_resting_heart_rate,
            min_hr_bpm: self.min_heart_rate,
            max_hr_bpm: self.max_heart_rate,
            abnormal_high_threshold_bpm: self.abnormal_hr_threshold_value,
            abnormal_low_threshold_bpm: self.low_abnormal_hr_threshold_value,
            abnormal_readings_bpm,
            tracked_until_local,
            readings_bpm,
            // `then_some` gives `Some(value)` only if the condition is true.
            interval_min: (interval_min > 2).then_some(interval_min),
            intervals_bpm,
        })
    }
}

/// Groups readings into intervals of `interval_min` minutes counted from
/// local midnight, giving `(local start, average, lowest, highest)` per
/// interval that has readings.
///
/// `&[(i64, u32)]` is a *slice*: a borrowed view of the list, so the
/// readings are read without being copied or moved.
fn group(
    readings: &[(i64, u32)],
    start_gmt: NaiveDateTime,
    start_local: NaiveDateTime,
    interval_min: u32,
) -> Vec<(String, u32, u32, u32)> {
    let start_millis = start_gmt.and_utc().timestamp_millis();
    let interval_millis = i64::from(interval_min) * 60_000;
    let mut out = Vec::new();
    // The interval being filled. `Option` because there is none before the
    // first reading.
    let mut current: Option<Bucket> = None;

    // Pushes a finished bucket to `out`. A closure can't be used here
    // because it would borrow `out` while the loop also uses it, so this
    // is a small inner function that takes `out` explicitly.
    fn finish(
        bucket: Bucket,
        start_local: NaiveDateTime,
        interval_min: u32,
        out: &mut Vec<(String, u32, u32, u32)>,
    ) {
        let begin = start_local + TimeDelta::minutes(bucket.index * i64::from(interval_min));
        // Rounded average: add half the count before the whole-number
        // division.
        let avg = (bucket.sum + bucket.count / 2) / bucket.count;
        out.push((
            begin.format("%H:%M").to_string(),
            avg,
            bucket.min,
            bucket.max,
        ));
    }

    // Readings arrive in time order, so each interval is filled in one go.
    for &(millis, bpm) in readings {
        // `div_euclid` rounds down also for negative numbers (a reading
        // before midnight, which shouldn't happen, would get index -1).
        let index = (millis - start_millis).div_euclid(interval_millis);
        match current.as_mut() {
            // Same interval: add the reading.
            Some(bucket) if bucket.index == index => {
                bucket.sum += bpm;
                bucket.count += 1;
                bucket.min = bucket.min.min(bpm);
                bucket.max = bucket.max.max(bpm);
            }
            // A new interval: finish the previous one (if any) and start.
            _ => {
                // `Option::take` moves the value out and leaves `None`.
                if let Some(done) = current.take() {
                    finish(done, start_local, interval_min, &mut out);
                }
                current = Some(Bucket {
                    index,
                    sum: bpm,
                    count: 1,
                    min: bpm,
                    max: bpm,
                });
            }
        }
    }
    if let Some(done) = current {
        finish(done, start_local, interval_min, &mut out);
    }
    out
}

/// Parses "2026-10-24T00:34:09.0" or "2026-10-24T00:34:09".
fn parse_time(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real day in progress (values made up) (Europe/Paris, UTC+1): readings at
    /// 00:00, 00:02, a gap, then 00:20.
    fn raw() -> DailyHeartRate {
        serde_json::from_value(serde_json::json!({
            "userProfilePK": 1, "calendarDate": "2026-10-27",
            "startTimestampGMT": "2026-10-26T23:00:00.0",
            "endTimestampGMT": "2026-10-27T10:29:00.0",
            "startTimestampLocal": "2026-10-27T00:00:00.0",
            "endTimestampLocal": "2026-10-28T00:00:00.0",
            "maxHeartRate": 96, "minHeartRate": 54, "restingHeartRate": 57,
            "lastSevenDaysAvgRestingHeartRate": 56,
            "heartRateValueDescriptors": [{"index": 0, "key": "timestamp"}],
            "heartRateValues": [
                [1793055600000_i64, 70], [1793055720000_i64, 61],
                [1793055840000_i64, null], [1793056800000_i64, 55]
            ]
        }))
        .unwrap()
    }

    #[test]
    fn groups_readings_into_intervals() {
        assert_eq!(
            serde_json::to_value(raw().into_summary(15)).unwrap(),
            serde_json::json!({
                "date": "2026-10-27", "resting_hr_bpm": 57, "resting_hr_7day_avg_bpm": 56,
                "min_hr_bpm": 54, "max_hr_bpm": 96,
                "tracked_until_local": "2026-10-27 11:29",
                "interval_min": 15,
                // (70 + 61) / 2 = 65.5 → 66.
                "intervals_bpm": [["00:00", 66, 61, 70], ["00:15", 55, 55, 55]]
            })
        );
    }

    #[test]
    fn interval_two_keeps_every_reading() {
        let day = raw().into_summary(2).unwrap();
        assert_eq!(
            day.readings_bpm,
            vec![
                ("00:00".to_string(), 70),
                ("00:02".to_string(), 61),
                ("00:20".to_string(), 55)
            ]
        );
        assert!(day.intervals_bpm.is_empty());
        assert_eq!(day.interval_min, None);
    }

    #[test]
    fn all_null_day_means_no_data() {
        let raw: DailyHeartRate = serde_json::from_value(serde_json::json!({
            "calendarDate": "2010-01-01", "startTimestampGMT": null,
            "maxHeartRate": null, "restingHeartRate": null,
            "heartRateValueDescriptors": null, "heartRateValues": null
        }))
        .unwrap();
        assert!(raw.into_summary(15).is_none());
    }
}
