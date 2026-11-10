//! Types for weight and body composition
//! (`GET /weight-service/weight/range/{startDate}/{endDate}?includeAll=true`)
//! and the weight goal
//! (`GET /goal-service/goal/user/effective/weightgoal/{startDate}/{endDate}`).
//!
//! Same two-step pattern as the other response types: [`WeightRange`] and
//! [`WeightGoals`] mirror Garmin's responses, and [`WeightRange::into_summary`]
//! merges both into the [`WeightReport`] the tool returns.
//!
//! Found by probing (the spec differs or is silent):
//! - Weights and masses are in **grams** (the spec says kg for `weight`):
//!   `86860.0` is 86.86 kg. `weightDelta` is grams too, the difference of the
//!   two weights rounded to 0.1 kg first, so it is off by up to 0.1 kg.
//! - `date` is epoch milliseconds already shifted to local time (like the
//!   sleep `...TimestampLocal` fields), so it is formatted as UTC.
//! - Days are newest first. Only days with a weigh-in are listed; a range
//!   without one has `dailyWeightSummaries: []` and an all-`null` average.
//! - `latestWeight.weightDelta` compares with the latest weigh-in of an
//!   earlier day, while each entry of `allWeightMetrics` (sent only with
//!   `includeAll=true`, also newest first) compares with the weigh-in just
//!   before it, even on the same day.
//! - `previousDateWeight` / `nextDateWeight` are the closest weigh-ins
//!   outside the range: the trend context for a short range.
//! - The range can span at least 10 years; reversed dates give 400 `{}`, a
//!   malformed date 404 `{}`.
//! - `/weight-service/weight/all/{startDate}/{endDate}` answered 400 `{}` to
//!   every range tried, so `range?includeAll=true` is used for every entry.
//! - The goal endpoint ignores its dates: it returned the same goal (start
//!   2026-02-09) for a 2025 range and for reversed dates.
//!
//! Left out: `samplePk` (entry id), `timestampGMT` (GMT copy of `date`),
//! `minWeight` / `maxWeight` (the day's entries are all listed),
//! `totalAverage.from` / `until` (the requested range), and the goals'
//! `goalName` (always `null`), `userGoalType` (always `WEIGHT_GRAMS`),
//! `createDate` / `updateDate` (when the goal was edited).

use serde::{Deserialize, Serialize};

use super::types::{positive, round, rounded};

/// Garmin's weight range response, as it arrives.
///
/// `default` on the struct lets any missing field fall back to `None`.
/// Lists are `Option<Vec<..>>` because Garmin may send `null` for them, and
/// `#[serde(default)]` only covers a *missing* field, not an explicit `null`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WeightRange {
    /// One entry per day with at least one weigh-in, newest first.
    daily_weight_summaries: Option<Vec<RawDay>>,
    /// Averages over the whole range (all `null` when there is no weigh-in).
    total_average: Option<RawWeight>,
    /// Last weigh-in before the range.
    previous_date_weight: Option<RawWeight>,
    /// First weigh-in after the range.
    next_date_weight: Option<RawWeight>,
}

/// One day of weigh-ins, as it arrives.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawDay {
    /// The day, "YYYY-MM-DD".
    summary_date: Option<String>,
    /// The day's last weigh-in: Garmin's value for the day.
    latest_weight: Option<RawWeight>,
    /// Every weigh-in of the day, newest first.
    all_weight_metrics: Option<Vec<RawWeight>>,
}

/// One weigh-in (or the range average), as it arrives. Masses in grams.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawWeight {
    /// Local time as epoch milliseconds (already shifted to local time).
    date: Option<i64>,
    /// The day, "YYYY-MM-DD".
    calendar_date: Option<String>,
    /// Body weight, grams.
    weight: Option<f64>,
    /// Body mass index (float32 noise, e.g. `25.700000762939453`).
    bmi: Option<f64>,
    /// Body fat, percent.
    body_fat: Option<f64>,
    /// Body water, percent.
    body_water: Option<f64>,
    /// Bone mass, grams.
    bone_mass: Option<f64>,
    /// Muscle mass, grams.
    muscle_mass: Option<f64>,
    /// Physique rating (1–9 on scales that report it).
    physique_rating: Option<f64>,
    /// Visceral fat rating (scales that report it).
    visceral_fat: Option<f64>,
    /// Metabolic age, years (scales that report it).
    metabolic_age: Option<f64>,
    /// Where the value came from, e.g. `INDEX_SCALE`, `MANUAL`.
    source_type: Option<String>,
    /// Change from the previous weigh-in, grams (see the module doc).
    weight_delta: Option<f64>,
}

