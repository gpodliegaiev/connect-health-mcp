//! Types for the user's heart rate zones
//! (`GET /biometric-service/heartRateZones/v2`).
//!
//! Garmin keeps one zone set per sport (`DEFAULT`, and optionally
//! `RUNNING`, `CYCLING`, `SWIMMING`, `ROWING`), each with five zones and the
//! values they were calculated from (max HR, resting HR, lactate threshold
//! HR). This tells an AI what the `hr_zones_min` of an activity mean in bpm.
//!
//! Same two-step pattern as the other response types: [`HeartRateZones`]
//! mirrors one entry of Garmin's list, and `into_summary()` turns it into
//! [`ZoneSet`] with each zone as a `[low, high]` bpm range.
//!
//! Found by probing: an account with only default zones returns a list with
//! one `DEFAULT` entry; `generateDefaults=false` made no difference, so the
//! param is not sent.
//!
//! Nothing is left out: every field Garmin sends is about the zones.

use serde::{Deserialize, Serialize};

use super::types::only_true;

/// One sport's zone configuration, as it arrives.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeartRateZones {
    /// `DEFAULT`, `RUNNING`, `CYCLING`, `SWIMMING` or `ROWING`.
    sport: Option<String>,
    /// How the zones are calculated: `HR_MAX`, `LACTATE_THRESHOLD`,
    /// `HR_RESERVE` or `CUSTOM`.
    training_method: Option<String>,
    /// Lower limit of each zone, bpm. Each zone ends 1 bpm below the next
    /// one's floor; zone 5 ends at the max HR.
    zone1_floor: Option<u32>,
    zone2_floor: Option<u32>,
    zone3_floor: Option<u32>,
    zone4_floor: Option<u32>,
    zone5_floor: Option<u32>,
    max_heart_rate_used: Option<u32>,
    resting_heart_rate_used: Option<u32>,
    lactate_threshold_heart_rate_used: Option<u32>,
    /// Whether the resting HR follows the watch's daily measurement.
    resting_hr_auto_update_used: Option<bool>,
}

/// One sport's zones, as the `get_heart_rate_zones` tool returns them.
#[derive(Debug, Serialize)]
pub struct ZoneSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub training_method: Option<String>,
    /// Each zone as `[low, high]` bpm, both inclusive.
    pub zones_bpm: ZoneRanges,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hr: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lactate_threshold_hr: Option<u32>,
    /// Only output when `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resting_hr_auto_update: Option<bool>,
}

/// The five zones. Same key names as `hr_zones_min` in activities.
#[derive(Debug, Serialize)]
pub struct ZoneRanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_1: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_2: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_3: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_4: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_5: Option<[u32; 2]>,
}

impl HeartRateZones {
    /// Converts one sport's zones. Takes `self` by value, so the strings are
    /// moved into the output instead of copied.
    pub fn into_summary(self) -> ZoneSet {
        // A zone's upper limit is 1 bpm below the next floor; for zone 5 it
        // is the max HR. `zip` gives `Some((low, next))` only if both exist.
        let range = |low: Option<u32>, next_floor: Option<u32>| {
            low.zip(next_floor)
                .map(|(low, next)| [low, next.saturating_sub(1)])
        };
        let max = self.max_heart_rate_used;
        ZoneSet {
            sport: self.sport,
            training_method: self.training_method,
            zones_bpm: ZoneRanges {
                zone_1: range(self.zone1_floor, self.zone2_floor),
                zone_2: range(self.zone2_floor, self.zone3_floor),
                zone_3: range(self.zone3_floor, self.zone4_floor),
                zone_4: range(self.zone4_floor, self.zone5_floor),
                // `max + 1` so that `range` ends zone 5 exactly at max HR.
                zone_5: range(self.zone5_floor, max.map(|m| m + 1)),
            },
            max_hr: max,
            resting_hr: self.resting_heart_rate_used,
            lactate_threshold_hr: self.lactate_threshold_heart_rate_used,
            resting_hr_auto_update: only_true(self.resting_hr_auto_update_used),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floors_become_ranges() {
        // Shape of a real response entry (values made up).
        let raw: HeartRateZones = serde_json::from_value(serde_json::json!({
            "trainingMethod": "HR_MAX", "restingHeartRateUsed": 55,
            "lactateThresholdHeartRateUsed": 160, "zone1Floor": 90, "zone2Floor": 108,
            "zone3Floor": 126, "zone4Floor": 144, "zone5Floor": 162, "maxHeartRateUsed": 180,
            "restingHrAutoUpdateUsed": true, "sport": "DEFAULT"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(raw.into_summary()).unwrap(),
            serde_json::json!({
                "sport": "DEFAULT", "training_method": "HR_MAX",
                "zones_bpm": {"zone_1": [90, 107], "zone_2": [108, 125], "zone_3": [126, 143],
                              "zone_4": [144, 161], "zone_5": [162, 180]},
                "max_hr": 180, "resting_hr": 55, "lactate_threshold_hr": 160,
                "resting_hr_auto_update": true
            })
        );
    }
}
