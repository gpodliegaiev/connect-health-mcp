//! Types for the activity list
//! (`GET /activitylist-service/activities/search/activities`).
//!
//! Same two-step pattern as the other responses: [`Activity`] mirrors one
//! entry of Garmin's list (Garmin units), and [`Activity::into_summary`]
//! turns it into the [`ActivitySummary`] the tool returns, with the unit in
//! every field name.
//!
//! The list entry is Garmin's richest activity record (~230 possible fields),
//! and every measurement in it is kept so an AI can evaluate an activity
//! fully: heart-rate and power zones, power curve, running dynamics, swim,
//! dive, cycling and strength metrics, fastest splits, stress, and more.
//! Most fields exist only for some sports or devices; anything Garmin leaves
//! out, sends as `null`, or sends as a placeholder `0` is left out of the
//! output.
//!
//! Left out on purpose (not about the workout): owner name, id and profile
//! images, privacy, likes and comments, photo/video flags, GPS coordinates,
//! device manufacturer, internal ids and UUIDs, duplicate flags (`pr` next to
//! `isPR`), and fields that repeat others (`startTimeGMT`, `beginTimestamp`,
//! `maxDoubleCadence`).

use serde::{Deserialize, Deserializer, Serialize};

use super::activity_detail::{PoolLengthUnit, RawSplit, SplitSummary, TypeKey};
use super::types::{
    SpeedStyle, benefit, count, event_type, format_time, null_as_default, only_true, positive,
    round, rounded, to_minutes,
};

/// One activity (run, walk, strength session, ...) as Garmin sends it.
///
/// Only used for reading (`Deserialize`). It is converted into the
/// [`ActivitySummary`] the tool returns, which uses friendlier units.
///
/// `rename_all = "camelCase"` matches Garmin's field names
/// (`startTimeLocal` <-> `start_time_local`). Names that the automatic
/// rename can't produce (`averageHR`, `hrTimeInZone_1`, ...) get an explicit
/// `rename`. Fields wrapped in `Option<T>` may be missing: most only exist
/// for some sports. `#[serde(default)]` on the struct means "a missing field
/// becomes its default" (`None` for an `Option`, an empty list for a `Vec`).
///
/// Units are Garmin's and noted on each field when not obvious.
// `Default` is derived so serde's `default` attribute (above) can fill in
// missing fields: `0` for numbers, `""` for strings, `None` for options.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Activity {
    // --- What, when, where ---
    activity_id: u64,
    activity_name: Option<String>,
    /// Free text the user wrote about the activity.
    description: Option<String>,
    // Garmin sends `"activityType": {"typeId": 1, "typeKey": "running", ...}`.
    // `deserialize_with` names a function (below) that pulls out just the
    // `typeKey` string.
    #[serde(deserialize_with = "type_key")]
    activity_type: String,
    /// e.g. `race`, `training`, `uncategorized`.
    event_type: Option<TypeKey>,
    start_time_local: String,
    location_name: Option<String>,
    /// Multisport: `true` on the parent activity that groups the legs.
    is_parent: Option<bool>,
    /// Multisport: the parent activity's id, on each leg.
    parent_id: Option<u64>,
    /// Personal record set in this activity.
    #[serde(rename = "isPR")]
    is_pr: Option<bool>,
    /// Entered by hand instead of recorded by a device.
    is_manual_activity: Option<bool>,
    lap_count: Option<u32>,

    // --- Time, distance, speed (s, m, m/s) ---
    /// Timer time: pauses excluded.
    duration: Option<f64>,
    /// Wall-clock time from start to end: pauses included.
    elapsed_duration: Option<f64>,
    moving_duration: Option<f64>,
    distance: Option<f64>,
    average_speed: Option<f64>,
    max_speed: Option<f64>,
    /// Speed adjusted for hills (running).
    avg_grade_adjusted_speed: Option<f64>,

    // --- Elevation (m, m/s) ---
    elevation_gain: Option<f64>,
    elevation_loss: Option<f64>,
    min_elevation: Option<f64>,
    max_elevation: Option<f64>,
    /// `-500.0` means "no elevation data" (indoor).
    avg_elevation: Option<f64>,
    avg_vertical_speed: Option<f64>,
    max_vertical_speed: Option<f64>,
    floors_climbed: Option<f64>,
    floors_descended: Option<f64>,
    steps: Option<u64>,

    // --- Heart rate (bpm, s) ---
    #[serde(rename = "averageHR")]
    average_hr: Option<f64>,
    #[serde(rename = "maxHR")]
    max_hr: Option<f64>,
    /// Seconds in each heart-rate zone. Time below zone 1 is in none.
    #[serde(rename = "hrTimeInZone_1")]
    hr_zone_1: Option<f64>,
    #[serde(rename = "hrTimeInZone_2")]
    hr_zone_2: Option<f64>,
    #[serde(rename = "hrTimeInZone_3")]
    hr_zone_3: Option<f64>,
    #[serde(rename = "hrTimeInZone_4")]
    hr_zone_4: Option<f64>,
    #[serde(rename = "hrTimeInZone_5")]
    hr_zone_5: Option<f64>,
    lactate_threshold_bpm: Option<f64>,
    /// Centimeters per millisecond (= 10 m/s). Garmin's field name really
    /// starts with `get`.
    #[serde(rename = "getlactateThresholdSpeed")]
    lactate_threshold_speed: Option<f64>,

    // --- Power (W, s) ---
    avg_power: Option<f64>,
    max_power: Option<f64>,
    norm_power: Option<f64>,
    max20_min_power: Option<f64>,
    max_ftp: Option<f64>,
    #[serde(rename = "powerTimeInZone_1")]
    power_zone_1: Option<f64>,
    #[serde(rename = "powerTimeInZone_2")]
    power_zone_2: Option<f64>,
    #[serde(rename = "powerTimeInZone_3")]
    power_zone_3: Option<f64>,
    #[serde(rename = "powerTimeInZone_4")]
    power_zone_4: Option<f64>,
    #[serde(rename = "powerTimeInZone_5")]
    power_zone_5: Option<f64>,
    #[serde(rename = "powerTimeInZone_6")]
    power_zone_6: Option<f64>,
    #[serde(rename = "powerTimeInZone_7")]
    power_zone_7: Option<f64>,
    /// Best average power over 1 s, 2 s, ... 5 h. `flatten` reads these
    /// 15 fields from the same JSON object into a separate struct.
    #[serde(flatten)]
    power_curve: PowerCurve,
    training_stress_score: Option<f64>,
    /// Fraction, e.g. 0.8 = 80 % of FTP.
    intensity_factor: Option<f64>,
    /// Pedal balance, percent.
    left_balance: Option<f64>,
    right_balance: Option<f64>,
    avg_left_balance: Option<f64>,
    /// Pedal force, newtons.
    avg_force: Option<f64>,
    max_force: Option<f64>,
    normalized_force: Option<f64>,
    /// Cycling position: seconds, and watts.
    standing_time: Option<f64>,
    seated_time: Option<f64>,
    avg_standing_power: Option<f64>,
    max_standing_power: Option<f64>,
    avg_seated_power: Option<f64>,
    max_seated_power: Option<f64>,

    // --- Cadence (per minute) ---
    average_running_cadence_in_steps_per_minute: Option<f64>,
    max_running_cadence_in_steps_per_minute: Option<f64>,
    max_lap_avg_run_cadence: Option<f64>,
    average_biking_cadence_in_rev_per_minute: Option<f64>,
    max_biking_cadence_in_rev_per_minute: Option<f64>,
    average_swim_cadence_in_strokes_per_minute: Option<f64>,
    max_swim_cadence_in_strokes_per_minute: Option<f64>,
    avg_wheelchair_cadence: Option<f64>,
    max_wheelchair_cadence: Option<f64>,
    avg_jump_rope_cadence: Option<f64>,
    max_jump_rope_cadence: Option<f64>,
    jump_count: Option<u32>,

    // --- Running dynamics ---
    /// Centimeters.
    avg_stride_length: Option<f64>,
    /// Centimeters.
    avg_vertical_oscillation: Option<f64>,
    /// Milliseconds.
    avg_ground_contact_time: Option<f64>,
    /// Percent.
    avg_vertical_ratio: Option<f64>,
    /// Percent on the left foot.
    avg_ground_contact_balance: Option<f64>,
    /// Centimeters, and percent.
    avg_step_speed_loss: Option<f64>,
    avg_step_speed_loss_percent: Option<f64>,
    /// Load on the body, as an equivalent distance in meters.
    impact_load: Option<f64>,

    // --- Swimming and paddling ---
    /// Centimeters, whatever `unitOfPoolLength` says.
    pool_length: Option<f64>,
    unit_of_pool_length: Option<PoolLengthUnit>,
    active_lengths: Option<u32>,
    strokes: Option<f64>,
    /// Strokes per length.
    avg_strokes: Option<f64>,
    min_strokes: Option<f64>,
    average_swolf: Option<f64>,
    /// Centimeters.
    avg_stroke_distance: Option<f64>,
    /// Strokes per minute (paddling, rowing).
    avg_stroke_cadence: Option<f64>,
    max_stroke_cadence: Option<f64>,
    /// Pool apnea, seconds.
    longest_pool_apnea_interval: Option<f64>,
    pool_apnea_breath_hold_duration: Option<f64>,

    // --- Strength training ---
    /// Working sets. (Here equal to `activeSets`.)
    total_sets: Option<u32>,
    active_sets: Option<u32>,
    total_reps: Option<u32>,
    /// Totals per exercise.
    #[serde(deserialize_with = "null_as_default")]
    summarized_exercise_sets: Vec<RawExerciseSet>,

    // --- Energy and hydration ---
    /// Active calories, kcal.
    calories: Option<f64>,
    /// Resting (basal) calories during the activity, kcal.
    bmr_calories: Option<f64>,
    /// Kilojoules.
    calories_estimated: Option<f64>,
    calories_consumed: Option<f64>,
    /// Estimated sweat loss, milliliters.
    water_estimated: Option<f64>,
    water_consumed: Option<f64>,
    /// Extra kcal from carrying a pack.
    pack_weight_calories: Option<u32>,
    /// Grams.
    begin_pack_weight: Option<f64>,

    // --- Respiration (breaths per minute) ---
    avg_respiration_rate: Option<f64>,
    min_respiration_rate: Option<f64>,
    max_respiration_rate: Option<f64>,

    // --- Training effect and load ---
    aerobic_training_effect: Option<f64>,
    anaerobic_training_effect: Option<f64>,
    /// e.g. `IMPROVING_AEROBIC_BASE_8`.
    aerobic_training_effect_message: Option<String>,
    anaerobic_training_effect_message: Option<String>,
    training_effect_label: Option<String>,
    activity_training_load: Option<f64>,
    #[serde(rename = "vO2MaxValue")]
    vo2_max_value: Option<f64>,
    moderate_intensity_minutes: Option<u32>,
    vigorous_intensity_minutes: Option<u32>,
    difference_body_battery: Option<i64>,

    // --- Stress (0–100) ---
    avg_stress: Option<f64>,
    start_stress: Option<f64>,
    end_stress: Option<f64>,
    max_stress: Option<f64>,
    difference_stress: Option<f64>,

    // --- Temperature (°C) ---
    min_temperature: Option<f64>,
    max_temperature: Option<f64>,

    // --- Fastest efforts (s) ---
    #[serde(flatten)]
    fastest: RawFastestSplits,

    // --- Diving (cm, ms, s) ---
    max_depth: Option<f64>,
    avg_depth: Option<f64>,
    surface_interval: Option<f64>,
    bottom_time: Option<f64>,
    max_bottom_time: Option<f64>,
    start_n2: Option<f64>,
    end_n2: Option<f64>,
    start_cns: Option<f64>,
    end_cns: Option<f64>,
    is_deco_dive: Option<bool>,
    dive_count: Option<u32>,
    dive_number: Option<u32>,

    // --- Mountain biking (Garmin's trail scores) ---
    flow: Option<f64>,
    grit: Option<f64>,
    avg_flow: Option<f64>,
    avg_grit: Option<f64>,

    // --- Video games / esports ---
    game_name: Option<String>,
    game_type: Option<String>,

    // --- Other ---
    /// Grams of CO2 saved (e.g. by cycling instead of driving).
    carbon_offset: Option<f64>,
    /// Totals per split type (e.g. run/walk/stand, ski runs vs. lifts).
    #[serde(deserialize_with = "null_as_default")]
    split_summaries: Vec<RawSplit>,
}

