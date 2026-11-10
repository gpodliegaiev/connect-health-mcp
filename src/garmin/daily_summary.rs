//! Types for daily wellness summaries:
//! `GET /usersummary-service/v2/dailysummaries?startDate=&endDate=` (one
//! entry per day, grouped into nested objects) and
//! `GET /usersummary-service/usersummary/daily?calendarDate=` (one day,
//! flat, with a few extra fields).
//!
//! A daily summary is the all-day picture outside workouts: steps, floors,
//! calories, heart rate (incl. resting), intensity minutes, stress, Body
//! Battery with what charged and drained it, respiration, SpO2 and
//! hydration. Both responses are converted into the same [`DaySummary`].
//! The single-day response adds: Body Battery at wake time and charged
//! during sleep, 7-day average resting HR, absolute min/max HR, time per
//! activity level (highly active, active, sedentary, sleeping), total
//! distance including activities, and the activity behind each Body Battery
//! event.
//!
//! Found by probing (the spec is silent):
//! - The range may span at most 365 days between the dates; longer,
//!   reversed or missing dates return 400 `{}`. Days without data are
//!   absent (`{"data": []}` if none).
//! - A year of raw data is about 1 MB (3 KB per day), mostly Body Battery
//!   events and duplicated values; the converted output is much smaller.
//! - The single-day endpoint needs `calendarDate` (400 without it). A day
//!   without data still returns 200 with only `calendarDate`, `source` and
//!   `includes*Data: false`.
//! - Heart rate `minValue`/`maxValue` in the range response are 2-minute
//!   averages (they match `minAvgHeartRate`/`maxAvgHeartRate` of the single
//!   day), and respiration `avgValue` is the waking average.
//! - Hydration has a goal every day, but `consumed` is 0 unless the user
//!   logs drinks, so hydration is only output when something was logged.
//!
//! Left out on purpose: ids and UUIDs, `source`, `includes*Data` flags, GMT
//! copies of local times, `lastSyncTimestampGMT`, device ids, algorithm and
//! feature versions, stress percentages / proportions (they follow from the
//! stress minutes), the "latest" respiration and SpO2 readings (one moment
//! of the day), `feedbackShortType` (a shorter copy of `feedbackLongType`),
//! fractional copies of hydration values, and `displayName`.

use chrono::{NaiveDateTime, TimeDelta};
use serde::{Deserialize, Serialize};

use super::types::{count, null_as_default, positive, round, to_minutes};

// ---------------------------------------------------------------------------
// Range response (v2): nested objects
// ---------------------------------------------------------------------------

/// The range response: `{"data": [ ...one entry per day... ]}`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct DailySummaries {
    #[serde(deserialize_with = "null_as_default")]
    data: Vec<RawDay>,
}

/// One day of the range response, as it arrives. Related values are grouped
/// into nested objects; a missing object behaves like an empty one
/// (`default`).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawDay {
    calendar_date: Option<String>,
    wellness_chronology: RawChronology,
    movement: RawMovement,
    calories: RawCalories,
    heart_rate: RawHeartRate,
    intensity_minutes: RawIntensityMinutes,
    stress: RawStress,
    body_battery: RawBodyBattery,
    hydration: RawHydration,
    respiration: RawRespiration,
    pulse_ox: RawPulseOx,
    jet_lag: RawJetLag,
}

/// When the watch recorded data during the day.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawChronology {
    end_timestamp_local: Option<String>,
    total_duration_in_millis: Option<f64>,
}

/// Steps, wheelchair pushes and floors.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawMovement {
    steps: RawMovementValue,
    pushes: RawMovementValue,
    floors_ascended: RawMovementValue,
    floors_descended: RawMovementValue,
}

