//! Shared Garmin types and helpers.
//!
//! - [`ActivityTypeFilter`]: the sport groups the activity list accepts.
//! - [`SpeedStyle`] and the small number helpers (`round`, `format_pace`,
//!   `positive`, ...): unit conversions shared by both activity outputs
//!   (`activity.rs` and `activity_detail.rs`).
//!
//! The response types live in their own files: `activity.rs`,
//! `activity_detail.rs`, `sleep.rs`.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Deserializer, Serialize};

/// Sport groups accepted by the activity list's `activityType` filter.
///
/// Garmin filters only by *parent* type: `running` also returns treadmill
/// runs, and strength training is under `fitness_equipment`. Exact subtypes
/// like `strength_training` are rejected by Garmin, so they aren't listed.
///
/// An `enum` is a type whose value is exactly one of a fixed set of
/// *variants*. Using one here (instead of a free-form `String`) means:
/// - the AI client sees the allowed values in the tool's JSON schema
///   (`JsonSchema`), and
/// - anything else is rejected while reading the tool arguments
///   (`Deserialize`), before we ever call Garmin.
///
/// `rename_all = "snake_case"` maps `FitnessEquipment` <-> `"fitness_equipment"`
/// both when reading tool input and when writing the URL (`Serialize`).
/// `Clone, Copy` let the value be duplicated freely; it is tiny.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTypeFilter {
    /// Running, including treadmill, trail and track running.
    Running,
    /// Cycling, including indoor cycling.
    Cycling,
    /// Walking.
    Walking,
    /// Hiking.
    Hiking,
    /// Swimming (pool and open water).
    Swimming,
    /// Gym activities: strength training, stair climbing (stair stepper),
    /// elliptical, etc. Floor climbing is not here: it is under `Other`.
    FitnessEquipment,
    /// Activities that fit no other group, e.g. floor climbing and breathwork.
    Other,
    /// Multisport, e.g. triathlon.
    MultiSport,
    /// Skiing, snowboarding and other winter sports.
    WinterSports,
    /// Rowing, paddling, surfing and other water sports.
    WaterSports,
    /// Diving.
    Diving,
    /// Team sports, e.g. football, basketball.
    TeamSports,
    /// Racket sports, e.g. tennis, padel.
    RacketSports,
}

/// How speed is usually expressed for a sport.
///
/// A plain `enum` with no data in its variants. `pub(super)` makes it visible
/// to the parent module (`garmin`) and its other files, like
/// `activity_detail.rs`, but not to the rest of the crate.
pub(super) enum SpeedStyle {
    /// Minutes per kilometer: running, walking, hiking.
    PacePerKm,
    /// Minutes per 100 meters: swimming.
    PacePer100m,
    /// Kilometers per hour: cycling and everything else.
    KmPerHour,
}

impl SpeedStyle {
    /// Picks the style from Garmin's activity `typeKey`.
    ///
    /// Matches on parts of the name so all variants are covered, e.g.
    /// `treadmill_running`, `trail_running`, `virtual_run`, `lap_swimming`,
    /// `open_water_swimming`, `casual_walking`.
    pub(super) fn for_activity(type_key: &str) -> Self {
        if type_key.contains("swim") {
            Self::PacePer100m
        } else if type_key.contains("run") || type_key.contains("walk") || type_key.contains("hik")
        {
            Self::PacePerKm
        } else {
            Self::KmPerHour
        }
    }

    /// Converts a speed in m/s into the three output forms, of which exactly
    /// one is filled for this style: `(pace per km, pace per 100 m, km/h)`.
    ///
    /// A *tuple* `(A, B, C)` groups several values without defining a struct;
    /// callers unpack it with `let (a, b, c) = ...`.
    /// `&self` borrows the style (it is only read).
    pub(super) fn convert(
        &self,
        speed: Option<f64>,
    ) -> (Option<String>, Option<String>, Option<f64>) {
        // A `match` must handle every variant of the enum; the compiler
        // checks that none is forgotten.
        // Pace = time per distance = distance / speed (in seconds).
        match self {
            Self::PacePerKm => (speed.map(|v| format_pace(1000.0 / v)), None, None),
            Self::PacePer100m => (None, speed.map(|v| format_pace(100.0 / v)), None),
            // m/s to km/h: multiply by 3600 s/h and divide by 1000 m/km.
            Self::KmPerHour => (None, None, speed.map(|v| round(v * 3.6, 1))),
        }
    }

    /// Splits a distance in meters into `(km, m)`: swimming keeps meters,
    /// everything else uses km with 3 decimals, which is still meter
    /// precision.
    pub(super) fn distance(&self, meters: Option<f64>) -> (Option<f64>, Option<f64>) {
        // `matches!(value, pattern)` is `true` if the value fits the pattern.
        if matches!(self, Self::PacePer100m) {
            (None, meters.map(|m| round(m, 0)))
        } else {
            (meters.map(|m| round(m / 1000.0, 3)), None)
        }
    }
}

