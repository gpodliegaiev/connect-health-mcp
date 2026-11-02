//! Types for one activity's full details
//! (`GET /activity-service/activity/{activityId}`).
//!
//! Same two-step pattern as `types.rs`: [`ActivityDetail`] mirrors Garmin's
//! response (nested, Garmin units), and [`ActivityDetail::into_summary`]
//! turns it into the flat [`ActivityDetailSummary`] the tool returns, with
//! the unit in every field name.
//!
//! Nearly every metric here is also in the activity list entry
//! (`activity.rs`). Details add the time zone, min and recovery heart rate,
//! average moving pace, stamina (start, end, lowest), multisport leg ids,
//! and richer split totals (heart rate, power, cadence, calories per split).

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use super::types::{
    SpeedStyle, benefit, count, event_type, null_as_default, only_true, positive, round, rounded,
    to_minutes,
};

/// Garmin's activity details response, as it arrives.
///
/// Only used for reading. The nested objects have names ending in `DTO`
/// ("data transfer object"), which the automatic camelCase rename would
/// spell `Dto`, so those fields are renamed by hand.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityDetail {
    activity_id: u64,
    activity_name: Option<String>,
    /// Free text the user wrote about the activity.
    description: Option<String>,
    location_name: Option<String>,
    #[serde(rename = "activityTypeDTO")]
    activity_type: Option<TypeKey>,
    /// Event classification, e.g. `race`, `training`, `uncategorized`.
    #[serde(rename = "eventTypeDTO")]
    event_type: Option<TypeKey>,
    #[serde(rename = "timeZoneUnitDTO")]
    time_zone: Option<TimeZone>,
    /// The measurements. `Option` because Garmin may leave it out.
    #[serde(rename = "summaryDTO")]
    summary: Option<DetailMeasurements>,
    /// Totals per split type. Empty for most activities.
    #[serde(deserialize_with = "null_as_default")]
    split_summaries: Vec<RawSplit>,
    /// `true` for a multisport activity that groups several legs.
    is_multi_sport_parent: Option<bool>,
    /// Upload metadata; only the multisport leg ids are useful.
    #[serde(rename = "metadataDTO")]
    metadata: Option<Metadata>,
}

/// The `metadataDTO` object. Mostly file and owner info; we keep only the
/// ids of a multisport activity's legs.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Metadata {
    #[serde(deserialize_with = "null_as_default")]
    child_ids: Vec<u64>,
}

/// A Garmin type object such as `{"typeId": 1, "typeKey": "running"}`; only
/// the key is kept.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct TypeKey {
    pub(super) type_key: Option<String>,
}

/// The `timeZoneUnitDTO` object, e.g. `{"timeZone": "Europe/Paris"}`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct TimeZone {
    time_zone: Option<String>,
}

/// The `unitOfPoolLength` object, e.g. `{"unitKey": "meter", ...}`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct PoolLengthUnit {
    pub(super) unit_key: Option<String>,
}