/// One kind of movement: the day's value, its goal and the distance (m).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawMovementValue {
    goal: Option<u32>,
    value: Option<u32>,
    distance_in_meters: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawCalories {
    burned_resting: Option<u32>,
    burned_active: Option<u32>,
    burned_total: Option<u32>,
    consumed_goal: Option<u32>,
    consumed_value: Option<u32>,
    consumed_remaining: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawHeartRate {
    /// Lowest 2-minute average.
    min_value: Option<u32>,
    /// Highest 2-minute average.
    max_value: Option<u32>,
    resting_value: Option<u32>,
    alert_count: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawIntensityMinutes {
    /// Weekly goal.
    goal: Option<u32>,
    moderate: Option<u32>,
    vigorous: Option<u32>,
}

/// Stress level (0–100) and time per stress category, in milliseconds.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawStress {
    avg_level: Option<u32>,
    max_level: Option<u32>,
    qualifier: Option<String>,
    rest_duration_in_millis: Option<f64>,
    activity_duration_in_millis: Option<f64>,
    uncategorized_duration_in_millis: Option<f64>,
    low_stress_duration_in_millis: Option<f64>,
    medium_stress_duration_in_millis: Option<f64>,
    high_stress_duration_in_millis: Option<f64>,
}

/// Body Battery (5–100) for the day.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawBodyBattery {
    min_value: Option<u32>,
    max_value: Option<u32>,
    charged_value: Option<u32>,
    drained_value: Option<u32>,
    latest_value: Option<u32>,
    /// Latest value before sleep (in the spec; not seen in real data).
    end_of_day_value: Option<u32>,
    dynamic_feedback_event: Option<RawFeedback>,
    end_of_day_dynamic_feedback_event: Option<RawFeedback>,
    #[serde(deserialize_with = "null_as_default")]
    activity_events: Vec<RawEventV2>,
}

/// Garmin's Body Battery comment, e.g. level `HIGH` with
/// `SLEEP_PREPARATION_BALANCED_AND_EXERCISE`. Same shape in both responses.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct RawFeedback {
    body_battery_level: Option<String>,
    feedback_long_type: Option<String>,
}

/// Something that charged or drained Body Battery (range response).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawEventV2 {
    /// `SLEEP`, `ACTIVITY`, `RECOVERY`, `STRESS` or `NAP`.
    event_type: Option<String>,
    event_start_time_local: Option<String>,
    duration_in_millis: Option<f64>,
    body_battery_impact: Option<i32>,
    feedback_type: Option<String>,
    short_feedback: Option<String>,
    activity_name: Option<String>,
    activity_type: Option<String>,
    activity_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawHydration {
    goal_in_ml: Option<f64>,
    consumed_in_ml: Option<f64>,
}

/// Breaths per minute.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawRespiration {
    /// Waking average.
    avg_value: Option<f64>,
    max_value: Option<f64>,
    min_value: Option<f64>,
}

/// Blood oxygen, percent, plus the altitude the watch was at.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawPulseOx {
    avg_value: Option<f64>,
    min_value: Option<f64>,
    avg_altitude_in_meters: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawJetLag {
    trip_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Single-day response (v1): one flat object
// ---------------------------------------------------------------------------

/// The single-day response, as it arrives. Flat: every value is a top-level
/// field. Only the fields that are output are listed; serde ignores the rest.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserDailySummary {
    calendar_date: Option<String>,
    includes_wellness_data: Option<bool>,
    includes_activity_data: Option<bool>,
    wellness_end_time_local: Option<String>,
    duration_in_milliseconds: Option<f64>,

    total_steps: Option<u32>,
    daily_step_goal: Option<u32>,
    wellness_distance_meters: Option<f64>,
    total_distance_meters: Option<f64>,
    total_pushes: Option<u32>,
    daily_pushes_goal: Option<u32>,
    total_push_distance: Option<f64>,
    /// Fractional here (154.26), whole in the range response.
    floors_ascended: Option<f64>,
    floors_descended: Option<f64>,
    user_floors_ascended_goal: Option<u32>,

    total_kilocalories: Option<f64>,
    active_kilocalories: Option<f64>,
    bmr_kilocalories: Option<f64>,
    consumed_kilocalories: Option<f64>,
    net_calorie_goal: Option<f64>,
    remaining_kilocalories: Option<f64>,

    min_heart_rate: Option<u32>,
    max_heart_rate: Option<u32>,
    min_avg_heart_rate: Option<u32>,
    max_avg_heart_rate: Option<u32>,
    resting_heart_rate: Option<u32>,
    last_seven_days_avg_resting_heart_rate: Option<u32>,
    abnormal_heart_rate_alerts_count: Option<u32>,

    moderate_intensity_minutes: Option<u32>,
    vigorous_intensity_minutes: Option<u32>,
    intensity_minutes_goal: Option<u32>,
    highly_active_seconds: Option<f64>,
    active_seconds: Option<f64>,
    sedentary_seconds: Option<f64>,
    sleeping_seconds: Option<f64>,

    average_stress_level: Option<u32>,
    max_stress_level: Option<u32>,
    stress_qualifier: Option<String>,
    /// Stress durations, in seconds (not milliseconds as in the range).
    rest_stress_duration: Option<f64>,
    activity_stress_duration: Option<f64>,
    uncategorized_stress_duration: Option<f64>,
    low_stress_duration: Option<f64>,
    medium_stress_duration: Option<f64>,
    high_stress_duration: Option<f64>,

    body_battery_highest_value: Option<u32>,
    body_battery_lowest_value: Option<u32>,
    body_battery_charged_value: Option<u32>,
    body_battery_drained_value: Option<u32>,
    body_battery_most_recent_value: Option<u32>,
    body_battery_at_wake_time: Option<u32>,
    body_battery_during_sleep: Option<u32>,
    body_battery_dynamic_feedback_event: Option<RawFeedback>,
    end_of_day_body_battery_dynamic_feedback_event: Option<RawFeedback>,
    #[serde(deserialize_with = "null_as_default")]
    body_battery_activity_event_list: Vec<RawEventV1>,

    avg_waking_respiration_value: Option<f64>,
    highest_respiration_value: Option<f64>,
    lowest_respiration_value: Option<f64>,
    average_spo2: Option<f64>,
    lowest_spo2: Option<f64>,
    average_monitoring_environment_altitude: Option<f64>,
    #[serde(rename = "hydrationValueInML")]
    hydration_value_in_ml: Option<f64>,
    #[serde(rename = "hydrationGoalInML")]
    hydration_goal_in_ml: Option<f64>,
    jet_lag_trip_name: Option<String>,
}