/// Garmin's weight goal response, as it arrives.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WeightGoals {
    /// Target weights.
    user_goals: Option<Vec<RawGoal>>,
    /// Target weight ranges (e.g. weekly ranges of a pregnancy plan).
    user_goal_ranges: Option<Vec<RawGoalRange>>,
}

/// One target weight, as it arrives.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawGoal {
    /// Who set it: `MANUAL`, `MY_AUTO`, `MYFITNESSPAL`, `PREGNANCY`, ...
    user_goal_category: Option<String>,
    /// First day the goal applies, "YYYY-MM-DD".
    start_date: Option<String>,
    /// Last day, when the goal has ended.
    end_date: Option<String>,
    /// Target weight, grams.
    goal_value: Option<f64>,
    /// `DAILY`, `SEVEN_DAYS`, ...
    tracking_period_type: Option<String>,
}

/// One target weight range, as it arrives (from the spec; not seen yet).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawGoalRange {
    user_goal_category: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    low_weight_goal_in_grams: Option<f64>,
    high_weight_goal_in_grams: Option<f64>,
    /// Week of a sequential plan (e.g. pregnancy week).
    week_num: Option<u32>,
    tracking_period_type: Option<String>,
}

/// What the `get_weight` tool returns.
#[derive(Debug, Serialize)]
pub struct WeightReport {
    /// Target weights.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<WeightGoal>,
    /// Target weight ranges.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub goal_ranges: Vec<WeightGoalRange>,
    /// Averages over the requested range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_average: Option<Body>,
    /// Days with a weigh-in, newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<WeightDay>,
    /// Last weigh-in before the range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_range: Option<Body>,
    /// First weigh-in after the range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_range: Option<Body>,
}

impl WeightReport {
    /// `true` when Garmin has nothing for the range: no weigh-in inside it,
    /// none around it, and no goal.
    pub fn is_empty(&self) -> bool {
        self.goals.is_empty()
            && self.goal_ranges.is_empty()
            && self.days.is_empty()
            && self.before_range.is_none()
            && self.after_range.is_none()
    }
}

/// One day's weigh-ins.
#[derive(Debug, Serialize)]
pub struct WeightDay {
    /// The day's last weigh-in, which Garmin uses as the day's value.
    /// `#[serde(flatten)]` writes its fields directly into the day object
    /// instead of nesting them under a `latest` key.
    #[serde(flatten)]
    pub latest: Body,
    /// Every weigh-in of the day, newest first; only when there were
    /// several.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub measurements: Vec<Body>,
}

/// One weigh-in or an average, with units in the names.
#[derive(Debug, Default, Serialize)]
pub struct Body {
    /// Day, "YYYY-MM-DD" (left out inside `measurements`, where the day is
    /// already known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Local time of the weigh-in, "HH:MM".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_kg: Option<f64>,
    /// Change from the previous weigh-in (the previous day's for a day,
    /// the one just before for an entry in `measurements`), kg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bmi: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_fat_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_water_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muscle_mass_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bone_mass_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visceral_fat_rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physique_rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metabolic_age_years: Option<f64>,
    /// Where the value came from, e.g. `INDEX_SCALE`, `MANUAL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A target weight.
#[derive(Debug, Serialize)]
pub struct WeightGoal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_weight_kg: Option<f64>,
    /// First day the goal applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// Last day, if the goal has ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// Who set it, e.g. `MANUAL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
    /// e.g. `DAILY`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking_period: Option<String>,
}

/// A target weight range.
#[derive(Debug, Serialize)]
pub struct WeightGoalRange {
    /// `[low, high]` kg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_kg: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// Week of a sequential plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking_period: Option<String>,
}