/// Best average power over fixed time windows (watts), aka the power curve.
///
/// This one struct both reads Garmin's names and writes the output names:
/// `rename(deserialize = ..., serialize = ...)` sets each side separately,
/// so `maxAvgPower_60` is read and written as `"60s"`. No unit conversion is
/// needed, only dropping zeros.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PowerCurve {
    #[serde(
        rename(deserialize = "maxAvgPower_1", serialize = "1s"),
        skip_serializing_if = "is_zero"
    )]
    s1: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_2", serialize = "2s"),
        skip_serializing_if = "is_zero"
    )]
    s2: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_5", serialize = "5s"),
        skip_serializing_if = "is_zero"
    )]
    s5: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_10", serialize = "10s"),
        skip_serializing_if = "is_zero"
    )]
    s10: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_20", serialize = "20s"),
        skip_serializing_if = "is_zero"
    )]
    s20: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_30", serialize = "30s"),
        skip_serializing_if = "is_zero"
    )]
    s30: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_60", serialize = "1min"),
        skip_serializing_if = "is_zero"
    )]
    s60: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_120", serialize = "2min"),
        skip_serializing_if = "is_zero"
    )]
    s120: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_300", serialize = "5min"),
        skip_serializing_if = "is_zero"
    )]
    s300: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_600", serialize = "10min"),
        skip_serializing_if = "is_zero"
    )]
    s600: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_1200", serialize = "20min"),
        skip_serializing_if = "is_zero"
    )]
    s1200: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_1800", serialize = "30min"),
        skip_serializing_if = "is_zero"
    )]
    s1800: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_3600", serialize = "1h"),
        skip_serializing_if = "is_zero"
    )]
    s3600: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_7200", serialize = "2h"),
        skip_serializing_if = "is_zero"
    )]
    s7200: Option<u32>,
    #[serde(
        rename(deserialize = "maxAvgPower_18000", serialize = "5h"),
        skip_serializing_if = "is_zero"
    )]
    s18000: Option<u32>,
}