/// The `summaryDTO` object: all measurements, in Garmin units (meters,
/// seconds, m/s). A field is `None` when the sport or device doesn't record
/// it; some are `0.0` instead, which `into_summary` also treats as missing.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct DetailMeasurements {
    /// "YYYY-MM-DDTHH:MM:SS.f", local time.
    start_time_local: Option<String>,
    distance: Option<f64>,
    /// Timer time: pauses excluded.
    duration: Option<f64>,
    moving_duration: Option<f64>,
    /// Wall-clock time from start to end: pauses included.
    elapsed_duration: Option<f64>,
    average_speed: Option<f64>,
    average_moving_speed: Option<f64>,
    max_speed: Option<f64>,
    elevation_gain: Option<f64>,
    elevation_loss: Option<f64>,
    max_elevation: Option<f64>,
    min_elevation: Option<f64>,
    /// `-500.0` means "no elevation data" (indoor).
    avg_elevation: Option<f64>,
    /// Meters per second.
    avg_vertical_speed: Option<f64>,
    max_vertical_speed: Option<f64>,
    /// Speed adjusted for hills, m/s.
    avg_grade_adjusted_speed: Option<f64>,
    floors_climbed: Option<f64>,
    floors_descended: Option<f64>,
    steps: Option<u64>,
    #[serde(rename = "averageHR")]
    average_hr: Option<f64>,
    #[serde(rename = "maxHR")]
    max_hr: Option<f64>,
    #[serde(rename = "minHR")]
    min_hr: Option<f64>,
    /// Heart rate drop in the first minutes after the activity, bpm.
    recovery_heart_rate: Option<f64>,
    average_power: Option<f64>,
    max_power: Option<f64>,
    normalized_power: Option<f64>,
    /// Mechanical work done, kcal (= average power × time).
    total_work: Option<f64>,
    /// Steps per minute.
    average_run_cadence: Option<f64>,
    max_run_cadence: Option<f64>,
    /// Revolutions per minute.
    average_bike_cadence: Option<f64>,
    max_bike_cadence: Option<f64>,
    /// Strokes per minute.
    average_swim_cadence: Option<f64>,
    /// Milliseconds.
    ground_contact_time: Option<f64>,
    /// Percent on the left foot.
    ground_contact_balance_left: Option<f64>,
    /// Centimeters.
    stride_length: Option<f64>,
    /// Centimeters.
    vertical_oscillation: Option<f64>,
    /// Percent.
    vertical_ratio: Option<f64>,
    pool_length: Option<f64>,
    unit_of_pool_length: Option<PoolLengthUnit>,
    number_of_active_lengths: Option<u32>,
    total_number_of_strokes: Option<u32>,
    /// Strokes per pool length.
    average_strokes: Option<f64>,
    #[serde(rename = "averageSWOLF")]
    average_swolf: Option<f64>,
    /// Working sets only. (`totalSets` here also counts rest periods.)
    active_sets: Option<u32>,
    total_exercise_reps: Option<u32>,
    calories: Option<f64>,
    /// Resting (basal) part of `calories`, kcal.
    bmr_calories: Option<f64>,
    /// Load on the body, as an equivalent distance in meters.
    impact_load: Option<f64>,
    /// Estimated sweat loss, in milliliters.
    water_estimated: Option<f64>,
    /// Breaths per minute.
    avg_respiration_rate: Option<f64>,
    min_respiration_rate: Option<f64>,
    max_respiration_rate: Option<f64>,
    /// Aerobic training effect (the list endpoint calls it
    /// `aerobicTrainingEffect`).
    training_effect: Option<f64>,
    anaerobic_training_effect: Option<f64>,
    /// e.g. `MAINTAINING_AEROBIC_FITNESS_1`.
    aerobic_training_effect_message: Option<String>,
    anaerobic_training_effect_message: Option<String>,
    training_effect_label: Option<String>,
    activity_training_load: Option<f64>,
    moderate_intensity_minutes: Option<u32>,
    vigorous_intensity_minutes: Option<u32>,
    difference_body_battery: Option<i64>,
    /// Stamina at start and end, in percent.
    begin_potential_stamina: Option<f64>,
    end_potential_stamina: Option<f64>,
    /// Lowest stamina reached, in percent.
    min_available_stamina: Option<f64>,
}

