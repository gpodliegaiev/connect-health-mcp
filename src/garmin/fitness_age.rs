//! Types for Fitness Age
//! (`GET /fitnessage-service/fitnessage/v2/{calendarDate}`).
//!
//! Same two-step pattern as the other response types: [`FitnessAge`]
//! mirrors Garmin's response, and [`FitnessAge::into_summary`] turns it into
//! the [`FitnessAgeSummary`] the tool returns.
//!
//! Fitness Age is Garmin's estimate of how old the user's body is, based on
//! a few *components*: vigorous days and vigorous minutes per week (averaged
//! over the last weeks), resting heart rate, and BMI or body fat. For each
//! component Garmin may add a target value and the Fitness Age the user would
//! reach by hitting it, which tells an AI where the biggest gain is.
//!
//! Found by probing (the spec differs or is silent):
//! - The response is one object, not the array the spec promises.
//! - It is the latest Fitness Age as of the date, so a future date returns
//!   today's values. `lastUpdated` says which day they are from (always at
//!   midnight, so only the date is output).
//! - Garmin uses `bodyFat` when there is a body-fat reading (smart scale)
//!   and `bmi` otherwise.
//! - A day with no Fitness Age (e.g. before the user had a watch) still
//!   answers 200, with only `chronologicalAge` and every component marked
//!   `stale` with the missing inputs (`staleInputs`, e.g. `["RHR"]`).
//! - `achievableFitnessAge` and the per-component `targetValue`,
//!   `improvementValue`, `potentialAge` and `priority` only appear when
//!   Garmin has a suggestion.
//! - The `fullHistoryScan` query param made no difference in any probe, so
//!   it is not sent (Garmin's default `true` searches the full history).
//! - A malformed date returns 404 `{}`.
//!
//! Nothing is left out: every field Garmin sends is about the user's fitness.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use super::types::{null_as_default, only_true, round};

/// Garmin's Fitness Age response, as it arrives.
///
/// `default` on the struct lets any missing field fall back to its default
/// (`None`, or an empty list), so a sparse response still parses.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FitnessAge {
    /// The user's actual age in years.
    chronological_age: Option<u32>,
    /// Calculated Fitness Age, in years (with many decimals).
    fitness_age: Option<f64>,
    /// Fitness Age reachable by hitting every component's target.
    achievable_fitness_age: Option<f64>,
    /// The Fitness Age before the latest update, to show the trend.
    previous_fitness_age: Option<f64>,
    /// The inputs Garmin used. Each one is optional.
    components: RawComponents,
    /// Day the values are from, "YYYY-MM-DDT00:00:00.0".
    last_updated: Option<String>,
    /// Why there is no Fitness Age, when there is none (from the spec).
    invalid_reason: Option<String>,
}

/// The `components` object: one entry per input to Fitness Age.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawComponents {
    /// Average number of days per week with vigorous activity.
    vigorous_days_avg: Option<RawComponent>,
    /// Average vigorous intensity minutes per week.
    vigorous_minutes_avg: Option<RawComponent>,
    /// Resting heart rate, bpm.
    rhr: Option<RawComponent>,
    /// Body mass index. Used when there is no body-fat reading.
    bmi: Option<RawComponent>,
    /// Body fat, percent (from a smart scale).
    body_fat: Option<RawComponent>,
}

/// One Fitness Age component, as it arrives. Every component has the same
/// shape; which fields are filled depends on the component and the data.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawComponent {
    /// The user's current value, in the component's unit.
    value: Option<f64>,
    /// The value Garmin suggests aiming for.
    target_value: Option<f64>,
    /// How far the value is from the target (e.g. BMI 25.0 → 21.1 is 3.9).
    improvement_value: Option<f64>,
    /// Fitness Age the user would have after reaching `target_value`.
    potential_age: Option<f64>,
    /// Which suggestion helps most: 1 is the biggest gain.
    priority: Option<u32>,
    /// `true` when there is no recent data for this component.
    stale: Option<bool>,
    /// Which inputs are missing, e.g. `["RHR"]` or `["WEIGHT"]`.
    #[serde(deserialize_with = "null_as_default")]
    stale_inputs: Vec<String>,
    /// Day of the latest weight or body-fat measurement, "YYYY-MM-DD".
    last_measurement_date: Option<String>,
    /// Number of weeks the weekly average is taken over (Garmin's name means
    /// "for intensity minutes").
    num_of_weeks_for_im: Option<u32>,
}

