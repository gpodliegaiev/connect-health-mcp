//! Types for one night's sleep
//! (`GET /sleep-service/sleep/dailySleep?date=YYYY-MM-DD`).
//!
//! Same two-step pattern as the activity types: [`DailySleep`] mirrors
//! Garmin's nested response, and [`DailySleep::into_summary`] flattens it
//! into the [`SleepSummary`] the tool returns, with the unit in every field
//! name.
//!
//! Every measurement is kept so an AI can evaluate the night fully: stages,
//! score and the rating of each score factor with Garmin's optimal range,
//! heart rate, respiration, SpO2, stress, naps, sleep need and its
//! adjustments, recommended bedtime, and sleep-timing alignment.
//!
//! Left out on purpose (not about the sleep): profile and device ids, GMT
//! copies of the local times, `sleepVersion`, `ageGroup`, confirmation and
//! device-capability flags.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use super::types::{count, null_as_default, positive, round, rounded};

/// Garmin's daily sleep response, as it arrives.
///
/// Only used for reading (`Deserialize`). Every field is an `Option` because,
/// for a day without sleep data, Garmin still answers with the same object
/// but with every field set to `null`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DailySleep {
    /// Day the sleep is filed under, "YYYY-MM-DD". `None` means no data.
    calendar_date: Option<String>,
    /// Bedtime as milliseconds since 1970, already shifted to local time.
    sleep_start_timestamp_local: Option<i64>,
    /// Wake-up time, same format as the start.
    sleep_end_timestamp_local: Option<i64>,

    // --- Durations (seconds) ---
    /// Total time asleep.
    sleep_time_seconds: Option<u32>,
    deep_sleep_seconds: Option<u32>,
    light_sleep_seconds: Option<u32>,
    rem_sleep_seconds: Option<u32>,
    awake_sleep_seconds: Option<u32>,
    /// Time the watch couldn't classify (e.g. worn loosely).
    unmeasurable_sleep_seconds: Option<u32>,
    /// Time spent napping during the day.
    nap_time_seconds: Option<u32>,
    /// Each nap. Garmin spells the name `DTOS`, so it is set by hand.
    #[serde(rename = "dailyNapDTOS")]
    #[serde(deserialize_with = "null_as_default")]
    naps: Vec<RawNap>,
    awake_count: Option<u32>,

    // --- Body measurements ---
    avg_heart_rate: Option<f64>,
    /// Breaths per minute.
    average_respiration_value: Option<f64>,
    lowest_respiration_value: Option<f64>,
    highest_respiration_value: Option<f64>,
    /// Blood oxygen, percent. Only with SpO2 tracking enabled during sleep.
    #[serde(rename = "averageSpO2Value")]
    average_spo2: Option<f64>,
    #[serde(rename = "lowestSpO2Value")]
    lowest_spo2: Option<f64>,
    #[serde(rename = "highestSpO2Value")]
    highest_spo2: Option<f64>,
    /// Average heart rate while SpO2 was measured.
    #[serde(rename = "averageSpO2HRSleep")]
    average_spo2_hr: Option<f64>,
    /// Readings below the alert threshold, and that threshold (percent).
    number_of_events_below_threshold: Option<u32>,
    #[serde(rename = "spo2Threshold")]
    spo2_threshold: Option<u32>,
    /// `NONE`, `LOW`, `MEDIUM`, `HIGH` or `UNMEASURABLE`.
    breathing_disruption_severity: Option<String>,
    /// Stress level during sleep, 0–100.
    avg_sleep_stress: Option<f64>,

    // --- Garmin's assessment ---
    /// Short code describing the night, e.g. `POSITIVE_HIGHLY_RECOVERING`.
    sleep_score_feedback: Option<String>,
    /// Short code with an extra insight, e.g. `POSITIVE_RESTFUL_EVENING`.
    sleep_score_insight: Option<String>,
    /// `NOT_AVAILABLE` when there is none.
    sleep_score_personalized_insight: Option<String>,
    /// Sleep score and the rating of each factor (nested object).
    sleep_scores: Option<SleepScores>,
    /// How much sleep Garmin recommended for this night, and for tonight.
    sleep_need: Option<SleepNeed>,
    next_sleep_need: Option<SleepNeed>,
    /// Whether sleep timing matches the user's usual rhythm.
    sleep_alignment: Option<SleepAlignment>,
    /// Free text the user wrote about the night.
    user_note: Option<String>,
}