/// A Body Battery event in the single-day response: GMT start plus the
/// time-zone offset in ms, and the activity behind it. Also used by
/// `body_battery.rs`, whose events have the same shape (without the
/// activity fields).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct RawEventV1 {
    event_type: Option<String>,
    event_start_time_gmt: Option<String>,
    timezone_offset: Option<i64>,
    duration_in_milliseconds: Option<f64>,
    body_battery_impact: Option<i32>,
    feedback_type: Option<String>,
    short_feedback: Option<String>,
    activity_name: Option<String>,
    activity_type: Option<String>,
    activity_id: Option<u64>,
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// One day, as the `get_daily_summary` tool returns it. Heart rate in bpm,
/// stress and Body Battery on Garmin's 0–100 scale, durations in minutes.
///
/// `Default` lets each conversion fill only the fields its response has
/// (`..Default::default()` sets the rest to `None` / empty).
#[derive(Debug, Default, Serialize)]
pub struct DaySummary {
    pub date: String,
    /// Hours of data, only when less than a full day (e.g. today so far).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracked_hours: Option<f64>,
    /// Time of the last data, only on a partial day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracked_until_local: Option<String>,

    // --- Movement ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_goal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps_distance_km: Option<f64>,
    /// Including activities. Only when it differs from the steps distance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_distance_km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pushes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_goal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_distance_km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floors_ascended: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floors_descended: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floors_goal: Option<u32>,

    // --- Calories ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_total_kcal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_active_kcal: Option<u32>,
    /// Basal metabolic rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_resting_kcal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_consumed_kcal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_consumed_goal_kcal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_remaining_kcal: Option<i32>,

    // --- Heart rate ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr_7day_avg: Option<u32>,
    /// Lowest / highest 2-minute average.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_hr_2min_avg: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr_2min_avg: Option<u32>,
    /// Absolute lowest / highest reading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_hr: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abnormal_hr_alerts: Option<u32>,

    // --- Activity ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderate_intensity_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vigorous_intensity_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity_minutes_weekly_goal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highly_active_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sedentary_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleeping_min: Option<f64>,

    // --- Stress ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_avg: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_max: Option<u32>,
    /// E.g. `BALANCED`, `STRESSFUL`, `CALM_AWAKE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_qualifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_rest_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_low_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_medium_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_high_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_activity_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stress_uncategorized_min: Option<f64>,

    // --- Body Battery ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_highest: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_lowest: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_charged: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_drained: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_latest: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_at_wake: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_charged_during_sleep: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_end_of_day: Option<u32>,
    /// Garmin's latest comment, e.g. `HIGH: DAY_NOT_STRESS_DATA_AND_EXERCISE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_feedback: Option<String>,
    /// Garmin's bedtime comment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_battery_end_of_day_feedback: Option<String>,
    /// What charged or drained Body Battery, in time order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub body_battery_events: Vec<BodyBatteryEvent>,