impl PowerCurve {
    /// `true` if no window has a value, so the whole object can be left out.
    fn is_empty(&self) -> bool {
        // An array of references (`&Option<u32>`) lets us check all fields
        // in one loop; `.iter().all(...)` is `true` if every item passes.
        [
            &self.s1,
            &self.s2,
            &self.s5,
            &self.s10,
            &self.s20,
            &self.s30,
            &self.s60,
            &self.s120,
            &self.s300,
            &self.s600,
            &self.s1200,
            &self.s1800,
            &self.s3600,
            &self.s7200,
            &self.s18000,
        ]
        .iter()
        .all(|v| is_zero(v))
    }
}

/// `skip_serializing_if` helper: missing or `0` counts as "no value".
/// serde passes a reference to the field, hence `&Option<u32>`.
fn is_zero(value: &Option<u32>) -> bool {
    value.unwrap_or(0) == 0
}

/// Garmin's fastest-effort times, in seconds, e.g. `fastestSplit_1000` for
/// the fastest kilometer. The names after `_` are distances in meters
/// (`1609` = mile, `91` = 100 yards).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawFastestSplits {
    #[serde(rename = "fastestSplit_100")]
    m100: Option<f64>,
    #[serde(rename = "fastestSplit_400")]
    m400: Option<f64>,
    #[serde(rename = "fastestSplit_750")]
    m750: Option<f64>,
    #[serde(rename = "fastestSplit_1000")]
    m1000: Option<f64>,
    #[serde(rename = "fastestSplit_1500")]
    m1500: Option<f64>,
    #[serde(rename = "fastestSplit_1609")]
    mile: Option<f64>,
    #[serde(rename = "fastestSplit_5000")]
    m5000: Option<f64>,
    #[serde(rename = "fastestSplit_10000")]
    m10000: Option<f64>,
    #[serde(rename = "fastestSplit_21098")]
    half_marathon: Option<f64>,
    #[serde(rename = "fastestSplit_40000")]
    m40000: Option<f64>,
    #[serde(rename = "fastestSplit_42195")]
    marathon: Option<f64>,
    #[serde(rename = "fastestSplit_91")]
    yd100: Option<f64>,
    #[serde(rename = "fastestSplit_457")]
    yd500: Option<f64>,
    #[serde(rename = "fastestSplit_914")]
    yd1000: Option<f64>,
    #[serde(rename = "fastestSplit_1508")]
    yd1650: Option<f64>,
}

/// Fastest time over each distance covered, as "m:ss" or "h:mm:ss".
/// Each field's output name is the distance.
#[derive(Debug, Serialize)]
pub struct FastestSplits {
    #[serde(rename = "100m", skip_serializing_if = "Option::is_none")]
    m100: Option<String>,
    #[serde(rename = "400m", skip_serializing_if = "Option::is_none")]
    m400: Option<String>,
    #[serde(rename = "750m", skip_serializing_if = "Option::is_none")]
    m750: Option<String>,
    #[serde(rename = "1km", skip_serializing_if = "Option::is_none")]
    m1000: Option<String>,
    #[serde(rename = "1500m", skip_serializing_if = "Option::is_none")]
    m1500: Option<String>,
    #[serde(rename = "1mile", skip_serializing_if = "Option::is_none")]
    mile: Option<String>,
    #[serde(rename = "5km", skip_serializing_if = "Option::is_none")]
    m5000: Option<String>,
    #[serde(rename = "10km", skip_serializing_if = "Option::is_none")]
    m10000: Option<String>,
    #[serde(rename = "half_marathon", skip_serializing_if = "Option::is_none")]
    half_marathon: Option<String>,
    #[serde(rename = "40km", skip_serializing_if = "Option::is_none")]
    m40000: Option<String>,
    #[serde(rename = "marathon", skip_serializing_if = "Option::is_none")]
    marathon: Option<String>,
    #[serde(rename = "100yd", skip_serializing_if = "Option::is_none")]
    yd100: Option<String>,
    #[serde(rename = "500yd", skip_serializing_if = "Option::is_none")]
    yd500: Option<String>,
    #[serde(rename = "1000yd", skip_serializing_if = "Option::is_none")]
    yd1000: Option<String>,
    #[serde(rename = "1650yd", skip_serializing_if = "Option::is_none")]
    yd1650: Option<String>,
}

impl RawFastestSplits {
    /// Formats every time, or returns `None` if there are none at all.
    fn into_summary(self) -> Option<FastestSplits> {
        // A closure (an inline function) stored in a variable: seconds →
        // formatted time, skipping missing or zero values.
        let time = |seconds: Option<f64>| positive(seconds).map(format_time);
        let out = FastestSplits {
            m100: time(self.m100),
            m400: time(self.m400),
            m750: time(self.m750),
            m1000: time(self.m1000),
            m1500: time(self.m1500),
            mile: time(self.mile),
            m5000: time(self.m5000),
            m10000: time(self.m10000),
            half_marathon: time(self.half_marathon),
            m40000: time(self.m40000),
            marathon: time(self.marathon),
            yd100: time(self.yd100),
            yd500: time(self.yd500),
            yd1000: time(self.yd1000),
            yd1650: time(self.yd1650),
        };
        // Compare against the empty JSON object: simpler than checking all
        // 15 fields. `.ok()?` gives up (returns `None`) if serializing fails.
        let empty = serde_json::to_value(&out).ok()?.as_object()?.is_empty();
        // `bool::then` turns `true` into `Some(out)` and `false` into `None`.
        (!empty).then_some(out)
    }
}

/// Minutes spent in each zone. Missing zones are left out; a zone with
/// `0.0` is kept when another zone has time, since "no time in zone 5" is
/// information too.
#[derive(Debug, Serialize)]
pub struct Zones {
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_1: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_3: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_4: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_5: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_6: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone_7: Option<f64>,
}

/// Builds [`Zones`] from up to 7 times in seconds, or `None` if every zone
/// is missing or `0` (e.g. power zones on a treadmill without a power
/// meter).
///
/// `[Option<f64>; 7]` is an *array*: a fixed-size list (here always 7 items).
fn zones(seconds: [Option<f64>; 7]) -> Option<Zones> {
    if seconds.iter().all(|s| positive(*s).is_none()) {
        return None;
    }
    // Seconds to minutes; 1 decimal (6 s) is plenty for time in zone.
    // `let [a, b, ...] = array.map(f)` applies `f` to each item, then
    // destructures the array into one variable per item.
    let [zone_1, zone_2, zone_3, zone_4, zone_5, zone_6, zone_7] =
        seconds.map(|s| s.map(|v| round(v / 60.0, 1)));
    Some(Zones {
        zone_1,
        zone_2,
        zone_3,
        zone_4,
        zone_5,
        zone_6,
        zone_7,
    })
}