impl WeightRange {
    /// Converts the weight range and the goals into the tool's output.
    ///
    /// Takes both by value (`self`, `goals`): they are used up, so their
    /// strings can be moved into the output without copying.
    pub fn into_summary(self, goals: WeightGoals) -> WeightReport {
        // `unwrap_or_default()` turns a `null` list into an empty one.
        let days = self
            .daily_weight_summaries
            .unwrap_or_default()
            .into_iter()
            // `filter_map` keeps only the days that produce `Some(..)`.
            .filter_map(RawDay::into_day)
            .collect();

        // An average without a weight means the range had no weigh-in.
        // `Option::filter` keeps the value only if the check passes.
        let range_average = self
            .total_average
            .map(RawWeight::into_body)
            .filter(|b| b.weight_kg.is_some());

        WeightReport {
            goals: goals
                .user_goals
                .unwrap_or_default()
                .into_iter()
                .map(|g| WeightGoal {
                    target_weight_kg: kg(g.goal_value),
                    start_date: g.start_date,
                    end_date: g.end_date,
                    set_by: g.user_goal_category,
                    tracking_period: g.tracking_period_type,
                })
                .collect(),
            goal_ranges: goals
                .user_goal_ranges
                .unwrap_or_default()
                .into_iter()
                .map(|r| WeightGoalRange {
                    // `zip` gives `Some((low, high))` only if both are there.
                    weight_kg: kg(r.low_weight_goal_in_grams)
                        .zip(kg(r.high_weight_goal_in_grams))
                        .map(|(low, high)| [low, high]),
                    start_date: r.start_date,
                    end_date: r.end_date,
                    week: r.week_num,
                    set_by: r.user_goal_category,
                    tracking_period: r.tracking_period_type,
                })
                .collect(),
            range_average,
            days,
            before_range: self.previous_date_weight.map(RawWeight::into_body),
            after_range: self.next_date_weight.map(RawWeight::into_body),
        }
    }
}

impl RawDay {
    /// Converts one day; `None` if it has no weigh-in.
    fn into_day(self) -> Option<WeightDay> {
        // `?` on an `Option` returns `None` from this function if it is
        // `None`, and otherwise unwraps the value.
        let mut latest = self.latest_weight?.into_body();
        // Prefer the day Garmin filed the summary under.
        latest.date = self.summary_date.or(latest.date);

        let all = self.all_weight_metrics.unwrap_or_default();
        // A single entry is the same weigh-in as `latest`, so list the
        // entries only when there were several.
        let measurements = if all.len() > 1 {
            all.into_iter()
                .map(|w| Body {
                    // The day is already on the day object.
                    date: None,
                    ..w.into_body()
                })
                .collect()
        } else {
            Vec::new()
        };
        Some(WeightDay {
            latest,
            measurements,
        })
    }
}

impl RawWeight {
    /// Converts grams to kg and drops float32 noise.
    fn into_body(self) -> Body {
        // `date` is already local time, so formatting it as UTC gives the
        // local clock time; the day comes from `calendarDate`.
        let local = self.date.and_then(|millis| {
            chrono::DateTime::from_timestamp_millis(millis)
                .map(|dt| dt.naive_utc().format("%H:%M").to_string())
        });
        Body {
            date: self.calendar_date,
            time_local: local,
            // A scale weighs to 10 g or 50 g, so 2 decimals keep everything.
            weight_kg: kg(self.weight),
            // `+ 0.0` turns `-0.0` (a tiny loss rounded to 0) into `0.0`.
            // A 0 change is a real result, so it is not filtered.
            change_kg: self.weight_delta.map(|d| round(d / 1000.0, 1) + 0.0),
            bmi: rounded(self.bmi, 1),
            body_fat_percent: rounded(self.body_fat, 1),
            body_water_percent: rounded(self.body_water, 1),
            muscle_mass_kg: kg(self.muscle_mass),
            bone_mass_kg: kg(self.bone_mass),
            visceral_fat_rating: rounded(self.visceral_fat, 1),
            physique_rating: rounded(self.physique_rating, 1),
            metabolic_age_years: rounded(self.metabolic_age, 1),
            source: self.source_type,
        }
    }
}