    // --- Other ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waking_respiration_avg_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respiration_min_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respiration_max_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spo2_avg_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spo2_min_percent: Option<f64>,
    /// Average altitude the watch was at during the day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_altitude_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hydration_ml: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hydration_goal_ml: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jet_lag_trip: Option<String>,
}

/// One Body Battery event.
#[derive(Debug, Serialize)]
pub struct BodyBatteryEvent {
    /// `SLEEP`, `ACTIVITY`, `RECOVERY`, `STRESS` or `NAP`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    /// "YYYY-MM-DD HH:MM", local time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<f64>,
    /// Change in Body Battery: positive charged, negative drained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<i32>,
    /// E.g. `EXERCISE_TRAINING_EFFECT_3`, `RECOVERY_BODY_BATTERY_INCREASE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// E.g. `IMPROVING_TEMPO`, `RESTFUL_PERIOD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_feedback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<u64>,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl DailySummaries {
    /// Converts the range response into one [`DaySummary`] per day.
    pub fn into_summary(self) -> Vec<DaySummary> {
        self.data
            .into_iter()
            .filter_map(RawDay::into_summary)
            .collect()
    }
}

impl RawDay {
    /// Converts one day; `None` if it has no date.
    fn into_summary(self) -> Option<DaySummary> {
        // Destructuring: pull the nested objects out into local variables,
        // so the struct literal below is shorter to read.
        let RawDay {
            movement: m,
            calories: c,
            heart_rate: hr,
            intensity_minutes: im,
            stress: s,
            body_battery: bb,
            ..
        } = self;
        let (tracked_hours, tracked_until_local) = partial_day(
            self.wellness_chronology.total_duration_in_millis,
            self.wellness_chronology.end_timestamp_local,
        );
        // Milliseconds to minutes.
        let ms = |v: Option<f64>| to_minutes(v.map(|v| v / 1000.0));
        let hydration = logged_hydration(self.hydration.consumed_in_ml, self.hydration.goal_in_ml);

        Some(DaySummary {
            date: self.calendar_date?,
            tracked_hours,
            tracked_until_local,
            steps: m.steps.value,
            step_goal: m.steps.goal,
            steps_distance_km: km(m.steps.distance_in_meters),
            pushes: count(m.pushes.value),
            push_goal: count(m.pushes.value).and(m.pushes.goal),
            push_distance_km: km(m.pushes.distance_in_meters),
            floors_ascended: m.floors_ascended.value,
            floors_descended: m.floors_descended.value,
            floors_goal: m.floors_ascended.goal,
            calories_total_kcal: c.burned_total,
            calories_active_kcal: c.burned_active,
            calories_resting_kcal: c.burned_resting,
            calories_consumed_kcal: count(c.consumed_value),
            calories_consumed_goal_kcal: c.consumed_goal,
            calories_remaining_kcal: c.consumed_remaining,
            resting_hr: hr.resting_value,
            min_hr_2min_avg: hr.min_value,
            max_hr_2min_avg: hr.max_value,
            abnormal_hr_alerts: count(hr.alert_count),
            moderate_intensity_minutes: im.moderate,
            vigorous_intensity_minutes: im.vigorous,
            intensity_minutes_weekly_goal: im.goal,
            stress_avg: s.avg_level,
            stress_max: s.max_level,
            stress_qualifier: qualifier(s.qualifier),
            stress_rest_min: ms(s.rest_duration_in_millis),
            stress_low_min: ms(s.low_stress_duration_in_millis),
            stress_medium_min: ms(s.medium_stress_duration_in_millis),
            stress_high_min: ms(s.high_stress_duration_in_millis),
            stress_activity_min: ms(s.activity_duration_in_millis),
            stress_uncategorized_min: ms(s.uncategorized_duration_in_millis),
            body_battery_highest: bb.max_value,
            body_battery_lowest: bb.min_value,
            body_battery_charged: bb.charged_value,
            body_battery_drained: bb.drained_value,
            body_battery_latest: bb.latest_value,
            body_battery_end_of_day: bb.end_of_day_value,
            body_battery_feedback: bb.dynamic_feedback_event.and_then(RawFeedback::text),
            body_battery_end_of_day_feedback: bb
                .end_of_day_dynamic_feedback_event
                .and_then(RawFeedback::text),
            body_battery_events: bb
                .activity_events
                .into_iter()
                .map(RawEventV2::into_event)
                .collect(),
            waking_respiration_avg_brpm: positive(self.respiration.avg_value),
            respiration_min_brpm: positive(self.respiration.min_value),
            respiration_max_brpm: positive(self.respiration.max_value),
            spo2_avg_percent: positive(self.pulse_ox.avg_value),
            spo2_min_percent: positive(self.pulse_ox.min_value),
            avg_altitude_m: self.pulse_ox.avg_altitude_in_meters,
            hydration_ml: hydration.0,
            hydration_goal_ml: hydration.1,
            jet_lag_trip: self.jet_lag.trip_name,
            // Single-day-only fields stay `None`.
            ..Default::default()
        })
    }
}