/// One entry of `splitSummaries`: totals for all splits of one type.
///
/// Both the activity list and the details send this list, with slightly
/// different fields (heart rate only in details, step and ski data mostly in
/// the list), so every field is optional and both modules share this struct.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct RawSplit {
    /// e.g. `INTERVAL_ACTIVE`, `RWD_RUN`, `RWD_WALK`, `RWD_STAND`,
    /// `SKI_RUN_SPLIT`, `SKI_LIFT_SPLIT`.
    split_type: Option<String>,
    /// How many splits of this type were added up.
    no_of_splits: Option<u32>,
    /// Meters.
    distance: Option<f64>,
    /// Longest single split of this type, meters.
    max_distance: Option<f64>,
    /// Seconds.
    duration: Option<f64>,
    /// Meters per second.
    average_speed: Option<f64>,
    max_speed: Option<f64>,
    #[serde(rename = "averageHR")]
    average_hr: Option<f64>,
    #[serde(rename = "maxHR")]
    max_hr: Option<f64>,
    /// Seconds.
    moving_duration: Option<f64>,
    /// Meters per second.
    average_moving_speed: Option<f64>,
    avg_grade_adjusted_speed: Option<f64>,
    avg_vertical_speed: Option<f64>,
    /// Meters climbed: `elevationGain` in details, `totalAscent` in the list.
    elevation_gain: Option<f64>,
    total_ascent: Option<f64>,
    elevation_loss: Option<f64>,
    /// Biggest and average climb of a single split, meters.
    max_elevation_gain: Option<f64>,
    average_elevation_gain: Option<f64>,
    /// Hardest ski run, e.g. `GREEN`, `BLUE`, `RED`, `BLACK`.
    max_piste_difficulty: Option<String>,
    num_falls: Option<u32>,
    /// Kcal, total and resting part.
    calories: Option<f64>,
    bmr_calories: Option<f64>,
    /// Watts.
    average_power: Option<f64>,
    max_power: Option<f64>,
    normalized_power: Option<f64>,
    // Running dynamics come under two names: details use the same names as
    // the activity (`averageRunCadence`, `strideLength` in cm, ...), the list
    // uses `avgStepFrequency`, `avgStepLength` in m, ... Both are read, and
    // the output takes whichever is present.
    /// Steps per minute.
    average_run_cadence: Option<f64>,
    avg_step_frequency: Option<f64>,
    max_run_cadence: Option<f64>,
    /// Centimeters.
    stride_length: Option<f64>,
    /// Meters.
    avg_step_length: Option<f64>,
    /// Milliseconds.
    ground_contact_time: Option<f64>,
    avg_ground_contact_time: Option<f64>,
    /// Percent spent on the left foot.
    ground_contact_balance_left: Option<f64>,
    avg_ground_contact_balance_left: Option<f64>,
    /// Centimeters.
    vertical_oscillation: Option<f64>,
    /// Percent.
    vertical_ratio: Option<f64>,
}

/// One activity's full details, in the flat shape the tool returns.
///
/// Like `ActivitySummary`, each unit is in the field name and fields that
/// don't apply are left out (`skip_serializing_if`). Speed is given the usual
/// way for the sport: exactly one of each `..._per_km` / `..._per_100m` /
/// `..._kmh` group is filled.
#[derive(Debug, Serialize)]
pub struct ActivityDetailSummary {
    pub activity_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Sport, e.g. `treadmill_running`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_type: Option<String>,
    /// e.g. `race`, `training`; left out when `uncategorized`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    /// Local start time, "YYYY-MM-DD HH:MM:SS".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time_local: Option<String>,
    /// IANA time zone of the activity, e.g. `Europe/Paris`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_name: Option<String>,
    /// `true` for a multisport activity that groups several legs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multisport_parent: Option<bool>,
    /// Multisport: the legs' activity ids, for more `get_activity` calls.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub child_activity_ids: Vec<u64>,

    /// Timer time, pauses excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moving_duration_min: Option<f64>,
    /// Start to finish, pauses included. Only when it differs from
    /// `duration_min`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_duration_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_speed_kmh: Option<f64>,
    /// Average while moving (stops excluded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_moving_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_moving_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_moving_speed_kmh: Option<f64>,
    /// Fastest pace / top speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_speed_kmh: Option<f64>,
    /// Grade-adjusted pace: the flat-ground equivalent of a hilly run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_adjusted_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_adjusted_speed_kmh: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_elevation_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_elevation_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_elevation_m: Option<f64>,
    /// Climbing rate, meters per hour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_vertical_speed_m_per_h: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_vertical_speed_m_per_h: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floors_climbed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floors_descended: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u64>,

    /// Heart rate, bpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_hr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_hr: Option<f64>,
    /// Heart rate drop shortly after the activity, bpm. Higher is better.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_power_w: Option<f64>,
    /// Mechanical work done (average power × time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_work_kcal: Option<f64>,

    /// Running/walking cadence, steps per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_run_cadence_spm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_run_cadence_spm: Option<f64>,
    /// Cycling cadence, revolutions per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_bike_cadence_rpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bike_cadence_rpm: Option<f64>,
    /// Running dynamics (need a compatible watch or sensor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_contact_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stride_length_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_oscillation_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_ratio_percent: Option<f64>,
    /// Share of ground contact time on the left foot; 50 is even.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_contact_balance_left_percent: Option<f64>,
    /// Load on the body, as the equivalent distance of flat running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_load_m: Option<f64>,