/// One entry of `summarizedExerciseSets`: totals for one exercise in a
/// strength session.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawExerciseSet {
    /// e.g. `BENCH_PRESS`, `LATERAL_RAISE`.
    category: Option<String>,
    /// e.g. `INCLINE_DUMBBELL_BENCH_PRESS`; missing if not chosen.
    sub_category: Option<String>,
    sets: Option<u32>,
    reps: Option<u32>,
    /// Milliseconds.
    duration: Option<f64>,
    /// Grams.
    max_weight: Option<f64>,
    /// Sum of weight × reps, grams.
    volume: Option<f64>,
    /// Watts.
    peak_power: Option<f64>,
    /// Centimeters per millisecond (= 10 m/s).
    concentric_mean_velocity: Option<f64>,
}

/// Totals for one exercise, in output units.
#[derive(Debug, Serialize)]
pub struct ExerciseSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exercise: Option<String>,
    /// More specific variant of `exercise`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sets: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reps: Option<u32>,
    /// Working time for this exercise (rests excluded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<f64>,
    /// Heaviest weight used. Left out for body-weight exercises.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_weight_kg: Option<f64>,
    /// Total weight × reps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concentric_mean_velocity_m_s: Option<f64>,
}

impl RawExerciseSet {
    fn into_summary(self) -> ExerciseSet {
        ExerciseSet {
            exercise: self.category,
            variant: self.sub_category,
            sets: count(self.sets),
            reps: count(self.reps),
            // Milliseconds to minutes.
            duration_min: positive(self.duration).map(|ms| round(ms / 60_000.0, 2)),
            // Grams to kilograms; 2 decimals keeps 1.25 kg plates exact.
            max_weight_kg: positive(self.max_weight).map(|g| round(g / 1000.0, 2)),
            volume_kg: positive(self.volume).map(|g| round(g / 1000.0, 1)),
            peak_power_w: rounded(self.peak_power, 1),
            // cm/ms to m/s: × 10.
            concentric_mean_velocity_m_s: positive(self.concentric_mean_velocity)
                .map(|v| round(v * 10.0, 2)),
        }
    }
}

/// One activity, in the shape and units the tool returns.
///
/// Only used for writing (`Serialize`). Each unit is in the field name.
/// `skip_serializing_if` leaves `None` fields out of the output entirely
/// instead of writing `null`, so each activity only shows what applies to
/// its sport. Speed is given the usual way for the sport: exactly one of each
/// `..._per_km` / `..._per_100m` / `..._kmh` group is filled.
#[derive(Debug, Serialize)]
pub struct ActivitySummary {
    // --- What, when, where ---
    /// Garmin's unique id for the activity.
    pub activity_id: u64,
    /// Name shown in Garmin Connect, e.g. "Morning Run".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_name: Option<String>,
    /// The user's own notes about the activity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Sport, e.g. `running`, `walking`, `strength_training`.
    pub activity_type: String,
    /// e.g. `race`, `training`; left out when `uncategorized`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    /// Start time in the activity's local time zone, "YYYY-MM-DD HH:MM:SS".
    pub start_time_local: String,
    /// Place name, if the activity had GPS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_name: Option<String>,
    /// `true` for a multisport activity that groups several legs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multisport_parent: Option<bool>,
    /// For one leg of a multisport activity: the parent's id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_activity_id: Option<u64>,
    /// `true` if a personal record was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personal_record: Option<bool>,
    /// `true` if entered by hand rather than recorded by a device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_entry: Option<bool>,
    /// Number of laps, when more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub laps: Option<u32>,

    // --- Time, distance, speed ---
    /// Timer time in minutes, pauses excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_min: Option<f64>,
    /// Time spent actually moving, in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moving_duration_min: Option<f64>,
    /// Start to finish, pauses included, in minutes. Only when it differs
    /// from `duration_min`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_duration_min: Option<f64>,
    /// Distance in kilometers (all sports except swimming).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_km: Option<f64>,
    /// Distance in meters (swimming only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
    /// Average pace as "minutes:seconds" per km (running, walking, hiking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pace_per_km: Option<String>,
    /// Average pace as "minutes:seconds" per 100 m (swimming).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pace_per_100m: Option<String>,
    /// Average speed in km/h (cycling and other distance sports).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_speed_kmh: Option<f64>,
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
    /// Fastest time over standard distances, e.g. `{"1km": "7:16"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fastest_splits: Option<FastestSplits>,

    // --- Elevation ---
    /// Total climb in meters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_gain_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_loss_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_elevation_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_elevation_m: Option<f64>,
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
    /// Step count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u64>,

    // --- Heart rate ---
    /// Average heart rate in beats per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_hr: Option<f64>,
    /// Maximum heart rate in beats per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr: Option<f64>,
    /// Minutes in each heart-rate zone (1 = easiest, 5 = hardest). Time
    /// below zone 1 is not counted, so the sum can be less than the
    /// duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hr_zones_min: Option<Zones>,
    /// Lactate threshold heart rate, bpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lactate_threshold_hr: Option<f64>,
    /// Lactate threshold pace / speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lactate_threshold_pace_per_km: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lactate_threshold_pace_per_100m: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lactate_threshold_speed_kmh: Option<f64>,

    // --- Power ---
    /// Average power in watts (only with a power meter or running power).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_power_w: Option<f64>,
    /// Best 20-minute power.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_20min_power_w: Option<f64>,
    /// Functional threshold power.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ftp_w: Option<f64>,
    /// Minutes in each power zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_zones_min: Option<Zones>,
    /// Best average power over windows from 1 s to 5 h, watts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_curve_w: Option<PowerCurve>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub training_stress_score: Option<f64>,
    /// Normalized power / FTP, e.g. 0.8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity_factor: Option<f64>,
    /// Left/right pedal power balance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_balance_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_balance_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_left_balance_percent: Option<f64>,
    /// Pedal force.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_force_n: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_force_n: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_force_n: Option<f64>,
    /// Cycling: time and power standing vs. seated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standing_time_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seated_time_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_standing_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_standing_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_seated_power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_seated_power_w: Option<f64>,

    // --- Cadence ---
    /// Running/walking cadence, steps per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_run_cadence_spm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_run_cadence_spm: Option<f64>,
    /// Highest average cadence of any lap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lap_avg_run_cadence_spm: Option<f64>,
    /// Cycling cadence, revolutions per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_bike_cadence_rpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bike_cadence_rpm: Option<f64>,
    /// Swim cadence, strokes per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_swim_cadence_spm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_swim_cadence_spm: Option<f64>,
    /// Wheelchair pushes per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_wheelchair_cadence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_wheelchair_cadence: Option<f64>,
    /// Jump rope: jumps per minute and total jumps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_jump_rope_cadence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_jump_rope_cadence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jumps: Option<u32>,

    // --- Running dynamics (need a compatible watch or sensor) ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stride_length_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_oscillation_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_contact_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_ratio_percent: Option<f64>,
    /// Share of ground contact time on the left foot; 50 is even.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_contact_balance_left_percent: Option<f64>,
    /// Speed lost at each foot strike; lower is more efficient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_speed_loss_cm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_speed_loss_percent: Option<f64>,
    /// Load on the body, as the equivalent distance of flat running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_load_m: Option<f64>,

    // --- Swimming and paddling ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_length_m: Option<f64>,
    /// Unit the pool was set up in, e.g. `meter`, `yard`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_length_unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_lengths: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_strokes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_strokes_per_length: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_strokes_per_length: Option<f64>,
    /// Strokes + seconds per length; lower is more efficient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_swolf: Option<f64>,
    /// Distance per stroke.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_stroke_distance_m: Option<f64>,
    /// Paddling/rowing stroke rate, strokes per minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_stroke_rate_spm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_stroke_rate_spm: Option<f64>,
    /// Pool apnea (breath-hold) training.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_apnea_interval_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_breath_hold_s: Option<f64>,