/// What the `get_fitness_age` tool returns.
///
/// `skip_serializing_if = "Option::is_none"` leaves a field out of the JSON
/// when it is `None`, instead of writing `null`.
#[derive(Debug, Serialize)]
pub struct FitnessAgeSummary {
    /// Day the values are from, "YYYY-MM-DD" (the latest on or before the
    /// requested date).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
    /// Actual age, years.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chronological_age: Option<u32>,
    /// Fitness Age, years (1 decimal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitness_age: Option<f64>,
    /// Fitness Age before the latest update, years.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_fitness_age: Option<f64>,
    /// Fitness Age reachable by hitting every target, years.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub achievable_fitness_age: Option<f64>,
    /// Why there is no Fitness Age.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    /// The inputs, each named with its unit.
    pub components: Components,
}

/// Fitness Age inputs, with the unit in each name. Missing ones are left out.
#[derive(Debug, Default, Serialize)]
pub struct Components {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vigorous_days_per_week: Option<Component>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vigorous_minutes_per_week: Option<Component>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr_bpm: Option<Component>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bmi: Option<Component>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_fat_percent: Option<Component>,
}

/// One input to Fitness Age. `value`, `target_value` and `improvement` are in
/// the unit named by the component (see [`Components`]).
#[derive(Debug, Serialize)]
pub struct Component {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Suggested value to aim for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_value: Option<f64>,
    /// Distance from `value` to `target_value`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub improvement: Option<f64>,
    /// Fitness Age after reaching `target_value`, years.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub potential_fitness_age: Option<f64>,
    /// 1 = the suggestion with the biggest gain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
    /// Weeks the weekly average covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub averaged_over_weeks: Option<u32>,
    /// Day of the latest weight or body-fat measurement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_measurement_date: Option<String>,
    /// Only output when `true`: no recent data for this component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<bool>,
    /// The missing inputs, e.g. `["RHR"]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stale_inputs: Vec<String>,
}

impl FitnessAge {
    /// Converts Garmin's response into the tool's output.
    ///
    /// `self` (not `&self`) takes ownership, so the strings can be moved into
    /// the summary instead of copied.
    pub fn into_summary(self) -> FitnessAgeSummary {
        // Ages arrive as float32-noisy doubles (`30.42472488496677`); one
        // decimal is what Garmin's app shows.
        let years = |age: Option<f64>| age.map(|a| round(a, 1));
        let c = self.components;

        FitnessAgeSummary {
            last_updated: self.last_updated.map(date_only),
            chronological_age: self.chronological_age,
            fitness_age: years(self.fitness_age),
            previous_fitness_age: years(self.previous_fitness_age),
            achievable_fitness_age: years(self.achievable_fitness_age),
            // An empty string would say nothing, so it is dropped.
            invalid_reason: self.invalid_reason.filter(|r| !r.trim().is_empty()),
            components: Components {
                // `.map(RawComponent::into_summary)` converts the component
                // only when it is present.
                vigorous_days_per_week: c.vigorous_days_avg.map(RawComponent::into_summary),
                vigorous_minutes_per_week: c.vigorous_minutes_avg.map(RawComponent::into_summary),
                resting_hr_bpm: c.rhr.map(RawComponent::into_summary),
                bmi: c.bmi.map(RawComponent::into_summary),
                body_fat_percent: c.body_fat.map(RawComponent::into_summary),
            },
        }
    }
}