    /// Pool swimming: length of the pool, in `pool_length_unit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_length: Option<f64>,
    /// e.g. `meter`, `yard`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_length_unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_lengths: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_strokes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_strokes_per_length: Option<f64>,
    /// Strokes + seconds per length; lower is more efficient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_swolf: Option<f64>,
    /// Swim cadence, strokes per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_swim_cadence_spm: Option<f64>,

    /// Strength training: working sets (rest periods not counted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_sets: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_reps: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_kcal: Option<f64>,
    /// The resting (basal) part of `calories_kcal`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_calories_kcal: Option<f64>,
    /// Estimated sweat loss.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweat_loss_ml: Option<f64>,
    /// Respiration, breaths per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_respiration_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_respiration_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_respiration_brpm: Option<f64>,

    /// Training effect, 0.0 to 5.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aerobic_training_effect: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anaerobic_training_effect: Option<f64>,
    /// Garmin's verdict, e.g. `IMPROVING_AEROBIC_BASE`, `NO_ANAEROBIC_BENEFIT`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aerobic_benefit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anaerobic_benefit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub training_effect_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_training_load: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderate_intensity_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vigorous_intensity_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difference_body_battery: Option<i64>,
    /// Stamina at start and end, percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina_start_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina_end_percent: Option<f64>,
    /// Lowest stamina during the activity, percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina_min_percent: Option<f64>,

    /// Totals per split type, e.g. run/walk/stand time of a treadmill
    /// session. Left out when Garmin has none.
    // `Vec::is_empty` is a method taking `&Vec`, so it fits
    // `skip_serializing_if` just like `Option::is_none`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<SplitSummary>,
}

/// Totals for all splits of one type, in output units.
#[derive(Debug, Serialize)]
pub struct SplitSummary {
    /// e.g. `RWD_RUN` (run/walk detection: running), `RWD_WALK`, `RWD_STAND`,
    /// `INTERVAL_ACTIVE`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_type: Option<String>,
    /// Number of splits of this type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_speed_kmh: Option<f64>,
    /// Fastest pace / top speed within these splits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_speed_kmh: Option<f64>,
    /// Average while moving (stops excluded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moving_duration_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_moving_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_moving_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_moving_speed_kmh: Option<f64>,
    /// Flat-ground equivalent pace of a hilly run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_adjusted_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_adjusted_speed_kmh: Option<f64>,
    /// Longest single split of this type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_km: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_hr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr: Option<f64>,
    /// Total climb and descent, e.g. ski lifts vs. ski runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_m: Option<f64>,
    /// Biggest and average climb of a single split.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_split_elevation_gain_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_split_elevation_gain_m: Option<f64>,
    /// Climbing rate, meters per hour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_vertical_speed_m_per_h: Option<f64>,
    /// Hardest ski run, e.g. `RED`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_piste_difficulty: Option<String>,
    /// Falls detected (e.g. skiing, snowboarding).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub falls: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_kcal: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_calories_kcal: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_power_w: Option<f64>,
    /// Running dynamics for these splits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_cadence_spm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cadence_spm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_stride_length_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_contact_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_contact_balance_left_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_oscillation_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_ratio_percent: Option<f64>,
}