    // --- Strength training ---
    /// Number of working sets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_sets: Option<u32>,
    /// Number of repetitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_reps: Option<u32>,
    /// Sets, reps, weight and volume per exercise.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exercises: Vec<ExerciseSet>,

    // --- Energy and hydration ---
    /// Calories burned (kcal), resting calories included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calories_kcal: Option<f64>,
    /// The resting (basal) part of `calories_kcal`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_calories_kcal: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub energy_estimated_kj: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub energy_consumed_kj: Option<f64>,
    /// Estimated sweat loss.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweat_loss_ml: Option<f64>,
    /// Water the user logged drinking during the activity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_consumed_ml: Option<f64>,
    /// Pack carried (e.g. rucking) and the extra calories it cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_weight_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_weight_calories_kcal: Option<u32>,

    // --- Respiration (breaths per minute) ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_respiration_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_respiration_brpm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_respiration_brpm: Option<f64>,

    // --- Training effect and load ---
    /// Aerobic training effect, 0.0 to 5.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aerobic_training_effect: Option<f64>,
    /// Anaerobic training effect, 0.0 to 5.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anaerobic_training_effect: Option<f64>,
    /// Garmin's verdict, e.g. `IMPROVING_AEROBIC_BASE`, `NO_ANAEROBIC_BENEFIT`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aerobic_benefit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anaerobic_benefit: Option<String>,
    /// Garmin's label for the main benefit, e.g. `TEMPO`, `ANAEROBIC_CAPACITY`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub training_effect_label: Option<String>,
    /// Garmin's training load score for this activity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_training_load: Option<f64>,
    /// VO2 max estimate from this activity, ml/kg/min.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vo2_max: Option<f64>,
    /// Minutes of moderate intensity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderate_intensity_minutes: Option<u32>,
    /// Minutes of vigorous intensity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vigorous_intensity_minutes: Option<u32>,
    /// Change in Body Battery during the activity (usually negative).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difference_body_battery: Option<i64>,

    // --- Stress (0–100) ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_stress: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_stress: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_stress: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_stress: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difference_stress: Option<f64>,

    // --- Temperature ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_temperature_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_temperature_c: Option<f64>,

    // --- Diving ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_depth_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_time_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bottom_time_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_interval_min: Option<f64>,
    /// Nitrogen loading and CNS oxygen toxicity, percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_n2_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_n2_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_cns_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_cns_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deco_dive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_number: Option<u32>,

    // --- Mountain biking ---
    /// Garmin's smoothness score; lower is smoother.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<f64>,
    /// Garmin's trail difficulty score; higher is harder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_flow: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_grit: Option<f64>,

    // --- Video games / esports ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_type: Option<String>,

    // --- Other ---
    /// CO2 saved, grams.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carbon_offset_g: Option<f64>,
    /// Totals per split type, e.g. run/walk/stand time of a treadmill
    /// session, or ski runs vs. lifts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<SplitSummary>,
}