/// Grams to kg with 2 decimals; `0` → `None`.
fn kg(grams: Option<f64>) -> Option<f64> {
    positive(grams).map(|g| round(g / 1000.0, 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real response (values made up): a day with two
    /// weigh-ins and context around.
    fn report() -> serde_json::Value {
        let range: WeightRange = serde_json::from_value(serde_json::json!({
            "dailyWeightSummaries": [{
                "summaryDate": "2026-10-05", "numOfWeightEntries": 2,
                "minWeight": 74500.0, "maxWeight": 75412.0,
                "latestWeight": {
                    "samplePk": 1, "date": 1791235682000_i64, "calendarDate": "2026-10-05",
                    "weight": 74500.0, "bmi": 23.399999618530273, "bodyFat": 18.2,
                    "bodyWater": 57.3, "boneMass": 3108, "muscleMass": 31450,
                    "physiqueRating": null, "visceralFat": null, "metabolicAge": null,
                    "sourceType": "INDEX_SCALE", "timestampGMT": 1791228482000_i64,
                    "weightDelta": -700.0
                },
                "allWeightMetrics": [
                    {"date": 1791235682000_i64, "calendarDate": "2026-10-05",
                     "weight": 74500.0, "sourceType": "INDEX_SCALE",
                     "weightDelta": -899.9999999999915},
                    {"date": 1791235471000_i64, "calendarDate": "2026-10-05",
                     "weight": 75412.0, "sourceType": "INDEX_SCALE",
                     "weightDelta": 600.0000000000085}
                ]
            }],
            "totalAverage": {"from": 1, "until": 2, "weight": 74500.0,
                             "bmi": 23.399999618530273, "boneMass": 3108},
            "previousDateWeight": {"date": 1788514900000_i64, "calendarDate": "2026-09-04",
                                   "weight": 75200.0, "weightDelta": -300.0},
            "nextDateWeight": null
        }))
        .unwrap();
        let goals: WeightGoals = serde_json::from_value(serde_json::json!({
            "userGoals": [{"userGoalCategory": "MANUAL", "userGoalType": "WEIGHT_GRAMS",
                           "startDate": "2026-02-09", "endDate": null, "goalName": null,
                           "goalValue": 72000.0, "trackingPeriodType": "DAILY"}],
            "userGoalRanges": null
        }))
        .unwrap();
        serde_json::to_value(range.into_summary(goals)).unwrap()
    }

    #[test]
    fn converts_grams_and_local_times() {
        assert_eq!(
            report(),
            serde_json::json!({
                "goals": [{"target_weight_kg": 72.0, "start_date": "2026-02-09",
                           "set_by": "MANUAL", "tracking_period": "DAILY"}],
                "range_average": {"weight_kg": 74.5, "bmi": 23.4, "bone_mass_kg": 3.11},
                "days": [{
                    "date": "2026-10-05", "time_local": "21:28", "weight_kg": 74.5,
                    "change_kg": -0.7, "bmi": 23.4, "body_fat_percent": 18.2,
                    "body_water_percent": 57.3, "muscle_mass_kg": 31.45,
                    "bone_mass_kg": 3.11, "source": "INDEX_SCALE",
                    "measurements": [
                        {"time_local": "21:28", "weight_kg": 74.5, "change_kg": -0.9,
                         "source": "INDEX_SCALE"},
                        {"time_local": "21:24", "weight_kg": 75.41, "change_kg": 0.6,
                         "source": "INDEX_SCALE"}
                    ]
                }],
                "before_range": {"date": "2026-09-04", "time_local": "09:41",
                                 "weight_kg": 75.2, "change_kg": -0.3}
            })
        );
    }

    #[test]
    fn range_without_weigh_ins_is_empty() {
        let range: WeightRange = serde_json::from_value(serde_json::json!({
            "dailyWeightSummaries": [],
            "totalAverage": {"from": 1, "until": 2, "weight": null, "bmi": null},
            "previousDateWeight": null, "nextDateWeight": null
        }))
        .unwrap();
        let report = range.into_summary(WeightGoals::default());
        assert!(report.is_empty());
        assert!(report.range_average.is_none());
    }
}