impl ActivityDetail {
    /// Converts Garmin's details into the tool's [`ActivityDetailSummary`].
    ///
    /// Same rules as the activity list: durations in minutes (2 dp),
    /// distance in km (m for swimming), speed by sport, `0.0` treated as
    /// "not recorded", and rounding only where float32 noise adds digits
    /// that don't matter for health evaluation.
    pub fn into_summary(self) -> ActivityDetailSummary {
        // `unwrap_or_default()` replaces a missing `summaryDTO` with an
        // empty one (every field `None`), so the code below needs no checks.
        let m = self.summary.unwrap_or_default();
        // `Option<TypeKey>` → `Option<String>`: `and_then` continues only if
        // there is a value, and its closure may itself return `None`.
        let activity_type = self.activity_type.and_then(|t| t.type_key);
        // `as_deref()` borrows `Option<String>` as `Option<&str>`;
        // `unwrap_or("")` falls back to an empty name (→ km/h style).
        let style = SpeedStyle::for_activity(activity_type.as_deref().unwrap_or(""));

        let (distance_km, distance_m) = style.distance(positive(m.distance));
        let (avg_pace_per_km, avg_pace_per_100m, avg_speed_kmh) =
            style.convert(positive(m.average_speed));
        let (avg_moving_pace_per_km, avg_moving_pace_per_100m, avg_moving_speed_kmh) =
            style.convert(positive(m.average_moving_speed));
        let (best_pace_per_km, best_pace_per_100m, max_speed_kmh) =
            style.convert(positive(m.max_speed));
        let (grade_adjusted_pace_per_km, _, grade_adjusted_speed_kmh) =
            style.convert(positive(m.avg_grade_adjusted_speed));
        let duration_min = to_minutes(m.duration);
        // Elapsed time only adds something when the activity was paused;
        // otherwise it repeats `duration_min`.
        let elapsed_duration_min =
            to_minutes(m.elapsed_duration).filter(|e| Some(*e) != duration_min);
        // m/s to m/h, whole meters.
        let per_hour = |v: Option<f64>| positive(v).map(|v| round(v * 3600.0, 0));

        // Split totals use the same speed style as the whole activity.
        // `into_iter()` moves each split out of the list; `&style` lends
        // the style to every call without giving it away.
        let splits = self
            .split_summaries
            .into_iter()
            .map(|s| s.into_summary(&style))
            .collect();

        ActivityDetailSummary {
            activity_id: self.activity_id,
            activity_name: self.activity_name,
            // Garmin sends `""` when the user wrote no description.
            description: self.description.filter(|d| !d.trim().is_empty()),
            activity_type,
            event_type: event_type(self.event_type.and_then(|t| t.type_key)),
            start_time_local: m.start_time_local.map(local_time),
            time_zone: self.time_zone.and_then(|t| t.time_zone),
            location_name: self.location_name,
            multisport_parent: only_true(self.is_multi_sport_parent),
            child_activity_ids: self.metadata.map(|m| m.child_ids).unwrap_or_default(),
            duration_min,
            moving_duration_min: to_minutes(m.moving_duration),
            elapsed_duration_min,
            distance_km,
            distance_m,
            avg_pace_per_km,
            avg_pace_per_100m,
            avg_speed_kmh,
            avg_moving_pace_per_km,
            avg_moving_pace_per_100m,
            avg_moving_speed_kmh,
            best_pace_per_km,
            best_pace_per_100m,
            max_speed_kmh,
            grade_adjusted_pace_per_km,
            grade_adjusted_speed_kmh,
            elevation_gain_m: m.elevation_gain,
            elevation_loss_m: m.elevation_loss,
            // Decimeter precision is plenty for altitude.
            max_elevation_m: m.max_elevation.map(|v| round(v, 1)),
            min_elevation_m: m.min_elevation.map(|v| round(v, 1)),
            // `-500.0` is Garmin's "no elevation" marker for indoor activities.
            avg_elevation_m: m
                .avg_elevation
                .filter(|v| *v != -500.0)
                .map(|v| round(v, 1)),
            avg_vertical_speed_m_per_h: per_hour(m.avg_vertical_speed),
            max_vertical_speed_m_per_h: per_hour(m.max_vertical_speed),
            floors_climbed: positive(m.floors_climbed),
            floors_descended: positive(m.floors_descended),
            steps: m.steps,
            average_hr: m.average_hr,
            max_hr: m.max_hr,
            min_hr: m.min_hr,
            recovery_hr: m.recovery_heart_rate,
            avg_power_w: positive(m.average_power),
            max_power_w: positive(m.max_power),
            normalized_power_w: positive(m.normalized_power),
            total_work_kcal: rounded(m.total_work, 0),
            // Cadence and running dynamics: 1 decimal is finer than any
            // meaningful change.
            avg_run_cadence_spm: rounded(m.average_run_cadence, 1),
            max_run_cadence_spm: rounded(m.max_run_cadence, 1),
            avg_bike_cadence_rpm: rounded(m.average_bike_cadence, 1),
            max_bike_cadence_rpm: rounded(m.max_bike_cadence, 1),
            ground_contact_time_ms: rounded(m.ground_contact_time, 1),
            stride_length_cm: rounded(m.stride_length, 1),
            vertical_oscillation_cm: rounded(m.vertical_oscillation, 1),
            vertical_ratio_percent: rounded(m.vertical_ratio, 1),
            ground_contact_balance_left_percent: rounded(m.ground_contact_balance_left, 1),
            impact_load_m: rounded(m.impact_load, 0),
            pool_length: positive(m.pool_length),
            pool_length_unit: m.unit_of_pool_length.and_then(|u| u.unit_key),
            active_lengths: m.number_of_active_lengths,
            total_strokes: m.total_number_of_strokes,
            avg_strokes_per_length: rounded(m.average_strokes, 1),
            avg_swolf: positive(m.average_swolf),
            avg_swim_cadence_spm: rounded(m.average_swim_cadence, 1),
            total_sets: m.active_sets,
            total_reps: m.total_exercise_reps,
            calories_kcal: m.calories,
            resting_calories_kcal: positive(m.bmr_calories),
            sweat_loss_ml: positive(m.water_estimated),
            avg_respiration_brpm: rounded(m.avg_respiration_rate, 1),
            min_respiration_brpm: rounded(m.min_respiration_rate, 1),
            max_respiration_brpm: rounded(m.max_respiration_rate, 1),
            // Training effect uses 0.1 steps, so 1 decimal loses nothing.
            aerobic_training_effect: m.training_effect.map(|v| round(v, 1)),
            anaerobic_training_effect: m.anaerobic_training_effect.map(|v| round(v, 1)),
            aerobic_benefit: m.aerobic_training_effect_message.map(benefit),
            anaerobic_benefit: m.anaerobic_training_effect_message.map(benefit),
            training_effect_label: m.training_effect_label,
            activity_training_load: m.activity_training_load.map(|v| round(v, 1)),
            moderate_intensity_minutes: m.moderate_intensity_minutes,
            vigorous_intensity_minutes: m.vigorous_intensity_minutes,
            difference_body_battery: m.difference_body_battery,
            stamina_start_percent: m.begin_potential_stamina,
            stamina_end_percent: m.end_potential_stamina,
            stamina_min_percent: m.min_available_stamina,
            splits,
        }
    }
}