impl RawEventV2 {
    fn into_event(self) -> BodyBatteryEvent {
        BodyBatteryEvent {
            event_type: self.event_type,
            start_local: self.event_start_time_local.as_deref().and_then(minute_text),
            duration_min: to_minutes(self.duration_in_millis.map(|v| v / 1000.0)),
            impact: self.body_battery_impact,
            feedback: not_none(self.feedback_type),
            short_feedback: not_none(self.short_feedback),
            activity_name: self.activity_name,
            activity_type: self.activity_type,
            activity_id: self.activity_id,
        }
    }
}

impl UserDailySummary {
    /// Converts the single-day response; `None` when Garmin has no data for
    /// the day (both `includes*Data` flags `false`).
    pub fn into_summary(self) -> Option<DaySummary> {
        if self.includes_wellness_data != Some(true) && self.includes_activity_data != Some(true) {
            return None;
        }
        let (tracked_hours, tracked_until_local) =
            partial_day(self.duration_in_milliseconds, self.wellness_end_time_local);
        let steps_distance_km = km(self.wellness_distance_meters);
        let total_distance_km = km(self.total_distance_meters);
        let hydration = logged_hydration(self.hydration_value_in_ml, self.hydration_goal_in_ml);

        Some(DaySummary {
            date: self.calendar_date?,
            tracked_hours,
            tracked_until_local,
            steps: self.total_steps,
            step_goal: self.daily_step_goal,
            steps_distance_km,
            // Only worth showing when activities added distance.
            total_distance_km: total_distance_km.filter(|t| Some(*t) != steps_distance_km),
            pushes: count(self.total_pushes),
            push_goal: count(self.total_pushes).and(self.daily_pushes_goal),
            push_distance_km: km(self.total_push_distance),
            floors_ascended: whole(self.floors_ascended),
            floors_descended: whole(self.floors_descended),
            floors_goal: self.user_floors_ascended_goal,
            calories_total_kcal: whole(self.total_kilocalories),
            calories_active_kcal: whole(self.active_kilocalories),
            calories_resting_kcal: whole(self.bmr_kilocalories),
            calories_consumed_kcal: whole(positive(self.consumed_kilocalories)),
            calories_consumed_goal_kcal: whole(self.net_calorie_goal),
            calories_remaining_kcal: self.remaining_kilocalories.map(|v| v.round() as i32),
            resting_hr: self.resting_heart_rate,
            resting_hr_7day_avg: self.last_seven_days_avg_resting_heart_rate,
            min_hr_2min_avg: self.min_avg_heart_rate,
            max_hr_2min_avg: self.max_avg_heart_rate,
            min_hr: self.min_heart_rate,
            max_hr: self.max_heart_rate,
            abnormal_hr_alerts: count(self.abnormal_heart_rate_alerts_count),
            moderate_intensity_minutes: self.moderate_intensity_minutes,
            vigorous_intensity_minutes: self.vigorous_intensity_minutes,
            intensity_minutes_weekly_goal: self.intensity_minutes_goal,
            highly_active_min: to_minutes(self.highly_active_seconds),
            active_min: to_minutes(self.active_seconds),
            sedentary_min: to_minutes(self.sedentary_seconds),
            sleeping_min: to_minutes(self.sleeping_seconds),
            stress_avg: self.average_stress_level,
            stress_max: self.max_stress_level,
            stress_qualifier: qualifier(self.stress_qualifier),
            stress_rest_min: to_minutes(self.rest_stress_duration),
            stress_low_min: to_minutes(self.low_stress_duration),
            stress_medium_min: to_minutes(self.medium_stress_duration),
            stress_high_min: to_minutes(self.high_stress_duration),
            stress_activity_min: to_minutes(self.activity_stress_duration),
            stress_uncategorized_min: to_minutes(self.uncategorized_stress_duration),
            body_battery_highest: self.body_battery_highest_value,
            body_battery_lowest: self.body_battery_lowest_value,
            body_battery_charged: self.body_battery_charged_value,
            body_battery_drained: self.body_battery_drained_value,
            body_battery_latest: self.body_battery_most_recent_value,
            body_battery_at_wake: self.body_battery_at_wake_time,
            body_battery_charged_during_sleep: self.body_battery_during_sleep,
            body_battery_end_of_day: None,
            body_battery_feedback: self
                .body_battery_dynamic_feedback_event
                .and_then(RawFeedback::text),
            body_battery_end_of_day_feedback: self
                .end_of_day_body_battery_dynamic_feedback_event
                .and_then(RawFeedback::text),
            body_battery_events: self
                .body_battery_activity_event_list
                .into_iter()
                .map(RawEventV1::into_event)
                .collect(),
            waking_respiration_avg_brpm: positive(self.avg_waking_respiration_value),
            respiration_min_brpm: positive(self.lowest_respiration_value),
            respiration_max_brpm: positive(self.highest_respiration_value),
            spo2_avg_percent: positive(self.average_spo2).map(|v| round(v, 1)),
            spo2_min_percent: positive(self.lowest_spo2),
            avg_altitude_m: self.average_monitoring_environment_altitude,
            hydration_ml: hydration.0,
            hydration_goal_ml: hydration.1,
            jet_lag_trip: self.jet_lag_trip_name,
        })
    }
}