impl Activity {
    /// Converts Garmin's raw activity into the tool's [`ActivitySummary`].
    ///
    /// Takes `self` by value, so strings and lists are moved, not copied.
    /// Rules, the same as for activity details:
    /// - `0` usually means "not recorded" (e.g. distance for strength
    ///   training), so it is left out via `positive` / `count`.
    /// - Rounding only removes float32 noise (`2.4000000953674316`) and
    ///   detail that is irrelevant for health evaluation.
    pub fn into_summary(self) -> ActivitySummary {
        let style = SpeedStyle::for_activity(&self.activity_type);
        let duration_min = to_minutes(self.duration);
        // Elapsed time only adds something when the activity was paused;
        // otherwise it repeats `duration_min`.
        let elapsed_duration_min =
            to_minutes(self.elapsed_duration).filter(|e| Some(*e) != duration_min);
        // Meters for swimming, km otherwise.
        let (distance_km, distance_m) = style.distance(positive(self.distance));
        // Each speed comes out in exactly one of three forms (see
        // `SpeedStyle::convert`). `_` discards a form that can't apply, e.g.
        // grade-adjusted pace per 100 m.
        let (avg_pace_per_km, avg_pace_per_100m, avg_speed_kmh) =
            style.convert(positive(self.average_speed));
        let (best_pace_per_km, best_pace_per_100m, max_speed_kmh) =
            style.convert(positive(self.max_speed));
        let (grade_adjusted_pace_per_km, _, grade_adjusted_speed_kmh) =
            style.convert(positive(self.avg_grade_adjusted_speed));
        // cm/ms to m/s: × 10.
        let (
            lactate_threshold_pace_per_km,
            lactate_threshold_pace_per_100m,
            lactate_threshold_speed_kmh,
        ) = style.convert(positive(self.lactate_threshold_speed).map(|v| v * 10.0));

        // Split totals use the same speed style as the whole activity.
        // `&style` lends the style to every call without giving it away.
        let splits = self
            .split_summaries
            .into_iter()
            .map(|s| s.into_summary(&style))
            .collect();
        // Passing the method name is shorthand for `.map(|e| e.into_summary())`.
        let exercises = self
            .summarized_exercise_sets
            .into_iter()
            .map(RawExerciseSet::into_summary)
            .collect();

        // Small converters used several times below. Closures can be stored
        // in variables and called like functions.
        // m/s to m/h: × 3600, whole meters.
        let per_hour = |v: Option<f64>| positive(v).map(|v| round(v * 3600.0, 0));
        // Centimeters to meters.
        let cm_to_m = |v: Option<f64>, decimals| positive(v).map(|v| round(v / 100.0, decimals));

        ActivitySummary {
            activity_id: self.activity_id,
            activity_name: self.activity_name,
            // Garmin sends `""` when the user wrote no description.
            description: self.description.filter(|d| !d.trim().is_empty()),
            activity_type: self.activity_type,
            event_type: event_type(self.event_type.and_then(|t| t.type_key)),
            start_time_local: self.start_time_local,
            location_name: self.location_name,
            multisport_parent: only_true(self.is_parent),
            parent_activity_id: self.parent_id,
            personal_record: only_true(self.is_pr),
            manual_entry: only_true(self.is_manual_activity),
            // One lap is the default, so only more than one says something.
            laps: self.lap_count.filter(|n| *n > 1),

            // Seconds to minutes. 2 decimals = 0.6 s precision.
            duration_min,
            // Indoor activities report a moving time of 0 because Garmin
            // doesn't track it there; `to_minutes` leaves that out.
            moving_duration_min: to_minutes(self.moving_duration),
            elapsed_duration_min,
            distance_km,
            distance_m,
            avg_pace_per_km,
            avg_pace_per_100m,
            avg_speed_kmh,
            best_pace_per_km,
            best_pace_per_100m,
            max_speed_kmh,
            grade_adjusted_pace_per_km,
            grade_adjusted_speed_kmh,
            fastest_splits: self.fastest.into_summary(),

            elevation_gain_m: positive(self.elevation_gain),
            elevation_loss_m: positive(self.elevation_loss),
            // Altitude can be 0 or negative (at or below sea level), so it
            // isn't filtered; decimeter precision is plenty.
            min_elevation_m: self.min_elevation.map(|v| round(v, 1)),
            max_elevation_m: self.max_elevation.map(|v| round(v, 1)),
            // `-500.0` is Garmin's "no elevation" marker for indoor activities.
            avg_elevation_m: self
                .avg_elevation
                .filter(|v| *v != -500.0)
                .map(|v| round(v, 1)),
            avg_vertical_speed_m_per_h: per_hour(self.avg_vertical_speed),
            max_vertical_speed_m_per_h: per_hour(self.max_vertical_speed),
            floors_climbed: positive(self.floors_climbed),
            floors_descended: positive(self.floors_descended),
            steps: self.steps.filter(|n| *n > 0),

            average_hr: positive(self.average_hr),
            max_hr: positive(self.max_hr),
            hr_zones_min: zones([
                self.hr_zone_1,
                self.hr_zone_2,
                self.hr_zone_3,
                self.hr_zone_4,
                self.hr_zone_5,
                None,
                None,
            ]),
            lactate_threshold_hr: positive(self.lactate_threshold_bpm),
            lactate_threshold_pace_per_km,
            lactate_threshold_pace_per_100m,
            lactate_threshold_speed_kmh,

            avg_power_w: positive(self.avg_power),
            max_power_w: positive(self.max_power),
            normalized_power_w: positive(self.norm_power),
            max_20min_power_w: positive(self.max20_min_power),
            ftp_w: positive(self.max_ftp),
            power_zones_min: zones([
                self.power_zone_1,
                self.power_zone_2,
                self.power_zone_3,
                self.power_zone_4,
                self.power_zone_5,
                self.power_zone_6,
                self.power_zone_7,
            ]),
            // `(!empty).then_some(x)`: `Some(x)` if there is any value.
            power_curve_w: (!self.power_curve.is_empty()).then_some(self.power_curve),
            training_stress_score: rounded(self.training_stress_score, 1),
            intensity_factor: rounded(self.intensity_factor, 2),
            left_balance_percent: rounded(self.left_balance, 1),
            right_balance_percent: rounded(self.right_balance, 1),
            avg_left_balance_percent: rounded(self.avg_left_balance, 1),
            avg_force_n: rounded(self.avg_force, 1),
            max_force_n: rounded(self.max_force, 1),
            normalized_force_n: rounded(self.normalized_force, 1),
            standing_time_min: to_minutes(self.standing_time),
            seated_time_min: to_minutes(self.seated_time),
            avg_standing_power_w: positive(self.avg_standing_power),
            max_standing_power_w: positive(self.max_standing_power),
            avg_seated_power_w: positive(self.avg_seated_power),
            max_seated_power_w: positive(self.max_seated_power),

            // Cadence and running dynamics: 1 decimal is finer than any
            // meaningful change.
            avg_run_cadence_spm: rounded(self.average_running_cadence_in_steps_per_minute, 1),
            max_run_cadence_spm: rounded(self.max_running_cadence_in_steps_per_minute, 1),
            max_lap_avg_run_cadence_spm: rounded(self.max_lap_avg_run_cadence, 1),
            avg_bike_cadence_rpm: rounded(self.average_biking_cadence_in_rev_per_minute, 1),
            max_bike_cadence_rpm: rounded(self.max_biking_cadence_in_rev_per_minute, 1),
            avg_swim_cadence_spm: rounded(self.average_swim_cadence_in_strokes_per_minute, 1),
            max_swim_cadence_spm: rounded(self.max_swim_cadence_in_strokes_per_minute, 1),
            avg_wheelchair_cadence: rounded(self.avg_wheelchair_cadence, 1),
            max_wheelchair_cadence: rounded(self.max_wheelchair_cadence, 1),
            avg_jump_rope_cadence: rounded(self.avg_jump_rope_cadence, 1),
            max_jump_rope_cadence: rounded(self.max_jump_rope_cadence, 1),
            jumps: count(self.jump_count),

            stride_length_cm: rounded(self.avg_stride_length, 1),
            vertical_oscillation_cm: rounded(self.avg_vertical_oscillation, 1),
            ground_contact_time_ms: rounded(self.avg_ground_contact_time, 1),
            vertical_ratio_percent: rounded(self.avg_vertical_ratio, 1),
            ground_contact_balance_left_percent: rounded(self.avg_ground_contact_balance, 1),
            step_speed_loss_cm: rounded(self.avg_step_speed_loss, 1),
            step_speed_loss_percent: rounded(self.avg_step_speed_loss_percent, 1),
            impact_load_m: rounded(self.impact_load, 0),

            pool_length_m: cm_to_m(self.pool_length, 2),
            pool_length_unit: self.unit_of_pool_length.and_then(|u| u.unit_key),
            active_lengths: count(self.active_lengths),
            total_strokes: positive(self.strokes),
            avg_strokes_per_length: rounded(self.avg_strokes, 1),
            min_strokes_per_length: positive(self.min_strokes),
            avg_swolf: rounded(self.average_swolf, 1),
            avg_stroke_distance_m: cm_to_m(self.avg_stroke_distance, 2),
            avg_stroke_rate_spm: rounded(self.avg_stroke_cadence, 1),
            max_stroke_rate_spm: rounded(self.max_stroke_cadence, 1),
            longest_apnea_interval_s: rounded(self.longest_pool_apnea_interval, 1),
            total_breath_hold_s: rounded(self.pool_apnea_breath_hold_duration, 1),

            // `activeSets` excludes rest periods; in this response it equals
            // `totalSets`, but prefer it when both exist.
            total_sets: count(self.active_sets.or(self.total_sets)),
            total_reps: count(self.total_reps),
            exercises,

            calories_kcal: positive(self.calories),
            resting_calories_kcal: positive(self.bmr_calories),
            energy_estimated_kj: rounded(self.calories_estimated, 0),
            energy_consumed_kj: rounded(self.calories_consumed, 0),
            sweat_loss_ml: positive(self.water_estimated),
            water_consumed_ml: positive(self.water_consumed),
            // Grams to kilograms.
            pack_weight_kg: positive(self.begin_pack_weight).map(|g| round(g / 1000.0, 1)),
            pack_weight_calories_kcal: count(self.pack_weight_calories),

            avg_respiration_brpm: rounded(self.avg_respiration_rate, 1),
            min_respiration_brpm: rounded(self.min_respiration_rate, 1),
            max_respiration_brpm: rounded(self.max_respiration_rate, 1),

            // Training effect uses 0.1 steps, so 1 decimal loses nothing.
            // `0.0` is a real result here ("no effect"), so it is kept.
            aerobic_training_effect: self.aerobic_training_effect.map(|v| round(v, 1)),
            anaerobic_training_effect: self.anaerobic_training_effect.map(|v| round(v, 1)),
            aerobic_benefit: self.aerobic_training_effect_message.map(benefit),
            anaerobic_benefit: self.anaerobic_training_effect_message.map(benefit),
            training_effect_label: self.training_effect_label,
            // Garmin itself only shows whole numbers for load.
            activity_training_load: self.activity_training_load.map(|v| round(v, 1)),
            vo2_max: rounded(self.vo2_max_value, 1),
            moderate_intensity_minutes: count(self.moderate_intensity_minutes),
            vigorous_intensity_minutes: count(self.vigorous_intensity_minutes),
            // Can be negative (drained) or positive (charged); `0` is a real
            // "no change", so it is kept.
            difference_body_battery: self.difference_body_battery,

            avg_stress: positive(self.avg_stress),
            start_stress: positive(self.start_stress),
            end_stress: positive(self.end_stress),
            max_stress: positive(self.max_stress),
            // Can be negative; only `0` is dropped.
            difference_stress: self.difference_stress.filter(|v| *v != 0.0),

            // 0 °C and below are real temperatures, so these aren't filtered.
            min_temperature_c: self.min_temperature.map(|v| round(v, 1)),
            max_temperature_c: self.max_temperature.map(|v| round(v, 1)),

            max_depth_m: cm_to_m(self.max_depth, 1),
            avg_depth_m: cm_to_m(self.avg_depth, 1),
            bottom_time_min: to_minutes(self.bottom_time),
            max_bottom_time_min: to_minutes(self.max_bottom_time),
            // Milliseconds to minutes.
            surface_interval_min: to_minutes(self.surface_interval.map(|ms| ms / 1000.0)),
            start_n2_percent: rounded(self.start_n2, 1),
            end_n2_percent: rounded(self.end_n2, 1),
            start_cns_percent: rounded(self.start_cns, 1),
            end_cns_percent: rounded(self.end_cns, 1),
            deco_dive: only_true(self.is_deco_dive),
            dive_count: count(self.dive_count),
            dive_number: count(self.dive_number),

            flow: rounded(self.flow, 1),
            grit: rounded(self.grit, 1),
            avg_flow: rounded(self.avg_flow, 1),
            avg_grit: rounded(self.avg_grit, 1),

            game_name: self.game_name.filter(|s| !s.is_empty()),
            game_type: self.game_type.filter(|s| !s.is_empty()),

            carbon_offset_g: rounded(self.carbon_offset, 0),
            splits,
        }
    }
}