impl RawSplit {
    /// Converts one split total, using the activity's speed style.
    /// `style: &SpeedStyle` borrows it, since every split uses the same one.
    /// `pub(super)` so `activity.rs` can convert the list's splits too.
    pub(super) fn into_summary(self, style: &SpeedStyle) -> SplitSummary {
        let (distance_km, distance_m) = style.distance(positive(self.distance));
        let (longest_km, longest_m) = style.distance(positive(self.max_distance));
        let (avg_pace_per_km, avg_pace_per_100m, avg_speed_kmh) =
            style.convert(positive(self.average_speed));
        let (best_pace_per_km, best_pace_per_100m, max_speed_kmh) =
            style.convert(positive(self.max_speed));
        let (avg_moving_pace_per_km, avg_moving_pace_per_100m, avg_moving_speed_kmh) =
            style.convert(positive(self.average_moving_speed));
        let (grade_adjusted_pace_per_km, _, grade_adjusted_speed_kmh) =
            style.convert(positive(self.avg_grade_adjusted_speed));
        // Stride length: centimeters in details, meters in the list.
        // `.or(other)` keeps the first value if there is one, else `other`.
        let stride_length_cm = positive(self.stride_length)
            .or(positive(self.avg_step_length).map(|m| m * 100.0))
            .map(|cm| round(cm, 1));
        SplitSummary {
            split_type: self.split_type,
            count: self.no_of_splits,
            duration_min: to_minutes(self.duration),
            distance_km,
            distance_m,
            avg_pace_per_km,
            avg_pace_per_100m,
            avg_speed_kmh,
            best_pace_per_km,
            best_pace_per_100m,
            max_speed_kmh,
            moving_duration_min: to_minutes(self.moving_duration),
            avg_moving_pace_per_km,
            avg_moving_pace_per_100m,
            avg_moving_speed_kmh,
            grade_adjusted_pace_per_km,
            grade_adjusted_speed_kmh,
            longest_km,
            longest_m,
            average_hr: positive(self.average_hr),
            max_hr: positive(self.max_hr),
            elevation_gain_m: positive(self.elevation_gain).or(positive(self.total_ascent)),
            elevation_loss_m: positive(self.elevation_loss),
            max_split_elevation_gain_m: positive(self.max_elevation_gain),
            avg_split_elevation_gain_m: positive(self.average_elevation_gain),
            // m/s to m/h.
            avg_vertical_speed_m_per_h: positive(self.avg_vertical_speed)
                .map(|v| round(v * 3600.0, 0)),
            max_piste_difficulty: self.max_piste_difficulty,
            falls: count(self.num_falls),
            calories_kcal: positive(self.calories),
            resting_calories_kcal: positive(self.bmr_calories),
            avg_power_w: positive(self.average_power),
            max_power_w: positive(self.max_power),
            normalized_power_w: positive(self.normalized_power),
            avg_cadence_spm: rounded(self.average_run_cadence.or(self.avg_step_frequency), 1),
            max_cadence_spm: rounded(self.max_run_cadence, 1),
            avg_stride_length_cm: stride_length_cm,
            ground_contact_time_ms: rounded(
                self.ground_contact_time.or(self.avg_ground_contact_time),
                1,
            ),
            ground_contact_balance_left_percent: rounded(
                self.ground_contact_balance_left
                    .or(self.avg_ground_contact_balance_left),
                1,
            ),
            vertical_oscillation_cm: rounded(self.vertical_oscillation, 1),
            vertical_ratio_percent: rounded(self.vertical_ratio, 1),
        }
    }
}