/// One entry of `dailyNapDTOS`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawNap {
    nap_time_sec: Option<u32>,
    /// UTC, e.g. "2026-10-23T13:05:00.0".
    #[serde(rename = "napStartTimestampGMT")]
    nap_start: Option<String>,
    #[serde(rename = "napEndTimestampGMT")]
    nap_end: Option<String>,
    /// Minutes to add to UTC for local time.
    nap_start_time_offset: Option<i64>,
    nap_end_time_offset: Option<i64>,
    nap_feedback: Option<String>,
}

/// The `sleepScores` object: the overall score and one entry per factor.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SleepScores {
    overall: Option<Score>,
    /// Optimal range in seconds of sleep.
    total_duration: Option<Score>,
    /// Optimal range in percent of total sleep.
    deep_percentage: Option<Score>,
    light_percentage: Option<Score>,
    rem_percentage: Option<Score>,
    /// Optimal range of average sleep stress.
    stress: Option<Score>,
    /// Optimal range of times awake.
    awake_count: Option<Score>,
    /// Optimal range of Garmin's restlessness measure.
    restlessness: Option<Score>,
}

/// One score inside `sleepScores`: a number, a rating word, and Garmin's
/// optimal range for this factor.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Score {
    /// The number (0–100 for the overall score, a percentage for stages).
    /// Missing for factors that only get a rating.
    value: Option<u32>,
    /// Rating such as `EXCELLENT`, `GOOD`, `FAIR`, `POOR`.
    qualifier_key: Option<String>,
    optimal_start: Option<f64>,
    optimal_end: Option<f64>,
}

impl Score {
    /// The optimal range as `[low, high]`, with `scale` applied to both ends
    /// (e.g. seconds → minutes), or `None` if Garmin gave no range.
    ///
    /// `&self` borrows the score; it is only read.
    fn range(&self, scale: f64) -> Option<[f64; 2]> {
        // A `match` on a tuple handles both values at once: only when both
        // are `Some` is there a range.
        match (self.optimal_start, self.optimal_end) {
            (Some(low), Some(high)) => Some([round(low * scale, 1), round(high * scale, 1)]),
            _ => None,
        }
    }
}

/// The `sleepNeed` / `nextSleepNeed` object.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SleepNeed {
    /// Recommended sleep, in minutes.
    actual: Option<u32>,
    /// The user's usual need before adjustments, in minutes.
    baseline: Option<u32>,
    /// Summary code, e.g. `NO_CHANGE_NO_ADJUSTMENTS`, `INCREASED`.
    feedback: Option<String>,
    /// Why the need changed: one code per reason, `NO_CHANGE` if it didn't.
    training_feedback: Option<String>,
    sleep_history_adjustment: Option<String>,
    hrv_adjustment: Option<String>,
    nap_adjustment: Option<String>,
    /// Recommended bedtime window, minutes after local midnight.
    recommended_bedtime_start_mins: Option<i64>,
    recommended_bedtime_end_mins: Option<i64>,
}

/// The `sleepAlignment` object. Times are minutes after local midnight.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SleepAlignment {
    /// e.g. `ALIGNED`.
    status: Option<String>,
    optimal_sleep_window_start_mins: Option<i64>,
    optimal_sleep_window_end_mins: Option<i64>,
    optimal_sleep_window_midpoint_mins: Option<i64>,
    last_sleep_midpoint_mins: Option<i64>,
}