/// Reads Garmin's `activityType` object and returns only its `typeKey`.
///
/// Used through `#[serde(deserialize_with = "type_key")]` on [`Activity`].
///
/// The signature looks scary but is standard serde boilerplate:
/// - `'de` is a *lifetime*: a name for "how long the input data lives".
///   serde uses it so deserialized values may borrow from the input.
/// - `D: Deserializer<'de>` means "any JSON reader"; it is a *generic*
///   parameter, so this one function works with whatever reader serde uses.
/// - `D::Error` is that reader's error type.
fn type_key<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    // Deserialize the object into the shared `TypeKey` struct, then return
    // just the string (empty if Garmin left it out).
    Ok(TypeKey::deserialize(deserializer)?
        .type_key
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed-down copy of a real strength-training entry from Garmin.
    // `r#"..."#` is a *raw string*: quotes inside need no escaping.
    const STRENGTH_JSON: &str = r#"{
        "activityId": 24824822921,
        "activityName": "Leg day",
        "activityType": {"typeId": 13, "typeKey": "strength_training", "parentTypeId": 29},
        "eventType": {"typeId": 9, "typeKey": "uncategorized"},
        "startTimeLocal": "2026-10-23 13:24:21",
        "duration": 4239.2,
        "distance": 0.0,
        "averageHR": 124.0,
        "maxHR": 159.0,
        "calories": 617.0,
        "totalSets": 24,
        "activeSets": 24,
        "totalReps": 285,
        "isPR": false,
        "lapCount": 1,
        "ownerFullName": "ignored",
        "hrTimeInZone_1": 822.537, "hrTimeInZone_2": 2145.585,
        "hrTimeInZone_3": 1020.966, "hrTimeInZone_4": 71.0, "hrTimeInZone_5": 0.0,
        "summarizedExerciseSets": [
            {"category": "LATERAL_RAISE", "subCategory": "ONE_ARM_CABLE_LATERAL_RAISE",
             "reps": 54, "volume": 435000, "duration": 452469.99, "sets": 4, "maxWeight": 10000},
            {"category": "LEG_RAISE", "subCategory": "HANGING_KNEE_RAISE",
             "reps": 66, "volume": 0, "duration": 183635.0, "sets": 3, "maxWeight": 0}
        ],
        "splitSummaries": []
    }"#;

    /// Parses one activity from JSON and converts it to the output shape.
    fn summary(json: &str) -> ActivitySummary {
        // `serde_json::from_str` parses text into our struct. `.unwrap()`
        // panics (fails the test) if parsing returns an error.
        let activity: Activity = serde_json::from_str(json).unwrap();
        activity.into_summary()
    }

    #[test]
    fn strength_training_has_no_distance_or_speed() {
        let out = summary(STRENGTH_JSON);
        assert_eq!(out.activity_type, "strength_training");
        assert_eq!(out.duration_min, Some(70.65));
        assert_eq!(out.average_hr, Some(124.0));
        assert_eq!(out.total_sets, Some(24));
        assert_eq!(out.total_reps, Some(285));
        // Distance 0 means "no distance", so all distance/speed fields are
        // empty. `avg_power` was missing from the JSON.
        assert_eq!(out.distance_km, None);
        assert_eq!(out.avg_pace_per_km, None);
        assert_eq!(out.avg_speed_kmh, None);
        assert_eq!(out.avg_power_w, None);
    }

    #[test]
    fn heart_rate_zones_in_minutes() {
        let out = serde_json::to_value(summary(STRENGTH_JSON)).unwrap();
        // 822.537 s = 13.7 min, ... Zone 5 had no time but is still shown;
        // zones 6 and 7 don't exist for heart rate.
        assert_eq!(
            out["hr_zones_min"],
            serde_json::json!({"zone_1": 13.7, "zone_2": 35.8, "zone_3": 17.0,
                               "zone_4": 1.2, "zone_5": 0.0})
        );
        // No power zones in the JSON at all.
        assert!(out.get("power_zones_min").is_none());
    }

    #[test]
    fn exercise_sets_in_kg_and_minutes() {
        let out = summary(STRENGTH_JSON);
        assert_eq!(out.exercises.len(), 2);
        let raise = &out.exercises[0];
        assert_eq!(raise.exercise.as_deref(), Some("LATERAL_RAISE"));
        assert_eq!(
            raise.variant.as_deref(),
            Some("ONE_ARM_CABLE_LATERAL_RAISE")
        );
        assert_eq!(raise.sets, Some(4));
        assert_eq!(raise.max_weight_kg, Some(10.0));
        assert_eq!(raise.volume_kg, Some(435.0));
        // 452470 ms = 7.54 min.
        assert_eq!(raise.duration_min, Some(7.54));
        // Body-weight exercise: weight and volume `0` are left out.
        assert_eq!(out.exercises[1].max_weight_kg, None);
        assert_eq!(out.exercises[1].volume_kg, None);
    }

    #[test]
    fn walking_gets_km_and_pace_per_km() {
        // Raw values exactly as Garmin sends them.
        let out = summary(
            r#"{
            "activityId": 1,
            "activityType": {"typeKey": "walking"},
            "duration": 3741.52587890625,
            "distance": 4380.18017578125,
            "averageSpeed": 1.1710000038146973,
            "maxSpeed": 1.8289999961853027,
            "aerobicTrainingEffect": 2.4000000953674316,
            "aerobicTrainingEffectMessage": "MAINTAINING_AEROBIC_BASE_7",
            "activityTrainingLoad": 62.689056396484375,
            "averageHR": 83.0,
            "avgElevation": 2.4000000953674316,
            "minElevation": -2.0,
            "maxVerticalSpeed": 0.8000001907348633
        }"#,
        );
        assert_eq!(out.duration_min, Some(62.36));
        assert_eq!(out.distance_km, Some(4.38));
        // 1000 m / 1.171 m/s = 853.97 s = 14 min 14 s.
        assert_eq!(out.avg_pace_per_km.as_deref(), Some("14:14"));
        assert_eq!(out.best_pace_per_km.as_deref(), Some("9:07"));
        assert_eq!(out.avg_speed_kmh, None);
        // Float noise removed.
        assert_eq!(out.aerobic_training_effect, Some(2.4));
        assert_eq!(out.activity_training_load, Some(62.7));
        assert_eq!(
            out.aerobic_benefit.as_deref(),
            Some("MAINTAINING_AEROBIC_BASE")
        );
        // Not rounded: already clean.
        assert_eq!(out.average_hr, Some(83.0));
        // Below sea level is kept.
        assert_eq!(out.min_elevation_m, Some(-2.0));
        assert_eq!(out.avg_elevation_m, Some(2.4));
        // 0.8 m/s = 2880 m/h.
        assert_eq!(out.max_vertical_speed_m_per_h, Some(2880.0));
    }

    #[test]
    fn treadmill_run_with_power_and_splits() {
        // 1000 / 2.778 = 360.0 s = exactly 6:00 per km.
        let out = summary(
            r#"{"activityId": 1, "activityType": {"typeKey": "treadmill_running"},
                "distance": 5000.0, "averageSpeed": 2.778, "avgElevation": -500.0,
                "avgPower": 78.0, "normPower": 85.0,
                "avgGroundContactTime": 336.0, "avgStrideLength": 68.10999755859375,
                "powerTimeInZone_1": 0.0, "powerTimeInZone_2": 0.0,
                "maxAvgPower_1": 185, "maxAvgPower_60": 120, "maxAvgPower_7200": 0,
                "fastestSplit_1000": 436.217, "fastestSplit_5000": 2277.897,
                "splitSummaries": [
                    {"noOfSplits": 3, "duration": 42.0, "splitType": "RWD_RUN",
                     "maxDistance": 27, "distance": 40.46, "averageSpeed": 0.963,
                     "maxSpeed": 1.474, "avgStepFrequency": 122.546875,
                     "avgStepLength": 0.6810999755859375, "numFalls": 0}
                ]}"#,
        );
        assert_eq!(out.avg_pace_per_km.as_deref(), Some("6:00"));
        // `-500.0` means "no elevation", so it is left out.
        assert_eq!(out.avg_elevation_m, None);
        assert_eq!(out.normalized_power_w, Some(85.0));
        assert_eq!(out.stride_length_cm, Some(68.1));
        // Every power zone is 0: no power-zone object at all.
        assert!(out.power_zones_min.is_none());

        let json = serde_json::to_value(&out).unwrap();
        // Zero windows (2 h) are left out.
        assert_eq!(
            json["power_curve_w"],
            serde_json::json!({"1s": 185, "1min": 120})
        );
        assert_eq!(
            json["fastest_splits"],
            serde_json::json!({"1km": "7:16", "5km": "37:58"})
        );

        let split = &out.splits[0];
        assert_eq!(split.split_type.as_deref(), Some("RWD_RUN"));
        assert_eq!(split.longest_km, Some(0.027));
        assert_eq!(split.avg_cadence_spm, Some(122.5));
        assert_eq!(split.avg_stride_length_cm, Some(68.1));
        // No falls: left out.
        assert_eq!(split.falls, None);
    }

    #[test]
    fn swimming_gets_meters_and_pace_per_100m() {
        // 100 / 0.8 = 125 s = 2:05 per 100 m.
        let out = summary(
            r#"{"activityId": 1, "activityType": {"typeKey": "lap_swimming"},
                "distance": 1500.0, "averageSpeed": 0.8,
                "poolLength": 2500.0, "unitOfPoolLength": {"unitKey": "meter"},
                "averageSwolf": 47.0, "fastestSplit_100": 98.4}"#,
        );
        assert_eq!(out.distance_m, Some(1500.0));
        assert_eq!(out.distance_km, None);
        assert_eq!(out.avg_pace_per_100m.as_deref(), Some("2:05"));
        assert_eq!(out.avg_pace_per_km, None);
        // Garmin sends centimeters.
        assert_eq!(out.pool_length_m, Some(25.0));
        assert_eq!(out.pool_length_unit.as_deref(), Some("meter"));
        assert_eq!(out.avg_swolf, Some(47.0));
    }

    #[test]
    fn cycling_gets_km_per_hour() {
        // 7.03 m/s * 3.6 = 25.308 km/h.
        let out = summary(
            r#"{"activityId": 1, "activityType": {"typeKey": "indoor_cycling"},
                "distance": 20000.0, "averageSpeed": 7.03, "movingDuration": 0.0,
                "averageBikingCadenceInRevPerMinute": 85.5, "intensityFactor": 0.7812,
                "trainingStressScore": 54.321}"#,
        );
        assert_eq!(out.distance_km, Some(20.0));
        // Indoor: Garmin's moving time of 0 means "not tracked".
        assert_eq!(out.moving_duration_min, None);
        assert_eq!(out.avg_speed_kmh, Some(25.3));
        assert_eq!(out.avg_pace_per_km, None);
        assert_eq!(out.avg_bike_cadence_rpm, Some(85.5));
        assert_eq!(out.intensity_factor, Some(0.78));
        assert_eq!(out.training_stress_score, Some(54.3));
    }

    #[test]
    fn output_skips_missing_and_noise_fields() {
        let out = serde_json::to_value(summary(STRENGTH_JSON)).unwrap();
        assert_eq!(out["activity_type"], "strength_training");
        // `None` fields are left out, and Garmin fields we don't model
        // (like `ownerFullName`) never appear.
        assert!(out.get("avg_power_w").is_none());
        assert!(out.get("distance_km").is_none());
        assert!(out.get("ownerFullName").is_none());
        // Default values that say nothing: `uncategorized`, `isPR: false`,
        // a single lap, empty splits.
        assert!(out.get("event_type").is_none());
        assert!(out.get("personal_record").is_none());
        assert!(out.get("laps").is_none());
        assert!(out.get("splits").is_none());
        assert!(out.get("fastest_splits").is_none());
        assert!(out.get("power_curve_w").is_none());
    }

    #[test]
    fn minimal_activity_has_only_id_type_and_start() {
        let json = serde_json::to_string(&summary(
            r#"{"activityId": 7, "activityType": {"typeKey": "other"},
                "startTimeLocal": "2026-10-24 11:06:58"}"#,
        ))
        .unwrap();
        assert_eq!(
            json,
            r#"{"activity_id":7,"activity_type":"other","start_time_local":"2026-10-24 11:06:58"}"#
        );
    }
}