/// Rewrites "2026-10-20T18:39:38.0" as "2026-10-20 18:39:38", the format
/// the activity list uses. Unexpected input is returned unchanged.
fn local_time(text: String) -> String {
    // `%.f` matches an optional fraction of a second, like ".0".
    match NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S%.f") {
        Ok(time) => time.format("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a JSON value as Garmin's response and converts it.
    fn summary(json: serde_json::Value) -> ActivityDetailSummary {
        serde_json::from_value::<ActivityDetail>(json)
            .unwrap()
            .into_summary()
    }

    #[test]
    fn treadmill_run_details() {
        // Trimmed from a real response.
        let s = summary(serde_json::json!({
            "activityId": 24793720591u64,
            "activityName": "Treadmill",
            "description": "",
            "activityTypeDTO": {"typeId": 18, "typeKey": "treadmill_running", "parentTypeId": 1},
            "eventTypeDTO": {"typeId": 9, "typeKey": "uncategorized"},
            "timeZoneUnitDTO": {"timeZone": "Europe/Paris"},
            "summaryDTO": {
                "startTimeLocal": "2026-10-20T18:39:38.0",
                "distance": 2800.0, "duration": 2703.101, "movingDuration": 2428.683,
                "elapsedDuration": 2703.101, "avgElevation": -500.0,
                "averageSpeed": 1.0360000133514404, "averageMovingSpeed": 1.1528882193105259,
                "maxSpeed": 1.4739999771118164, "averageHR": 114.0, "minHR": 62.0,
                "averageRunCadence": 87.78125, "averagePower": 78.0, "minPower": 0.0,
                "strideLength": 68.10999755859375, "trainingEffect": 1.600000023841858,
                "aerobicTrainingEffectMessage": "MINOR_AEROBIC_BENEFIT_6",
                "anaerobicTrainingEffectMessage": "NO_ANAEROBIC_BENEFIT_0",
                "waterEstimated": 282.0
            },
            "splitSummaries": [
                {"splitType": "RWD_RUN", "noOfSplits": 3, "distance": 40.46, "duration": 42.0,
                 "averageSpeed": 0.9629999995231628, "averageHR": 90.0},
                {"splitType": "RWD_STAND", "noOfSplits": 4, "distance": 0.0, "duration": 212.999,
                 "averageSpeed": 0.0, "averageHR": 106.0}
            ]
        }));
        assert_eq!(s.description, None);
        assert_eq!(s.activity_type.as_deref(), Some("treadmill_running"));
        // `uncategorized` carries no information, so it is left out.
        assert_eq!(s.event_type, None);
        assert_eq!(s.time_zone.as_deref(), Some("Europe/Paris"));
        assert_eq!(s.start_time_local.as_deref(), Some("2026-10-20 18:39:38"));
        assert_eq!(s.distance_km, Some(2.8));
        assert_eq!(s.duration_min, Some(45.05));
        assert_eq!(s.moving_duration_min, Some(40.48));
        assert_eq!(s.avg_pace_per_km.as_deref(), Some("16:05"));
        assert_eq!(s.avg_moving_pace_per_km.as_deref(), Some("14:27"));
        assert_eq!(s.best_pace_per_km.as_deref(), Some("11:18"));
        assert_eq!(s.avg_speed_kmh, None);
        assert_eq!(s.min_hr, Some(62.0));
        assert_eq!(s.avg_run_cadence_spm, Some(87.8));
        assert_eq!(s.stride_length_cm, Some(68.1));
        assert_eq!(s.aerobic_training_effect, Some(1.6));
        assert_eq!(s.aerobic_benefit.as_deref(), Some("MINOR_AEROBIC_BENEFIT"));
        assert_eq!(s.anaerobic_benefit.as_deref(), Some("NO_ANAEROBIC_BENEFIT"));
        assert_eq!(s.sweat_loss_ml, Some(282.0));
        // Indoors: no elevation fields at all (the -500 sentinel is ignored).
        assert_eq!(s.max_elevation_m, None);

        assert_eq!(s.splits.len(), 2);
        assert_eq!(s.splits[0].split_type.as_deref(), Some("RWD_RUN"));
        assert_eq!(s.splits[0].count, Some(3));
        assert_eq!(s.splits[0].distance_km, Some(0.04));
        assert_eq!(s.splits[0].duration_min, Some(0.7));
        // Standing still: no distance or pace, but time and heart rate.
        assert_eq!(s.splits[1].distance_km, None);
        assert_eq!(s.splits[1].avg_pace_per_km, None);
        assert_eq!(s.splits[1].duration_min, Some(3.55));
        assert_eq!(s.splits[1].average_hr, Some(106.0));
    }

    #[test]
    fn pool_swim_details() {
        let s = summary(serde_json::json!({
            "activityId": 1,
            "activityTypeDTO": {"typeKey": "lap_swimming"},
            "summaryDTO": {
                "distance": 1500.0, "averageSpeed": 0.7239999771118164,
                "maxSpeed": 0.9070000052452087, "averageMovingSpeed": 0.0,
                "poolLength": 25.0, "unitOfPoolLength": {"unitId": 1, "unitKey": "meter", "factor": 100.0},
                "numberOfActiveLengths": 60, "totalNumberOfStrokes": 509,
                "averageStrokes": 12.100000381469727, "averageSWOLF": 47.0,
                "averageSwimCadence": 21.0
            }
        }));
        assert_eq!(s.distance_m, Some(1500.0));
        assert_eq!(s.distance_km, None);
        assert_eq!(s.avg_pace_per_100m.as_deref(), Some("2:18"));
        assert_eq!(s.best_pace_per_100m.as_deref(), Some("1:50"));
        // `averageMovingSpeed: 0.0` means "not recorded".
        assert_eq!(s.avg_moving_pace_per_100m, None);
        assert_eq!(s.pool_length, Some(25.0));
        assert_eq!(s.pool_length_unit.as_deref(), Some("meter"));
        assert_eq!(s.active_lengths, Some(60));
        assert_eq!(s.avg_strokes_per_length, Some(12.1));
        assert_eq!(s.avg_swolf, Some(47.0));
    }

    #[test]
    fn strength_counts_working_sets_only() {
        let s = summary(serde_json::json!({
            "activityId": 1,
            "activityTypeDTO": {"typeKey": "strength_training"},
            "summaryDTO": {"distance": 0.0, "averageSpeed": 0.0, "totalSets": 48,
                           "activeSets": 24, "totalExerciseReps": 285}
        }));
        assert_eq!(s.total_sets, Some(24));
        assert_eq!(s.total_reps, Some(285));
        assert_eq!(s.distance_km, None);
        assert_eq!(s.avg_speed_kmh, None);
    }

    #[test]
    fn output_skips_missing_fields() {
        let json = serde_json::to_string(&summary(serde_json::json!({"activityId": 7}))).unwrap();
        assert_eq!(json, r#"{"activity_id":7}"#);
    }
}