/// One night's sleep, in the flat shape the tool returns.
///
/// Only used for writing (`Serialize`). Fields Garmin did not provide, or
/// that hold a "nothing to report" value (`0`, `NOT_AVAILABLE`,
/// `NO_CHANGE`), are left out. Ranges are `[low, high]`.
#[derive(Debug, Serialize)]
pub struct SleepSummary {
    /// Day the sleep is filed under (the day you woke up), "YYYY-MM-DD".
    pub calendar_date: String,
    /// Bedtime, local time, "YYYY-MM-DD HH:MM:SS".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_start_local: Option<String>,
    /// Wake-up time, local time, "YYYY-MM-DD HH:MM:SS".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_end_local: Option<String>,

    // --- Durations ---
    /// Total time asleep, in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_sleep_minutes: Option<f64>,
    /// Time in each sleep stage, in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_sleep_minutes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_sleep_minutes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rem_sleep_minutes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awake_minutes: Option<f64>,
    /// Time the watch couldn't classify.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmeasurable_minutes: Option<f64>,
    /// Number of times awake during the night.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awake_count: Option<u32>,
    /// Time spent napping during the day, in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nap_minutes: Option<f64>,
    /// Each nap.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub naps: Vec<Nap>,

    // --- Score ---
    /// Garmin sleep score, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_score: Option<u32>,
    /// Rating of the sleep score, e.g. `EXCELLENT`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_score_rating: Option<String>,
    /// Rating of the total sleep time and its optimal range in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_optimal_minutes: Option<[f64; 2]>,
    /// Share of sleep in each stage (percent), its rating, and the optimal
    /// range in percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_percent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_optimal_percent: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_percent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_optimal_percent: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rem_percent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rem_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rem_optimal_percent: Option<[f64; 2]>,
    /// Rating and optimal range of the other score factors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_optimal: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awake_count_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awake_count_optimal: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restlessness_rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restlessness_optimal: Option<[f64; 2]>,
    /// Garmin's feedback code for the night.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Garmin's insight codes for the night.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personalized_insight: Option<String>,

    // --- Body measurements ---
    /// Heart rate during sleep, bpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_heart_rate: Option<f64>,
    /// Breaths per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_respiration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_respiration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_respiration: Option<f64>,
    /// Blood oxygen saturation, percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_spo2_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_spo2_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_spo2_percent: Option<f64>,
    /// Heart rate while SpO2 was measured, bpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_spo2_heart_rate: Option<f64>,
    /// SpO2 readings below the alert threshold, and that threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spo2_events_below_threshold: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spo2_threshold_percent: Option<u32>,
    /// `NONE`, `LOW`, `MEDIUM`, `HIGH` or `UNMEASURABLE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breathing_disruption: Option<String>,
    /// Average stress level during sleep (0–100, lower is calmer).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_sleep_stress: Option<f64>,

    // --- Sleep need and timing ---
    /// Recommended sleep for this night, and the usual baseline, in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_baseline_minutes: Option<u32>,
    /// Why the need differs from the baseline, e.g. `INCREASED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_feedback: Option<String>,
    /// Individual reasons; left out when `NO_CHANGE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_training_adjustment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_sleep_history_adjustment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_hrv_adjustment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_need_nap_adjustment: Option<String>,
    /// Bedtime window Garmin recommended for this night, local "HH:MM".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_bedtime: Option<[String; 2]>,
    /// Recommended sleep and bedtime for the coming night.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_sleep_need_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_sleep_need_feedback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_recommended_bedtime: Option<[String; 2]>,
    /// Whether sleep timing matches the usual rhythm, e.g. `ALIGNED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_alignment: Option<String>,
    /// The user's optimal sleep window, local "HH:MM".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimal_sleep_window: Option<[String; 2]>,
    /// Middle of the optimal window vs. middle of this night's sleep.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimal_sleep_midpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sleep_midpoint: Option<String>,