/// Rounds `value` to `decimals` decimal places.
pub(super) fn round(value: f64, decimals: i32) -> f64 {
    // `10f64.powi(n)` is 10 to the power `n` (`f64` is the number type, and
    // `10f64` is the literal 10 written as an `f64`).
    let factor = 10f64.powi(decimals);
    (value * factor).round() / factor
}

/// Formats a number of seconds as "minutes:seconds", e.g. `383.4` -> "6:23".
pub(super) fn format_pace(seconds: f64) -> String {
    // Round to whole seconds first, so 359.6 s becomes "6:00", not "5:60".
    // `as u64` converts the float to an unsigned integer.
    let total = seconds.round() as u64;
    // `/` on integers divides and drops the remainder; `%` is the remainder.
    // `{:02}` pads with a leading zero to 2 digits ("6:05", not "6:5").
    format!("{}:{:02}", total / 60, total % 60)
}

/// Formats a number of seconds as "m:ss", or "h:mm:ss" from one hour up,
/// e.g. `2277.9` -> "37:58", `7385.0` -> "2:03:05". For finish times, where
/// "123:05" would be hard to read.
pub(super) fn format_time(seconds: f64) -> String {
    let total = seconds.round() as u64;
    if total < 3600 {
        format_pace(seconds)
    } else {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    }
}

/// Treats `0.0` (and below) as "not recorded": Garmin sends `0.0` for
/// values a sport doesn't track, e.g. distance for strength training.
pub(super) fn positive(value: Option<f64>) -> Option<f64> {
    value.filter(|v| *v > 0.0)
}

/// Like [`positive`], then rounded to `decimals`.
pub(super) fn rounded(value: Option<f64>, decimals: i32) -> Option<f64> {
    positive(value).map(|v| round(v, decimals))
}

/// Seconds to minutes with 2 decimals (0.6 s precision); `0` → `None`.
pub(super) fn to_minutes(seconds: Option<f64>) -> Option<f64> {
    positive(seconds).map(|s| round(s / 60.0, 2))
}

/// Treats a count of `0` as "not recorded", like [`positive`] for numbers
/// with decimals.
pub(super) fn count(value: Option<u32>) -> Option<u32> {
    value.filter(|n| *n > 0)
}

/// Keeps a yes/no flag only when it is `true`: `false` is the normal case
/// and would only add noise to every activity.
pub(super) fn only_true(flag: Option<bool>) -> Option<bool> {
    flag.filter(|f| *f)
}

/// Drops the trailing message number from a training-effect message:
/// `MAINTAINING_AEROBIC_FITNESS_1` → `MAINTAINING_AEROBIC_FITNESS`.
/// The number only picks Garmin's wording variant; the words carry the
/// meaning.
pub(super) fn benefit(message: String) -> String {
    // `rsplit_once('_')` splits at the last `_` into `(before, after)`.
    // `if let` + `&&` (a let chain) keeps the cut only when `after` is all
    // digits, so messages without a number are left alone.
    if let Some((words, number)) = message.rsplit_once('_')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
    {
        return words.to_string();
    }
    message
}

/// Drops Garmin's default event type `uncategorized`, which says nothing.
/// Real values are e.g. `race`, `training`, `fitness`.
pub(super) fn event_type(key: Option<String>) -> Option<String> {
    key.filter(|k| k != "uncategorized")
}

/// Reads a field that Garmin may send as `null`, turning `null` into the
/// type's empty default (an empty list for `Vec`).
///
/// Why it is needed: `#[serde(default)]` on a struct only fills in a field
/// that is *missing* from the JSON. A field that is present but `null` is
/// still handed to the field's own `Deserialize`, and `Vec` refuses `null`
/// ("invalid type: null, expected a sequence"). Garmin does send `null` lists,
/// e.g. `dailyNapDTOS` on some nights of the sleep range.
///
/// Used as `#[serde(deserialize_with = "null_as_default")]` on a field.
/// - `'de` is a *lifetime*: it ties borrowed data to the input being read.
/// - `D: Deserializer<'de>` is "any JSON reader"; `T` is the field's type,
///   which must be readable (`Deserialize`) and have an empty value
///   (`Default`).
///
/// It reads the value as `Option<T>` (`null` → `None`), then
/// `unwrap_or_default()` swaps `None` for `T::default()`. The `?` returns
/// early with the error if the value is neither `null` nor a valid `T`.
pub(super) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_rounds_seconds_without_showing_60() {
        assert_eq!(format_pace(359.6), "6:00");
        assert_eq!(format_pace(365.0), "6:05");
    }

    #[test]
    fn time_adds_hours_from_one_hour() {
        assert_eq!(format_time(2277.9), "37:58");
        assert_eq!(format_time(7385.0), "2:03:05");
    }

    #[test]
    fn benefit_drops_only_a_trailing_number() {
        assert_eq!(benefit("IMPACTING_TEMPO_22".into()), "IMPACTING_TEMPO");
        assert_eq!(benefit("RECOVERY".into()), "RECOVERY");
        assert_eq!(benefit("ENDS_WITH_".into()), "ENDS_WITH_");
    }
}