impl RawEventV1 {
    pub(super) fn into_event(self) -> BodyBatteryEvent {
        // GMT start + offset (ms) = local start. `TimeDelta` is a length of
        // time that can be added to a date-time.
        let start_local = self.event_start_time_gmt.as_deref().and_then(|text| {
            let gmt = parse_time(text)?;
            let local = gmt + TimeDelta::milliseconds(self.timezone_offset.unwrap_or(0));
            Some(local.format("%Y-%m-%d %H:%M").to_string())
        });
        BodyBatteryEvent {
            event_type: self.event_type,
            start_local,
            duration_min: to_minutes(self.duration_in_milliseconds.map(|v| v / 1000.0)),
            impact: self.body_battery_impact,
            feedback: not_none(self.feedback_type),
            short_feedback: not_none(self.short_feedback),
            activity_name: self.activity_name,
            activity_type: self.activity_type,
            activity_id: self.activity_id,
        }
    }
}

impl RawFeedback {
    /// Joins level and comment: `"HIGH: SLEEP_PREPARATION_..."`. `None` if
    /// both are missing.
    pub(super) fn text(self) -> Option<String> {
        match (self.body_battery_level, self.feedback_long_type) {
            (Some(level), Some(feedback)) => Some(format!("{level}: {feedback}")),
            // `or` picks whichever one is present.
            (level, feedback) => level.or(feedback),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Meters to km with 3 decimals; `0` → `None`.
fn km(meters: Option<f64>) -> Option<f64> {
    positive(meters).map(|m| round(m / 1000.0, 3))
}

/// Rounds to a whole number, e.g. fractional floors `154.26` → `154`.
fn whole(value: Option<f64>) -> Option<u32> {
    // `as u32` turns the rounded float into an unsigned integer (negative
    // values become 0, which none of these fields have).
    value.map(|v| v.round() as u32)
}

/// For a day with less than 24 h of data (usually today), returns the hours
/// tracked and the time of the last data. A full day returns `(None, None)`.
fn partial_day(
    duration_ms: Option<f64>,
    end_local: Option<String>,
) -> (Option<f64>, Option<String>) {
    match duration_ms {
        // 86_400_000 ms = 24 h. `_` in a number is only a visual separator.
        Some(ms) if ms < 86_400_000.0 => (
            Some(round(ms / 3_600_000.0, 1)),
            end_local.as_deref().and_then(minute_text),
        ),
        _ => (None, None),
    }
}

/// Hydration `(consumed, goal)` in ml, only when the user logged a drink.
fn logged_hydration(consumed: Option<f64>, goal: Option<f64>) -> (Option<f64>, Option<f64>) {
    match positive(consumed) {
        Some(ml) => (Some(round(ml, 0)), goal.map(|g| round(g, 0))),
        None => (None, None),
    }
}

/// Upper-cases the stress qualifier (the range response sends lower case)
/// and drops `UNKNOWN`.
fn qualifier(text: Option<String>) -> Option<String> {
    text.map(|t| t.to_uppercase()).filter(|t| t != "UNKNOWN")
}

/// Drops Garmin's `NONE` placeholder.
fn not_none(text: Option<String>) -> Option<String> {
    text.filter(|t| t != "NONE")
}

/// Parses "2026-10-24T00:34:09.0" or "2026-10-24T00:34:09".
fn parse_time(text: &str) -> Option<NaiveDateTime> {
    // `.ok()` turns the `Result` into an `Option`, dropping the error.
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
}

/// "2026-10-24T00:34:09.0" → "2026-10-24 00:34".
fn minute_text(text: &str) -> Option<String> {
    parse_time(text).map(|t| t.format("%Y-%m-%d %H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real entry of the range response (values made up):
    /// a partial day (today).
    fn range_day() -> serde_json::Value {
        serde_json::json!({
            "uuid": "x", "userProfilePk": 1, "calendarDate": "2026-10-24", "source": "garmin",
            "includesWellnessData": true, "includesActivityData": true,
            "wellnessChronology": {"startTimestampLocal": "2026-10-24T00:00:00.0",
                "endTimestampLocal": "2026-10-24T15:14:00.0", "totalDurationInMillis": 54840000},
            "movement": {"steps": {"goal": 7500, "value": 7342, "distanceInMeters": 4421.0},
                "pushes": {}, "floorsAscended": {"goal": 10, "value": 154, "distanceInMeters": 470.19},
                "floorsDescended": {"value": 147, "distanceInMeters": 450.5}},
            "calories": {"burnedResting": 1612, "burnedActive": 488, "burnedTotal": 2100},
            "heartRate": {"minValue": 54, "maxValue": 141, "restingValue": 56},
            "intensityMinutes": {"goal": 150, "moderate": 34, "vigorous": 30},
            "stress": {"avgLevel": 18, "maxLevel": 90, "restProportion": 0.66, "qualifier": "unknown",
                "restDurationInMillis": 36180000, "activityDurationInMillis": 1740000,
                "uncategorizedDurationInMillis": 8340000, "lowStressDurationInMillis": 5160000,
                "mediumStressDurationInMillis": 2820000, "highStressDurationInMillis": 600000},
            "bodyBattery": {"minValue": 31, "maxValue": 100, "chargedValue": 69, "drainedValue": 40,
                "latestValue": 60, "featureVersion": "3.0",
                "dynamicFeedbackEvent": {"eventTimestampGmt": "2026-10-24T12:39:22.0",
                    "bodyBatteryLevel": "HIGH", "feedbackShortType": "DAY_NOT_STRESS_DATA_AND_EXERCISE",
                    "feedbackLongType": "DAY_NOT_STRESS_DATA_AND_EXERCISE"},
                "activityEvents": [{"eventType": "ACTIVITY", "eventStartTimeGmt": "2026-10-24T09:06:58.0",
                    "eventStartTimeLocal": "2026-10-24T11:06:58.0", "bodyBatteryImpact": -15,
                    "feedbackType": "EXERCISE_TRAINING_EFFECT_2", "shortFeedback": "MAINTAINING_AEROBIC_BASE",
                    "deviceId": 1, "durationInMillis": 4800000},
                    {"eventType": "SLEEP", "eventStartTimeLocal": "2026-10-24T00:34:09.0",
                     "bodyBatteryImpact": 69, "feedbackType": "NONE", "shortFeedback": "NONE",
                     "durationInMillis": 32160000}]},
            "hydration": {"goalInMl": 2950, "goalInFractionalMl": 2950.412, "consumedInMl": 0,
                "consumedInFractionalMl": 0.0},
            "respiration": {"avgValue": 17, "maxValue": 33, "minValue": 8, "latestValue": 19},
            "pulseOx": {"avgAltitudeInMeters": 91.0},
            "jetLag": {}
        })
    }

    #[test]
    fn range_day_is_converted() {
        let raw: DailySummaries =
            serde_json::from_value(serde_json::json!({"data": [range_day()]})).unwrap();
        let out = serde_json::to_value(raw.into_summary()).unwrap();
        assert_eq!(
            out,
            serde_json::json!([{
                "date": "2026-10-24", "tracked_hours": 15.2, "tracked_until_local": "2026-10-24 15:14",
                "steps": 7342, "step_goal": 7500, "steps_distance_km": 4.421,
                "floors_ascended": 154, "floors_descended": 147, "floors_goal": 10,
                "calories_total_kcal": 2100, "calories_active_kcal": 488, "calories_resting_kcal": 1612,
                "resting_hr": 56, "min_hr_2min_avg": 54, "max_hr_2min_avg": 141,
                "moderate_intensity_minutes": 34, "vigorous_intensity_minutes": 30,
                "intensity_minutes_weekly_goal": 150,
                "stress_avg": 18, "stress_max": 90,
                "stress_rest_min": 603.0, "stress_low_min": 86.0, "stress_medium_min": 47.0,
                "stress_high_min": 10.0, "stress_activity_min": 29.0, "stress_uncategorized_min": 139.0,
                "body_battery_highest": 100, "body_battery_lowest": 31, "body_battery_charged": 69,
                "body_battery_drained": 40, "body_battery_latest": 60,
                "body_battery_feedback": "HIGH: DAY_NOT_STRESS_DATA_AND_EXERCISE",
                "body_battery_events": [
                    {"type": "ACTIVITY", "start_local": "2026-10-24 11:06", "duration_min": 80.0,
                     "impact": -15, "feedback": "EXERCISE_TRAINING_EFFECT_2",
                     "short_feedback": "MAINTAINING_AEROBIC_BASE"},
                    {"type": "SLEEP", "start_local": "2026-10-24 00:34", "duration_min": 536.0, "impact": 69}
                ],
                "waking_respiration_avg_brpm": 17.0, "respiration_min_brpm": 8.0,
                "respiration_max_brpm": 33.0, "avg_altitude_m": 91.0
            }])
        );
    }

    #[test]
    fn single_day_adds_extras() {
        let raw: UserDailySummary = serde_json::from_value(serde_json::json!({
            "calendarDate": "2026-10-23", "includesWellnessData": true,
            "durationInMilliseconds": 86400000,
            "totalSteps": 12000, "wellnessDistanceMeters": 9000, "totalDistanceMeters": 15000,
            "floorsAscended": 154.2628, "totalKilocalories": 2500.0,
            "consumedKilocalories": null, "minHeartRate": 52, "maxHeartRate": 148,
            "lastSevenDaysAvgRestingHeartRate": 56, "highlyActiveSeconds": 2691,
            "stressQualifier": "BALANCED", "restStressDuration": 36180,
            "bodyBatteryAtWakeTime": 100, "bodyBatteryDuringSleep": 68,
            "hydrationValueInML": 0.0, "hydrationGoalInML": 2950.0,
            "bodyBatteryActivityEventList": [{"eventType": "ACTIVITY",
                "eventStartTimeGmt": "2026-10-24T09:06:58", "timezoneOffset": 7200000,
                "durationInMilliseconds": 4800000, "bodyBatteryImpact": -15,
                "feedbackType": "EXERCISE_TRAINING_EFFECT_2", "shortFeedback": "MAINTAINING_AEROBIC_BASE",
                "deviceId": 1, "activityName": "Floor Climb", "activityType": "floor_climbing",
                "activityId": 1003}]
        }))
        .unwrap();
        let out = serde_json::to_value(raw.into_summary().unwrap()).unwrap();
        assert_eq!(
            out,
            serde_json::json!({
                "date": "2026-10-23", "steps": 12000, "steps_distance_km": 9.0,
                "total_distance_km": 15.0, "floors_ascended": 154, "calories_total_kcal": 2500,
                "resting_hr_7day_avg": 56, "min_hr": 52, "max_hr": 148,
                "highly_active_min": 44.85, "stress_qualifier": "BALANCED", "stress_rest_min": 603.0,
                "body_battery_at_wake": 100, "body_battery_charged_during_sleep": 68,
                "body_battery_events": [{"type": "ACTIVITY", "start_local": "2026-10-24 11:06",
                    "duration_min": 80.0, "impact": -15, "feedback": "EXERCISE_TRAINING_EFFECT_2",
                    "short_feedback": "MAINTAINING_AEROBIC_BASE", "activity_name": "Floor Climb",
                    "activity_type": "floor_climbing", "activity_id": 1003}]
            })
        );
    }

    #[test]
    fn single_day_without_data_is_none() {
        let raw: UserDailySummary = serde_json::from_value(serde_json::json!({
            "calendarDate": "2015-01-01", "includesWellnessData": false,
            "includesActivityData": false, "source": "GARMIN"
        }))
        .unwrap();
        assert!(raw.into_summary().is_none());
    }
}