    /// The user's own note about the night.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One nap, in output units.
#[derive(Debug, Serialize)]
pub struct Nap {
    /// Local time, "YYYY-MM-DD HH:MM:SS".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minutes: Option<f64>,
    /// Garmin's code for the nap, e.g. `IDEAL_TIMING_IDEAL_DURATION`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

impl DailySleep {
    /// Flattens Garmin's response into a [`SleepSummary`].
    ///
    /// Takes `self` by value (no `&`): the raw struct is *consumed*, so its
    /// strings can be moved into the summary instead of copied.
    ///
    /// Returns `None` if Garmin had no sleep data for the day.
    pub fn into_summary(self) -> Option<SleepSummary> {
        // `?` also works on `Option`: if `calendar_date` is `None`, this
        // function returns `None` right here.
        let calendar_date = self.calendar_date?;

        // Replace missing nested objects with empty defaults, so the code
        // below doesn't need to check each level for `None`.
        // `unwrap_or_default()` gives `SleepScores::default()` if `None`.
        let scores = self.sleep_scores.unwrap_or_default();
        let overall = scores.overall.unwrap_or_default();
        let duration = scores.total_duration.unwrap_or_default();
        let deep = scores.deep_percentage.unwrap_or_default();
        let light = scores.light_percentage.unwrap_or_default();
        let rem = scores.rem_percentage.unwrap_or_default();
        let stress = scores.stress.unwrap_or_default();
        let awake = scores.awake_count.unwrap_or_default();
        let restless = scores.restlessness.unwrap_or_default();
        let need = self.sleep_need.unwrap_or_default();
        let next = self.next_sleep_need.unwrap_or_default();
        let alignment = self.sleep_alignment.unwrap_or_default();

        Some(SleepSummary {
            calendar_date,
            // `.and_then(f)` runs `f` only if there is a value, and `f` may
            // itself return `None` (here: if the timestamp is out of range).
            sleep_start_local: self.sleep_start_timestamp_local.and_then(local_time),
            sleep_end_local: self.sleep_end_timestamp_local.and_then(local_time),

            // Garmin gives seconds; `minutes` converts (see below). `0`
            // means "none", e.g. no unmeasurable time.
            total_sleep_minutes: minutes(self.sleep_time_seconds),
            deep_sleep_minutes: minutes(self.deep_sleep_seconds),
            light_sleep_minutes: minutes(self.light_sleep_seconds),
            rem_sleep_minutes: minutes(self.rem_sleep_seconds),
            awake_minutes: minutes(self.awake_sleep_seconds),
            unmeasurable_minutes: minutes(self.unmeasurable_sleep_seconds),
            // A night without waking is worth reporting, so `0` is kept.
            awake_count: self.awake_count,
            nap_minutes: minutes(self.nap_time_seconds),
            naps: self.naps.into_iter().map(RawNap::into_summary).collect(),

            sleep_score: overall.value,
            sleep_score_rating: overall.qualifier_key,
            duration_rating: duration.qualifier_key.clone(),
            // Seconds to minutes.
            duration_optimal_minutes: duration.range(1.0 / 60.0),
            deep_percent: deep.value,
            deep_optimal_percent: deep.range(1.0),
            deep_rating: deep.qualifier_key,
            light_percent: light.value,
            light_optimal_percent: light.range(1.0),
            light_rating: light.qualifier_key,
            rem_percent: rem.value,
            rem_optimal_percent: rem.range(1.0),
            rem_rating: rem.qualifier_key,
            stress_optimal: stress.range(1.0),
            stress_rating: stress.qualifier_key,
            awake_count_optimal: awake.range(1.0),
            awake_count_rating: awake.qualifier_key,
            restlessness_optimal: restless.range(1.0),
            restlessness_rating: restless.qualifier_key,
            feedback: self.sleep_score_feedback,
            insight: self.sleep_score_insight,
            personalized_insight: self
                .sleep_score_personalized_insight
                .filter(|s| s != "NOT_AVAILABLE"),

            avg_heart_rate: positive(self.avg_heart_rate),
            avg_respiration: rounded(self.average_respiration_value, 1),
            min_respiration: rounded(self.lowest_respiration_value, 1),
            max_respiration: rounded(self.highest_respiration_value, 1),
            avg_spo2_percent: rounded(self.average_spo2, 1),
            min_spo2_percent: positive(self.lowest_spo2),
            max_spo2_percent: positive(self.highest_spo2),
            avg_spo2_heart_rate: rounded(self.average_spo2_hr, 1),
            spo2_events_below_threshold: self.number_of_events_below_threshold,
            spo2_threshold_percent: count(self.spo2_threshold),
            breathing_disruption: self.breathing_disruption_severity,
            avg_sleep_stress: self.avg_sleep_stress,

            sleep_need_minutes: need.actual,
            sleep_need_baseline_minutes: need.baseline,
            sleep_need_feedback: need.feedback,
            sleep_need_training_adjustment: changed(need.training_feedback),
            sleep_need_sleep_history_adjustment: changed(need.sleep_history_adjustment),
            sleep_need_hrv_adjustment: changed(need.hrv_adjustment),
            sleep_need_nap_adjustment: changed(need.nap_adjustment),
            recommended_bedtime: window(
                need.recommended_bedtime_start_mins,
                need.recommended_bedtime_end_mins,
            ),
            next_sleep_need_minutes: next.actual,
            next_sleep_need_feedback: next.feedback,
            next_recommended_bedtime: window(
                next.recommended_bedtime_start_mins,
                next.recommended_bedtime_end_mins,
            ),
            sleep_alignment: alignment.status,
            optimal_sleep_window: window(
                alignment.optimal_sleep_window_start_mins,
                alignment.optimal_sleep_window_end_mins,
            ),
            optimal_sleep_midpoint: alignment.optimal_sleep_window_midpoint_mins.map(clock),
            last_sleep_midpoint: alignment.last_sleep_midpoint_mins.map(clock),

            // An empty note says nothing.
            note: self.user_note.filter(|n| !n.trim().is_empty()),
        })
    }
}

impl RawNap {
    fn into_summary(self) -> Nap {
        Nap {
            start_local: utc_to_local(self.nap_start, self.nap_start_time_offset),
            end_local: utc_to_local(self.nap_end, self.nap_end_time_offset),
            minutes: minutes(self.nap_time_sec),
            feedback: self.nap_feedback,
        }
    }
}

/// Converts whole seconds to minutes, rounded to 0.1 (6 s); `0` → `None`.
fn minutes(seconds: Option<u32>) -> Option<f64> {
    // `f64::from` converts the integer to a float without any loss.
    count(seconds).map(|s| round(f64::from(s) / 60.0, 1))
}

/// Drops `NO_CHANGE`: only an actual adjustment is worth reporting.
fn changed(code: Option<String>) -> Option<String> {
    code.filter(|c| c != "NO_CHANGE")
}

/// Formats minutes after local midnight as "HH:MM", e.g. `40` -> "00:40".
/// Values outside one day wrap around: `-30` -> "23:30" (the evening before).
fn clock(minutes: i64) -> String {
    // `rem_euclid` is a remainder that is never negative, unlike `%`.
    let m = minutes.rem_euclid(24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Builds a `["HH:MM", "HH:MM"]` window, or `None` unless both ends exist.
fn window(start: Option<i64>, end: Option<i64>) -> Option<[String; 2]> {
    // `?` on each `Option` returns `None` from the function if it's missing.
    Some([clock(start?), clock(end?)])
}

/// Formats one of Garmin's "local" timestamps as "YYYY-MM-DD HH:MM:SS".
///
/// Garmin's `...TimestampLocal` values are milliseconds since 1970 that have
/// already been shifted by the user's time-zone offset. Reading them as if
/// they were UTC therefore gives the local wall-clock time.
fn local_time(millis: i64) -> Option<String> {
    // `from_timestamp_millis` returns `None` for impossible values.
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|dt| dt.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Turns a UTC time text like "2026-10-23T13:05:00.0" plus an offset in
/// minutes into local "YYYY-MM-DD HH:MM:SS". `None` if either is missing or
/// the text can't be read.
fn utc_to_local(utc: Option<String>, offset_minutes: Option<i64>) -> Option<String> {
    let utc = utc?;
    // `%.f` matches an optional fraction of a second, like ".0".
    // `.ok()` turns the parse `Result` into an `Option`, dropping the error.
    let time = NaiveDateTime::parse_from_str(&utc, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    let local = time + chrono::Duration::minutes(offset_minutes.unwrap_or(0));
    Some(local.format("%Y-%m-%d %H:%M:%S").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed-down copy of a real `dailySleep` response.
    const SLEEP_JSON: &str = r#"{
        "id": 1792794849000,
        "userProfilePK": 1,
        "calendarDate": "2026-10-24",
        "sleepTimeSeconds": 29760,
        "napTimeSeconds": 0,
        "sleepStartTimestampLocal": 1792802049000,
        "sleepEndTimestampLocal": 1792834209000,
        "unmeasurableSleepSeconds": 0,
        "deepSleepSeconds": 5940,
        "lightSleepSeconds": 18480,
        "remSleepSeconds": 5340,
        "awakeSleepSeconds": 2400,
        "averageRespirationValue": 12.0,
        "lowestRespirationValue": 8.0,
        "highestRespirationValue": 19.0,
        "awakeCount": 2,
        "avgSleepStress": 12.0,
        "ageGroup": "ADULT",
        "avgHeartRate": 53.0,
        "sleepScoreFeedback": "POSITIVE_LONG_AND_DEEP",
        "sleepScoreInsight": "POSITIVE_LATE_BED_TIME",
        "sleepScorePersonalizedInsight": "NOT_AVAILABLE",
        "sleepScores": {
            "totalDuration": {"qualifierKey": "EXCELLENT", "optimalStart": 29400.0, "optimalEnd": 29400.0},
            "stress": {"qualifierKey": "EXCELLENT", "optimalStart": 0.0, "optimalEnd": 15.0},
            "awakeCount": {"qualifierKey": "FAIR", "optimalStart": 0.0, "optimalEnd": 1.0},
            "overall": {"value": 87, "qualifierKey": "GOOD"},
            "remPercentage": {"value": 18, "qualifierKey": "FAIR", "optimalStart": 21.0, "optimalEnd": 31.0,
                              "idealStartInSeconds": 6249.6, "idealEndInSeconds": 9225.6},
            "restlessness": {"qualifierKey": "FAIR", "optimalStart": 0.0, "optimalEnd": 5.0},
            "deepPercentage": {"value": 20, "qualifierKey": "EXCELLENT", "optimalStart": 16.0, "optimalEnd": 33.0}
        },
        "sleepNeed": {"baseline": 490, "actual": 490, "feedback": "NO_CHANGE_NO_ADJUSTMENTS",
                      "trainingFeedback": "NO_CHANGE", "hrvAdjustment": "DECREASED",
                      "recommendedBedtimeStartMins": 40, "recommendedBedtimeEndMins": 60},
        "nextSleepNeed": {"baseline": 490, "actual": 500, "feedback": "INCREASED",
                          "recommendedBedtimeStartMins": -30, "recommendedBedtimeEndMins": 10},
        "sleepAlignment": {"status": "ALIGNED", "optimalSleepWindowStartMins": 10,
                           "optimalSleepWindowEndMins": 500, "optimalSleepWindowMidpointMins": 255,
                           "lastSleepMidpointMins": 302},
        "dailyNapDTOS": [
            {"napTimeSec": 1819, "napStartTimestampGMT": "2026-10-23T12:05:00.0",
             "napEndTimestampGMT": "2026-10-23T12:35:19.0", "napStartTimeOffset": 120,
             "napEndTimeOffset": 120, "napFeedback": "IDEAL_TIMING_IDEAL_DURATION"}
        ]
    }"#;

    fn sleep() -> SleepSummary {
        let raw: DailySleep = serde_json::from_str(SLEEP_JSON).unwrap();
        // `.expect(msg)` is like `.unwrap()` but with a custom panic message.
        raw.into_summary().expect("has data")
    }

    #[test]
    fn flattens_sleep_response() {
        let s = sleep();
        assert_eq!(s.calendar_date, "2026-10-24");
        assert_eq!(s.sleep_score, Some(87));
        assert_eq!(s.sleep_score_rating.as_deref(), Some("GOOD"));
        // 29760 s = 496 min, 5940 s = 99 min.
        assert_eq!(s.total_sleep_minutes, Some(496.0));
        assert_eq!(s.deep_sleep_minutes, Some(99.0));
        assert_eq!(s.min_respiration, Some(8.0));
        assert_eq!(s.max_respiration, Some(19.0));
        assert_eq!(s.sleep_start_local.as_deref(), Some("2026-10-24 00:34:09"));
    }

    #[test]
    fn score_factors_with_optimal_ranges() {
        let s = sleep();
        assert_eq!(s.deep_percent, Some(20));
        assert_eq!(s.deep_rating.as_deref(), Some("EXCELLENT"));
        assert_eq!(s.deep_optimal_percent, Some([16.0, 33.0]));
        assert_eq!(s.rem_optimal_percent, Some([21.0, 31.0]));
        // Light was missing from the JSON.
        assert_eq!(s.light_percent, None);
        // 29400 s = 490 min.
        assert_eq!(s.duration_optimal_minutes, Some([490.0, 490.0]));
        assert_eq!(s.restlessness_rating.as_deref(), Some("FAIR"));
        assert_eq!(s.awake_count_optimal, Some([0.0, 1.0]));
    }

    #[test]
    fn sleep_need_and_timing() {
        let s = sleep();
        assert_eq!(s.sleep_need_minutes, Some(490));
        assert_eq!(s.sleep_need_baseline_minutes, Some(490));
        // `NO_CHANGE` is dropped, a real adjustment is kept.
        assert_eq!(s.sleep_need_training_adjustment, None);
        assert_eq!(s.sleep_need_hrv_adjustment.as_deref(), Some("DECREASED"));
        assert_eq!(
            s.recommended_bedtime,
            Some(["00:40".to_string(), "01:00".to_string()])
        );
        assert_eq!(s.next_sleep_need_minutes, Some(500));
        // -30 min = 23:30 the evening before.
        assert_eq!(
            s.next_recommended_bedtime,
            Some(["23:30".to_string(), "00:10".to_string()])
        );
        assert_eq!(s.sleep_alignment.as_deref(), Some("ALIGNED"));
        assert_eq!(
            s.optimal_sleep_window,
            Some(["00:10".to_string(), "08:20".to_string()])
        );
        assert_eq!(s.last_sleep_midpoint.as_deref(), Some("05:02"));
    }

    #[test]
    fn naps_in_local_time() {
        let s = sleep();
        assert_eq!(s.naps.len(), 1);
        // 12:05 UTC + 120 min = 14:05 local.
        assert_eq!(
            s.naps[0].start_local.as_deref(),
            Some("2026-10-23 14:05:00")
        );
        // 1819 s = 30.3 min.
        assert_eq!(s.naps[0].minutes, Some(30.3));
    }

    #[test]
    fn output_skips_empty_values() {
        let out = serde_json::to_value(sleep()).unwrap();
        // 0 s and `NOT_AVAILABLE` say nothing, so they are left out; ids
        // and `ageGroup` are never read.
        assert!(out.get("nap_minutes").is_none());
        assert!(out.get("unmeasurable_minutes").is_none());
        assert!(out.get("personalized_insight").is_none());
        assert!(out.get("avg_spo2_percent").is_none());
        assert!(out.get("ageGroup").is_none());
        assert!(out.get("id").is_none());
    }

    #[test]
    fn all_null_sleep_response_means_no_data() {
        // What Garmin sends for a day without sleep data.
        let json = r#"{"calendarDate": null, "sleepTimeSeconds": null, "sleepScores": null}"#;
        let raw: DailySleep = serde_json::from_str(json).unwrap();
        assert!(raw.into_summary().is_none());
    }

    #[test]
    fn null_nap_list_reads_as_no_naps() {
        // The sleep range sends `"dailyNapDTOS": null` on some nights; a
        // plain `Vec` would fail with "invalid type: null, expected a sequence".
        let json =
            r#"{"calendarDate": "2026-11-01", "sleepTimeSeconds": 27000, "dailyNapDTOS": null}"#;
        let raw: DailySleep = serde_json::from_str(json).unwrap();
        let summary = raw.into_summary().unwrap();
        assert!(summary.naps.is_empty());
    }
}