impl RawComponent {
    /// Converts one component. Values are kept as sent (already 1 decimal),
    /// only rounded to 1 decimal to remove float noise.
    fn into_summary(self) -> Component {
        let one_decimal = |v: Option<f64>| v.map(|v| round(v, 1));
        Component {
            value: one_decimal(self.value),
            target_value: one_decimal(self.target_value),
            improvement: one_decimal(self.improvement_value),
            potential_fitness_age: one_decimal(self.potential_age),
            priority: self.priority,
            averaged_over_weeks: self.num_of_weeks_for_im,
            last_measurement_date: self.last_measurement_date,
            stale: only_true(self.stale),
            stale_inputs: self.stale_inputs,
        }
    }
}

/// Keeps only the date of `"2026-10-24T00:00:00.0"` → `"2026-10-24"`.
/// Garmin always sends midnight here. Text in another format is kept as is.
fn date_only(text: String) -> String {
    match NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S%.f") {
        Ok(time) => time.date().to_string(),
        Err(_) => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a JSON value as Garmin's response and converts it to the tool
    /// output, as JSON (so tests also check which fields are left out).
    fn summary(json: serde_json::Value) -> serde_json::Value {
        let raw: FitnessAge = serde_json::from_value(json).unwrap();
        serde_json::to_value(raw.into_summary()).unwrap()
    }

    #[test]
    fn suggestions_are_converted() {
        // Real response for a day with suggestions (BMI, vigorous days).
        let out = summary(serde_json::json!({
            "chronologicalAge": 35,
            "fitnessAge": 30.83807597434009,
            "achievableFitnessAge": 29.340520234469633,
            "previousFitnessAge": 30.83807597434009,
            "components": {
                "vigorousDaysAvg": {"value": 2.5, "targetValue": 3, "potentialAge": 31.589901864459407,
                                    "priority": 2, "stale": false, "numOfWeeksForIm": 6},
                "rhr": {"value": 52, "stale": false},
                "vigorousMinutesAvg": {"value": 89.3, "stale": false, "numOfWeeksForIm": 6},
                "bmi": {"value": 25.0, "targetValue": 21.1, "improvementValue": 3.9,
                        "potentialAge": 30.044528789489956, "priority": 1, "stale": false,
                        "lastMeasurementDate": "2024-11-11"}
            },
            "lastUpdated": "2025-01-01T00:00:00.0"
        }));
        assert_eq!(
            out,
            serde_json::json!({
                "last_updated": "2025-01-01",
                "chronological_age": 35,
                "fitness_age": 30.8,
                "previous_fitness_age": 30.8,
                "achievable_fitness_age": 29.3,
                "components": {
                    "vigorous_days_per_week": {"value": 2.5, "target_value": 3.0,
                        "potential_fitness_age": 31.6, "priority": 2, "averaged_over_weeks": 6},
                    "vigorous_minutes_per_week": {"value": 89.3, "averaged_over_weeks": 6},
                    "resting_hr_bpm": {"value": 52.0},
                    "bmi": {"value": 25.0, "target_value": 21.1, "improvement": 3.9,
                        "potential_fitness_age": 30.0, "priority": 1,
                        "last_measurement_date": "2024-11-11"}
                }
            })
        );
    }

    #[test]
    fn body_fat_replaces_bmi() {
        let out = summary(serde_json::json!({
            "components": {"bodyFat": {"value": 17.0, "stale": false, "lastMeasurementDate": "2026-10-17"}}
        }));
        assert_eq!(
            out["components"],
            serde_json::json!({"body_fat_percent": {"value": 17.0, "last_measurement_date": "2026-10-17"}})
        );
    }

    #[test]
    fn day_without_fitness_age_shows_missing_inputs() {
        // Real response for a date before the user had data.
        let out = summary(serde_json::json!({
            "chronologicalAge": 25,
            "components": {
                "rhr": {"stale": true, "staleInputs": ["RHR"]},
                "bmi": {"stale": true, "staleInputs": ["WEIGHT"]}
            },
            "lastUpdated": "2015-01-01T00:00:00.0"
        }));
        assert_eq!(
            out,
            serde_json::json!({
                "last_updated": "2015-01-01",
                "chronological_age": 25,
                "components": {
                    "resting_hr_bpm": {"stale": true, "stale_inputs": ["RHR"]},
                    "bmi": {"stale": true, "stale_inputs": ["WEIGHT"]}
                }
            })
        );
    }
}
