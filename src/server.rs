//! The MCP server itself: which tools it offers and how it describes itself.
//!
//! An MCP "tool" is a function the AI client can call, with named, typed
//! arguments. The `rmcp` crate provides macros that turn ordinary Rust
//! methods into tools, generating the JSON schema and request routing for us.

// `Arc` ("atomically reference-counted") is a shared pointer: cloning it
// gives another handle to the *same* value instead of copying the value.
use std::sync::Arc;

// Import the pieces of the `rmcp` crate we use. Nested braces let us import
// many names from one crate in a single `use` statement.
use rmcp::{
    // Trait every MCP server must implement (see the bottom of this file).
    // `ErrorData` is a protocol-level error; `RoleServer` marks the server
    // side of a connection.
    ErrorData,
    RoleServer,
    ServerHandler,
    // `ToolRouter` maps an incoming tool name (e.g. "list_activities") to the Rust
    // method that handles it. `Parameters` wraps a tool's input arguments.
    // `ToolCallContext` bundles one call's request for the router.
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    // Types used to describe the server to the client during the handshake,
    // and the request and result of a tool call (used by `call_tool`).
    model::{
        CallToolRequestParams, CallToolResponse, ContentBlock, Implementation, ServerCapabilities,
        ServerConfig,
    },
    // `schemars` generates JSON Schema from Rust types. Clients use that
    // schema to know which arguments a tool accepts.
    schemars::{self, JsonSchema},
    // Per-request information passed to handlers by rmcp.
    service::RequestContext,
    // The macros that generate tool plumbing (explained where they're used).
    tool,
    tool_handler,
    tool_router,
};
// `NaiveDate` is a calendar date without a time zone; `Local` reads this
// computer's clock and time zone (used for "today").
use chrono::{Days, Local, NaiveDate};
// `Deserialize` lets serde build a Rust struct from JSON.
use serde::Deserialize;

use crate::{
    garmin::{GarminClient, client::ActivitiesQuery, types::ActivityTypeFilter},
    update::UpdateNotice,
};

/// Largest `limit` accepted by `list_activities`, so one call cannot pull a
/// huge amount of data into the AI's context.
const MAX_ACTIVITIES: u32 = 100;

/// Input arguments for the `list_activities` tool.
///
/// The `#[derive(...)]` line asks the compiler to automatically write code
/// (implement traits) for this struct:
/// - `Debug` lets it be printed with `{:?}`, useful for debugging.
/// - `Deserialize` lets it be created from the JSON the client sends,
///   e.g. `{"start": 0, "limit": 5}`.
/// - `JsonSchema` produces a JSON Schema describing the fields. The `///`
///   comment on each field becomes that field's description in the schema,
///   so the AI client sees it.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListActivitiesRequest {
    /// Number of most recent activities to skip (for paging). Default 0.
    // `#[serde(default)]` makes the field optional in the JSON: if it is
    // missing, serde uses the type's default value (0 for numbers).
    // `u32` is an unsigned 32-bit integer, so negative values are rejected.
    #[serde(default)]
    pub start: u32,
    /// How many activities to return, newest first. Default 5, maximum 100.
    // `default = "default_limit"` names a function that supplies the value
    // when the field is missing.
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Only return activities of this sport group. Omit for all activities.
    /// Groups include their subtypes: `running` includes treadmill runs, and
    /// strength training is under `fitness_equipment`, floor climbing under
    /// `other`.
    // Field name as the client sends it: `activity_type`. Any value not in
    // the `ActivityTypeFilter` enum makes the call fail with a clear message.
    #[serde(default)]
    pub activity_type: Option<ActivityTypeFilter>,
    /// Only activities that started on or after this local date, as
    /// YYYY-MM-DD. Raise `limit` to get every activity in a date range.
    #[serde(default)]
    pub start_date: Option<String>,
    /// Only activities that started on or before this local date, as
    /// YYYY-MM-DD (inclusive).
    #[serde(default)]
    pub end_date: Option<String>,
    /// Minimum distance in km (inclusive), e.g. 10 for runs of 10 km or
    /// more. Also in km for swimming: 1.5 means 1500 m.
    // `f64` is a 64-bit decimal number, so values like 5.5 are accepted.
    #[serde(default)]
    pub min_distance_km: Option<f64>,
    /// Maximum distance in km (inclusive).
    #[serde(default)]
    pub max_distance_km: Option<f64>,
    /// Minimum duration in minutes (inclusive), not counting pauses. This is
    /// the `duration_min` field of the result.
    #[serde(default)]
    pub min_duration_min: Option<f64>,
    /// Maximum duration in minutes (inclusive), not counting pauses.
    #[serde(default)]
    pub max_duration_min: Option<f64>,
    /// Minimum total elevation gain in meters (inclusive).
    // `u32` cannot be negative, so serde already rejects e.g. -5.
    #[serde(default)]
    pub min_elevation_gain_m: Option<u32>,
    /// Maximum total elevation gain in meters (inclusive).
    #[serde(default)]
    pub max_elevation_gain_m: Option<u32>,
}

/// Default for `ListActivitiesRequest::limit`.
fn default_limit() -> u32 {
    5
}

impl ListActivitiesRequest {
    /// Checks the tool arguments and converts them into Garmin's query
    /// parameters: dates parsed, km → meters, minutes → seconds.
    ///
    /// Takes `self` by value (no `&`): the request is used up, since it's
    /// not needed once converted. Returns `Err(message)` for input the AI
    /// should fix, e.g. a bad date or a minimum above the maximum.
    fn into_query(self) -> Result<ActivitiesQuery, String> {
        // `Option::map` runs the closure only when there is a value:
        // `Some(text)` becomes `Some(parse_date(text))`, `None` stays `None`.
        // `transpose` turns that `Option<Result<..>>` into
        // `Result<Option<..>>`, so `?` can return a parse error early.
        let start_date = self.start_date.as_deref().map(parse_date).transpose()?;
        let end_date = self.end_date.as_deref().map(parse_date).transpose()?;

        // Garmin answers a reversed range with an empty list, which would look
        // like "no activities". Reject it so the AI can correct the call.
        // Checked on the tool's own values, so the message shows the units
        // the AI used (km, minutes), not Garmin's.
        check_range("start_date", start_date, "end_date", end_date)?;
        check_range(
            "min_distance_km",
            self.min_distance_km,
            "max_distance_km",
            self.max_distance_km,
        )?;
        check_range(
            "min_duration_min",
            self.min_duration_min,
            "max_duration_min",
            self.max_duration_min,
        )?;
        check_range(
            "min_elevation_gain_m",
            self.min_elevation_gain_m,
            "max_elevation_gain_m",
            self.max_elevation_gain_m,
        )?;

        Ok(ActivitiesQuery {
            start: self.start,
            limit: self.limit.clamp(1, MAX_ACTIVITIES),
            activity_type: self.activity_type,
            start_date,
            end_date,
            min_distance: scaled("min_distance_km", self.min_distance_km, 1000.0)?,
            max_distance: scaled("max_distance_km", self.max_distance_km, 1000.0)?,
            min_duration: scaled("min_duration_min", self.min_duration_min, 60.0)?,
            max_duration: scaled("max_duration_min", self.max_duration_min, 60.0)?,
            // `u64::from` widens the `u32` losslessly to the query's type.
            min_elevation: self.min_elevation_gain_m.map(u64::from),
            max_elevation: self.max_elevation_gain_m.map(u64::from),
        })
    }
}

/// Parses a `YYYY-MM-DD` tool argument (surrounding spaces allowed).
fn parse_date(text: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .map_err(|_| format!("invalid date {text:?}: expected YYYY-MM-DD"))
}

/// Works out a date range from optional `start_date` / `end_date` tool
/// arguments.
///
/// - `end` defaults to today (this computer's time zone).
/// - `start` defaults to `default_days` days ending on `end` (7 → a week).
/// - At most `max_days` days apart (Garmin's limit for the endpoint), and
///   `start` must not be after `end`; Garmin answers both with a bare 400.
fn date_range(
    start: Option<&str>,
    end: Option<&str>,
    default_days: u64,
    max_days: u64,
) -> Result<(NaiveDate, NaiveDate), String> {
    // `.map(parse_date)` gives `Option<Result<..>>`; `.transpose()` flips it
    // to `Result<Option<..>>`, so `?` can return a bad-date error.
    let end = end
        .map(parse_date)
        .transpose()?
        .unwrap_or_else(|| Local::now().date_naive());
    let start = match start {
        Some(text) => parse_date(text)?,
        // `checked_sub_days` returns `None` only before year -262143, so
        // falling back to `end` never happens in practice.
        None => end
            .checked_sub_days(Days::new(default_days - 1))
            .unwrap_or(end),
    };
    check_range("start_date", Some(start), "end_date", Some(end))?;
    // Subtracting two dates gives a `TimeDelta`; `num_days` is whole days.
    let days_apart = (end - start).num_days();
    if days_apart > max_days as i64 {
        return Err(format!(
            "start_date and end_date are {days_apart} days apart; the maximum is {max_days}"
        ));
    }
    Ok((start, end))
}

/// Converts an optional tool value to Garmin's whole units, e.g. km → m
/// with `factor` 1000. Garmin rejects decimals, so the result is rounded.
/// `name` is the argument name, used in the error message.
fn scaled(name: &str, value: Option<f64>, factor: f64) -> Result<Option<u64>, String> {
    match value {
        None => Ok(None),
        // A `match` arm can have a guard (`if ...`): this arm is taken only
        // for negative values.
        Some(v) if v < 0.0 => Err(format!("{name} must not be negative, got {v}")),
        // `as u64` converts the rounded decimal to a whole number.
        Some(v) => Ok(Some((v * factor).round() as u64)),
    }
}

/// Fails if both bounds are given and `min` is greater than `max`.
///
/// `<T: PartialOrd + Debug>` makes this work for any type that can be
/// compared with `>` and printed with `{:?}`: dates and numbers here.
fn check_range<T: PartialOrd + std::fmt::Debug>(
    min_name: &str,
    min: Option<T>,
    max_name: &str,
    max: Option<T>,
) -> Result<(), String> {
    // `if let` matches a single pattern: here, only when both are `Some`.
    // `&& min > max` adds a second condition (a "let chain", Rust 2024).
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(format!(
            "{min_name} ({min:?}) is greater than {max_name} ({max:?})"
        ));
    }
    Ok(())
}

/// Input arguments for the `get_activity` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetActivityRequest {
    /// Id of the activity, as returned in `activity_id` by `list_activities`.
    // No `#[serde(default)]`: the argument is required, and a call without
    // it is rejected before the tool runs. `u64` fits Garmin's 11-digit ids.
    pub activity_id: u64,
}

/// Input arguments for the `get_sleep` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSleepRequest {
    /// Day to get sleep for, as YYYY-MM-DD. Garmin files a night under the
    /// day you woke up, so last night's sleep is today's date. Defaults to
    /// today.
    // `Option<String>` makes the argument optional: missing or `null` in the
    // JSON becomes `None`. (`#[serde(default)]` lets it be left out entirely.)
    #[serde(default)]
    pub date: Option<String>,
    /// First night of a range, YYYY-MM-DD (use instead of `date`). Defaults
    /// to 6 days before end_date (a week).
    #[serde(default)]
    pub start_date: Option<String>,
    /// Last night of a range, YYYY-MM-DD, inclusive (use instead of `date`).
    /// Defaults to today. At most 31 days after start_date.
    #[serde(default)]
    pub end_date: Option<String>,
}

/// Input arguments for the `get_fitness_age` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFitnessAgeRequest {
    /// Day to get Fitness Age for, as YYYY-MM-DD. Returns the latest values
    /// on or before that day. Defaults to today.
    #[serde(default)]
    pub date: Option<String>,
}

/// Input arguments for the `get_hrv` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetHrvRequest {
    /// First day, YYYY-MM-DD. Defaults to 6 days before end_date (a week).
    #[serde(default)]
    pub start_date: Option<String>,
    /// Last day, YYYY-MM-DD, inclusive. Defaults to today. At most 366 days
    /// after start_date. When start_date equals end_date, the night's
    /// 5-minute readings are included.
    #[serde(default)]
    pub end_date: Option<String>,
}

/// Input arguments for the `get_body_battery` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetBodyBatteryRequest {
    /// First day, YYYY-MM-DD. Defaults to 6 days before end_date (a week).
    #[serde(default)]
    pub start_date: Option<String>,
    /// Last day, YYYY-MM-DD, inclusive. Defaults to today. At most 30 days
    /// after start_date (31 days in all).
    #[serde(default)]
    pub end_date: Option<String>,
    /// Number of equal intervals per day, 1 to 96; each gives its lowest
    /// and highest level with the time. Defaults to 12 (2-hour intervals);
    /// 96 gives 15-minute detail, 1 only the day's low and high.
    #[serde(default)]
    pub intervals_per_day: Option<u32>,
}

/// Input arguments for the `get_heart_rate_day` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetHeartRateDayRequest {
    /// Day, YYYY-MM-DD. Defaults to today.
    #[serde(default)]
    pub date: Option<String>,
    /// Minutes per interval, 2 to 60. Defaults to 15. 2 returns every
    /// reading as it is (the watch records one every 2 minutes); larger
    /// values give each interval's average, lowest and highest.
    #[serde(default)]
    pub interval_min: Option<u32>,
}

/// Input arguments for the `get_weight` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetWeightRequest {
    /// First day, YYYY-MM-DD. Defaults to 29 days before end_date (30 days).
    #[serde(default)]
    pub start_date: Option<String>,
    /// Last day, YYYY-MM-DD, inclusive. Defaults to today. At most 366 days
    /// after start_date.
    #[serde(default)]
    pub end_date: Option<String>,
}

/// Input arguments for the `get_daily_summary` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetDailySummaryRequest {
    /// First day, YYYY-MM-DD. Defaults to 6 days before end_date (a week).
    #[serde(default)]
    pub start_date: Option<String>,
    /// Last day, YYYY-MM-DD, inclusive. Defaults to today. At most 31 days
    /// after start_date. When start_date equals end_date, a few extra fields
    /// are included (Body Battery at wake and during sleep, 7-day resting HR,
    /// absolute min/max HR, time per activity level, total distance, the
    /// activity behind each Body Battery event).
    #[serde(default)]
    pub end_date: Option<String>,
}

/// Our MCP server. Holds everything it needs to answer requests.
///
/// - `Clone` lets the value be duplicated; `rmcp` needs this because it may
///   hand copies of the server to different async tasks.
/// - `pub` makes the struct usable from `main.rs`.
#[derive(Clone)]
pub struct GarminServer {
    /// Garmin API client, shared by all copies of the server.
    // Wrapped in `Arc` so that cloning the server is cheap and every copy
    // uses the same client (and therefore the same cached token).
    garmin: Arc<GarminClient>,
    /// Note about a newer release, filled in by the background update check
    /// and added once to a tool result (see `update.rs` and `call_tool`).
    update: Arc<UpdateNotice>,
    /// Lookup table from tool name to handler method. Filled in by the
    /// `#[tool_router]` macro below via `Self::tool_router()`.
    // `ToolRouter<Self>` is a *generic* type: `Self` (here `GarminServer`)
    // tells the router which type its handler methods belong to.
    tool_router: ToolRouter<Self>,
}

// An `impl` block adds methods to a type. This one holds ordinary helpers.
impl GarminServer {
    /// Creates a new server with all tools registered.
    ///
    /// Rust has no constructors; by convention a function named `new` that
    /// returns `Self` plays that role. It takes ownership of `garmin` (no `&`),
    /// moving the client into the server. `update` is shared with the update
    /// check, which may fill it in later.
    pub fn new(garmin: GarminClient, update: Arc<UpdateNotice>) -> Self {
        // Build the struct. `Self::tool_router()` is generated by the
        // `#[tool_router]` macro and collects every `#[tool]` method below.
        // No trailing `;`, so this struct is the function's return value.
        Self {
            garmin: Arc::new(garmin),
            update,
            tool_router: Self::tool_router(),
        }
    }
}

// A second `impl` block, holding the tools. Rust allows several `impl` blocks
// for one type.
//
// `#[tool_router]` scans this block for methods marked `#[tool]` and
// generates a `tool_router()` function that registers all of them. To add a
// new tool, add another `#[tool]` method here.
#[tool_router]
impl GarminServer {
    /// Lists the user's most recent Garmin activities as a compact JSON summary.
    ///
    /// `#[tool(...)]` marks this method as an MCP tool. The tool's name is the
    /// method name (`list_activities`), and `description` is shown to the AI
    /// client so it knows when to call it.
    ///
    /// Parameters:
    /// - `&self` borrows the server (read-only access, no ownership taken).
    /// - `Parameters(req): Parameters<ListActivitiesRequest>` is
    ///   *destructuring*: `Parameters` is a wrapper struct, and this pattern
    ///   unpacks the request inside it straight into the variable `req`.
    ///
    /// Returns `Result<String, String>`: `Ok(json)` is sent as a normal
    /// result; `Err(message)` is sent as a tool error (`isError: true`), so
    /// the AI sees what went wrong instead of the whole call failing.
    #[tool(
        description = "List the user's Garmin Connect activities (runs, walks, strength sessions, etc.), newest first, with every metric Garmin recorded, for full activity evaluation. Optional filters (all inclusive, combinable): sport group (activity_type), local start date range (start_date, end_date as YYYY-MM-DD), distance in km, duration in minutes, elevation gain in m. Only `limit` activities are returned (default 5, max 100), so raise it to get everything in a range. Returns a JSON array, one object per activity. Each field name includes its unit (e.g. duration_min, distance_km, calories_kcal, stride_length_cm; heart rate in bpm). Speed is given the usual way for the sport: *_pace_per_km (\"m:ss\") for running, walking and hiking; *_pace_per_100m and distance_m for swimming; *_speed_kmh otherwise. Besides basics (time, distance, pace, elevation, heart rate, calories, training effect and load, body battery, intensity minutes) an activity can include: hr_zones_min and power_zones_min (minutes per zone; time below zone 1 is not counted), power (avg/max/normalized, power_curve_w best power per time window, FTP, TSS, intensity factor, pedal balance and force), cadence, running dynamics (stride length, ground contact time and balance, vertical oscillation and ratio, step speed loss), fastest_splits (best time per standard distance), grade-adjusted pace, lactate threshold, VO2 max, respiration, stress, sweat loss, temperature, swim, paddling, dive and mountain-bike metrics, strength exercises (sets, reps, max weight, volume per exercise), and splits (totals per split type, e.g. run/walk/stand or ski runs vs. lifts). Fields that don't apply or weren't recorded are omitted."
    )]
    async fn list_activities(
        &self,
        Parameters(req): Parameters<ListActivitiesRequest>,
    ) -> Result<String, String> {
        // Check the arguments and convert them to Garmin's units. A bad
        // argument returns its message to the AI here, via `?`.
        let query = req.into_query()?;

        // Call Garmin. `map_err` converts the `anyhow::Error` into a
        // `String`; `{e:#}` prints the error together with its context chain
        // (e.g. "GET ... failed: HTTP 500: ..."). `?` returns that `Err` early.
        let activities = self
            .garmin
            .list_activities(&query)
            .await
            .map_err(|e| format!("{e:#}"))?;

        // Turn the list of `Activity` structs into JSON text for the client.
        // Compact (no indentation) to keep the result small for the AI.
        serde_json::to_string(&activities).map_err(|e| e.to_string())
    }

    /// Returns the full details of one activity as compact JSON.
    #[tool(
        description = "Get details of one Garmin Connect activity by activity_id (from list_activities). Most metrics are already in list_activities; this adds the activity's time zone, min and recovery heart rate, average moving pace/speed, stamina at start and end, and heart rate per split type. Each field name includes its unit; fields that don't apply are omitted."
    )]
    async fn get_activity(
        &self,
        Parameters(req): Parameters<GetActivityRequest>,
    ) -> Result<String, String> {
        let detail = self
            .garmin
            .activity_detail(req.activity_id)
            .await
            .map_err(|e| format!("{e:#}"))?;
        serde_json::to_string(&detail).map_err(|e| e.to_string())
    }

    /// Returns one night's sleep summary as compact JSON, or a short message
    /// if Garmin has no sleep recorded for that day.
    #[tool(
        description = "Get the user's sleep for one night from Garmin Connect, with every metric Garmin recorded: bed and wake times (local), total and per-stage sleep (deep, light, REM, awake, unmeasurable) and naps, sleep score and rating, each score factor's rating with Garmin's optimal range (duration, deep/light/REM percent, stress, awake count, restlessness), heart rate, respiration (avg/min/max), SpO2 and breathing disruption when tracked, sleep stress, feedback and insight codes, sleep need (baseline, adjustments for training, HRV, naps, sleep history) and recommended bedtime for this and the next night, and sleep-timing alignment (optimal window and midpoints, local HH:MM). Garmin files a night under the day the user woke up, so last night's sleep is today's date. Pass `date` (default today) for one night, returned as one object; or start_date and/or end_date for a list of nights (default the last 7, at most 31 days apart; nights without data are left out). Durations are in minutes; ranges are [low, high]. Fields that weren't recorded are omitted."
    )]
    async fn get_sleep(
        &self,
        Parameters(req): Parameters<GetSleepRequest>,
    ) -> Result<String, String> {
        // A range was asked for: return a list of nights.
        if req.start_date.is_some() || req.end_date.is_some() {
            if req.date.is_some() {
                return Err("pass either date or start_date/end_date, not both".into());
            }
            let (start, end) =
                date_range(req.start_date.as_deref(), req.end_date.as_deref(), 7, 31)?;
            let nights = self
                .garmin
                .sleep_range(start, end)
                .await
                .map_err(|e| format!("{e:#}"))?;
            if nights.is_empty() {
                return Ok(format!("No sleep data recorded from {start} to {end}."));
            }
            return serde_json::to_string(&nights).map_err(|e| e.to_string());
        }

        // Otherwise one night. Work out which day to ask for.
        // `match` compares a value against patterns and runs the first arm
        // that fits. `Some(text)` binds the string inside to `text`.
        let date = match req.date {
            // Parse the text as a date; `?` returns a bad-date message early.
            Some(text) => parse_date(&text)?,
            // No date given: use today in this computer's time zone.
            None => Local::now().date_naive(),
        };

        let sleep = self
            .garmin
            .daily_sleep(date)
            .await
            .map_err(|e| format!("{e:#}"))?;

        // `Some(summary)`: send it as JSON. `None`: say there is no data. This
        // is a normal answer, not an error, so it is returned as `Ok`.
        match sleep {
            Some(summary) => serde_json::to_string(&summary).map_err(|e| e.to_string()),
            None => Ok(format!("No sleep data recorded for {date}.")),
        }
    }

    /// Returns the latest Fitness Age as of a day, as compact JSON.
    #[tool(
        description = "Get the user's Garmin Fitness Age: the latest values on or before a date (last_updated says which day), with chronological age, fitness age, previous fitness age (trend) and achievable fitness age (years, 1 decimal). Also each input to it: vigorous days and vigorous minutes per week (with the number of weeks averaged), resting heart rate, and BMI or body fat percent (with the last measurement date). When Garmin has a suggestion, an input also has target_value, improvement, potential_fitness_age (fitness age after reaching the target) and priority (1 = biggest gain). An input without recent data is marked stale with the missing inputs; if there is no fitness age at all, only chronological age and stale inputs are returned."
    )]
    async fn get_fitness_age(
        &self,
        Parameters(req): Parameters<GetFitnessAgeRequest>,
    ) -> Result<String, String> {
        // Same date handling as `get_sleep`: parse the argument, or use today.
        let date = match req.date {
            Some(text) => parse_date(&text)?,
            None => Local::now().date_naive(),
        };
        let fitness_age = self
            .garmin
            .fitness_age(date)
            .await
            .map_err(|e| format!("{e:#}"))?;
        serde_json::to_string(&fitness_age).map_err(|e| e.to_string())
    }

    /// Returns daily wellness summaries for a date range as compact JSON, or
    /// a short message if no day in the range has data.
    #[tool(
        description = "Get the user's daily wellness summaries from Garmin Connect, one entry per day: steps (and goal, distance), floors, calories (total, active, resting, consumed), resting heart rate and min/max 2-minute average HR, abnormal HR alerts, moderate/vigorous intensity minutes and weekly goal, stress (average, max, qualifier, minutes at rest/low/medium/high/activity), Body Battery (highest, lowest, charged, drained, latest, Garmin's feedback) with every event that charged or drained it (sleep, activity, recovery, stress, nap: local start, minutes, impact, feedback), waking respiration, SpO2 when tracked, average altitude, hydration when logged. A partial day (e.g. today) has tracked_hours and tracked_until_local. For a single day (start_date = end_date) it also has Body Battery at wake and charged during sleep, 7-day average resting HR, absolute min/max HR, minutes highly active/active/sedentary/sleeping, total distance incl. activities, and the activity behind each Body Battery event. Defaults to the last 7 days; start_date and end_date can be at most 31 days apart (each day is about 1.4 KB), so request longer periods in several calls. Days without data are omitted."
    )]
    async fn get_daily_summary(
        &self,
        Parameters(req): Parameters<GetDailySummaryRequest>,
    ) -> Result<String, String> {
        // Garmin allows 365 days apart, but a year of output is about
        // 500 KB, too much for an AI's context; 31 days is about 45 KB.
        let (start, end) = date_range(req.start_date.as_deref(), req.end_date.as_deref(), 7, 31)?;

        // One day: the single-day endpoint has more fields. Wrap its result
        // in a list so both cases have the same shape (`Option::into_iter`
        // gives zero or one item, `collect` makes a `Vec` of it).
        let days = if start == end {
            self.garmin
                .daily_summary(start)
                .await
                .map(|day| day.into_iter().collect::<Vec<_>>())
        } else {
            self.garmin.daily_summaries(start, end).await
        }
        .map_err(|e| format!("{e:#}"))?;

        if days.is_empty() {
            return Ok(format!("No daily summary recorded from {start} to {end}."));
        }
        serde_json::to_string(&days).map_err(|e| e.to_string())
    }

    /// Returns the user's heart rate zones as compact JSON.
    ///
    /// This tool takes no arguments, so the method has no `Parameters(...)`
    /// argument; `#[tool]` then advertises an empty input schema.
    #[tool(
        description = "Get the user's heart rate zones from Garmin Connect, one set per sport (DEFAULT, and RUNNING/CYCLING/SWIMMING/ROWING if configured): each of the 5 zones as [low, high] bpm, the training method (HR_MAX, LACTATE_THRESHOLD, HR_RESERVE, CUSTOM), and the max, resting and lactate threshold heart rates used. Use it to read the hr_zones_min of activities in bpm."
    )]
    async fn get_heart_rate_zones(&self) -> Result<String, String> {
        let zones = self
            .garmin
            .heart_rate_zones()
            .await
            .map_err(|e| format!("{e:#}"))?;
        serde_json::to_string(&zones).map_err(|e| e.to_string())
    }

    /// Returns HRV per night for a date range as compact JSON, or a short
    /// message if no night in the range has data.
    #[tool(
        description = "Get the user's overnight heart rate variability (HRV) from Garmin Connect, one entry per night (date = the day the user woke up): last night's average and highest 5-minute average, 7-day average, the personal baseline (balanced range [low, high] and the upper limit of LOW), HRV status (BALANCED, UNBALANCED, LOW, POOR) and Garmin's feedback code. All values in ms. Defaults to the last 7 nights; a range can span up to 366 days. For a single day (start_date = end_date) the result also has the measured window and every 5-minute reading as [local HH:MM, ms]. Nights without data are omitted."
    )]
    async fn get_hrv(&self, Parameters(req): Parameters<GetHrvRequest>) -> Result<String, String> {
        // `.as_deref()` turns `Option<String>` into `Option<&str>`
        // (borrowing the text instead of moving it).
        let (start, end) = date_range(req.start_date.as_deref(), req.end_date.as_deref(), 7, 366)?;

        // One day: use the single-night endpoint, which has the readings.
        // It returns one day; wrap it in a list so both cases have the same
        // shape. `Option::map(|d| vec![d])` does that only if there is data.
        let days = if start == end {
            self.garmin
                .hrv_night(start)
                .await
                .map(|d| d.map(|d| vec![d]))
        } else {
            self.garmin.hrv_range(start, end).await
        }
        .map_err(|e| format!("{e:#}"))?;

        match days {
            Some(days) if !days.is_empty() => {
                serde_json::to_string(&days).map_err(|e| e.to_string())
            }
            // `_` matches everything else: `None` or an empty list.
            _ => Ok(format!("No HRV data recorded from {start} to {end}.")),
        }
    }

    /// Returns weigh-ins, body composition and the weight goal for a date
    /// range as compact JSON, or a short message if there is nothing.
    #[tool(
        description = "Get the user's weight and body composition from Garmin Connect for a date range: one entry per day with a weigh-in (newest first) with local time, weight, change from the previous weigh-in, BMI, body fat and body water percent, muscle and bone mass, and visceral fat, physique rating and metabolic age when the scale reports them, plus the source (e.g. INDEX_SCALE, MANUAL); days with several weigh-ins also list each one. Also the averages over the range, the closest weigh-in before and after the range (for the trend), and the user's weight goal (target, start date, who set it). Masses in kg. Defaults to the last 30 days; start_date and end_date can be at most 366 days apart."
    )]
    async fn get_weight(
        &self,
        Parameters(req): Parameters<GetWeightRequest>,
    ) -> Result<String, String> {
        // Garmin accepts 10 years and more, but people who weigh daily get
        // about 300 bytes per day; a year is about 110 KB at most.
        let (start, end) = date_range(req.start_date.as_deref(), req.end_date.as_deref(), 30, 366)?;
        let report = self
            .garmin
            .weight(start, end)
            .await
            .map_err(|e| format!("{e:#}"))?;
        if report.is_empty() {
            return Ok(format!("No weight recorded around {start} to {end}."));
        }
        serde_json::to_string(&report).map_err(|e| e.to_string())
    }

    /// Returns the Body Battery report per day as compact JSON, or a short
    /// message if no day in the range has data.
    #[tool(
        description = "Get the user's Body Battery (Garmin's 5-100 energy level) through the day from Garmin Connect, one entry per day: charged and drained totals, the day's lowest and highest level, the level curve as [local HH:MM, level] pairs (the low and the high of each interval), Garmin's feedback (latest and end-of-sleep comment with level), and every event that charged or drained it (sleep, activity, recovery, stress, nap: local start, minutes, impact, Garmin's feedback codes). A day still in progress has tracked_until_local. Defaults to the last 7 days with 12 intervals per day (2 hours); start_date and end_date can be at most 30 days apart. Use intervals_per_day 96 for 15-minute detail on a single day. Days without data are omitted."
    )]
    async fn get_body_battery(
        &self,
        Parameters(req): Parameters<GetBodyBatteryRequest>,
    ) -> Result<String, String> {
        // Garmin's limit is 31 days in all, i.e. 30 days apart.
        let (start, end) = date_range(req.start_date.as_deref(), req.end_date.as_deref(), 7, 30)?;
        // Garmin accepts any number, but 96 (15 minutes) is already finer
        // than its readings are useful for, and 2 points per interval adds up.
        let chunks = req.intervals_per_day.unwrap_or(12);
        if !(1..=96).contains(&chunks) {
            return Err(format!("intervals_per_day must be 1 to 96, got {chunks}"));
        }
        let days = self
            .garmin
            .body_battery(start, end, chunks)
            .await
            .map_err(|e| format!("{e:#}"))?;
        if days.is_empty() {
            return Ok(format!("No Body Battery recorded from {start} to {end}."));
        }
        serde_json::to_string(&days).map_err(|e| e.to_string())
    }

    /// Returns one day's heart rate through the day as compact JSON, or a
    /// short message if the day has no data.
    #[tool(
        description = "Get the user's heart rate through one day from Garmin Connect (all-day wrist readings, one every 2 minutes, including sleep and workouts): resting HR, 7-day average resting HR, min and max HR, and the readings grouped into intervals as [local HH:MM start, average, lowest, highest] bpm (default 15 minutes, about 1.7 KB per day). interval_min 2 returns every reading as [local HH:MM, bpm] (about 10 KB). Intervals without readings (watch off) are missing. A day still in progress has tracked_until_local. Abnormal-HR thresholds and flagged readings are included when Garmin sends them. One day per call; for daily resting HR over many days use get_daily_summary."
    )]
    async fn get_heart_rate_day(
        &self,
        Parameters(req): Parameters<GetHeartRateDayRequest>,
    ) -> Result<String, String> {
        let date = match req.date {
            Some(text) => parse_date(&text)?,
            None => Local::now().date_naive(),
        };
        let interval = req.interval_min.unwrap_or(15);
        if !(2..=60).contains(&interval) {
            return Err(format!("interval_min must be 2 to 60, got {interval}"));
        }
        let day = self
            .garmin
            .heart_rate_day(date, interval)
            .await
            .map_err(|e| format!("{e:#}"))?;
        match day {
            Some(day) => serde_json::to_string(&day).map_err(|e| e.to_string()),
            None => Ok(format!("No heart rate recorded on {date}.")),
        }
    }
}

// Implement the `ServerHandler` trait: the interface `rmcp` uses to talk to
// any MCP server. Most of its methods have default implementations, so we
// only write the ones we need.
//
// `#[tool_handler(router = self.tool_router)]` generates the trait methods
// that list tools and dispatch tool calls, forwarding them to the router
// stored in the `tool_router` field. It skips any method already written
// here, so our own `call_tool` below replaces the generated one.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for GarminServer {
    /// Runs a tool call through the router (as the generated version would),
    /// then adds the update note, if there is one, to the first successful
    /// result. The note is a separate text block after the tool's JSON, so
    /// the data itself is unchanged.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let call = ToolCallContext::new(self, request, context);
        let mut response = self.tool_router.call(call).await?;
        // `if let` with `&mut` borrows the result inside the enum so it can
        // be changed in place. `Complete` is a finished call (other variants
        // are for protocol extensions this server doesn't use). Errors
        // (`is_error == Some(true)`) don't carry the note.
        if let CallToolResponse::Complete(result) = &mut response
            && result.is_error != Some(true)
            && let Some(note) = self.update.take()
        {
            result.content.push(ContentBlock::text(note));
        }
        Ok(response)
    }

    /// Describes this server to the client during the initial handshake:
    /// what it can do (capabilities), its name and version, and free-form
    /// instructions for the AI.
    fn get_info(&self) -> ServerConfig {
        // Declare that this server offers tools (as opposed to, say, prompts
        // or resources). The builder pattern chains calls and ends with
        // `.build()` to produce the final value.
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            // Name and version. `env!` reads values at *compile time*; Cargo
            // sets these from the `[package]` section of `Cargo.toml`.
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            // Hint given to the AI client about what this server is for.
            .with_instructions("Tools for reading Garmin Connect data.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a request from tool-argument JSON, the way the client sends
    /// it, and converts it to Garmin's query.
    fn query(json: serde_json::Value) -> Result<ActivitiesQuery, String> {
        let request: ListActivitiesRequest = serde_json::from_value(json).unwrap();
        request.into_query()
    }

    #[test]
    fn filters_are_converted_to_garmin_units() {
        let q = query(serde_json::json!({
            "start_date": "2026-10-01",
            "end_date": " 2026-10-15 ",
            "min_distance_km": 5.5,
            "max_distance_km": 21.0975,
            "min_duration_min": 30,
            "max_duration_min": 90.5,
            "min_elevation_gain_m": 100,
            "max_elevation_gain_m": 500,
        }))
        .unwrap();
        assert_eq!(
            q,
            ActivitiesQuery {
                start: 0,
                limit: 5,
                activity_type: None,
                start_date: NaiveDate::from_ymd_opt(2026, 10, 1),
                end_date: NaiveDate::from_ymd_opt(2026, 10, 15),
                min_distance: Some(5500),
                // 21097.5 m rounds to a whole meter: Garmin rejects decimals.
                max_distance: Some(21098),
                min_duration: Some(1800),
                max_duration: Some(5430),
                min_elevation: Some(100),
                max_elevation: Some(500),
            }
        );
    }

    #[test]
    fn date_range_defaults_and_limits() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        // Only the end given: a week ending on it.
        assert_eq!(
            date_range(None, Some("2026-10-24"), 7, 366),
            Ok((d("2026-10-18"), d("2026-10-24")))
        );
        // Exactly 366 days apart is allowed, one more is not.
        assert!(date_range(Some("2025-10-23"), Some("2026-10-24"), 7, 366).is_ok());
        assert_eq!(
            date_range(Some("2025-10-22"), Some("2026-10-24"), 7, 366),
            Err("start_date and end_date are 367 days apart; the maximum is 366".into())
        );
        assert_eq!(
            date_range(Some("2026-10-24"), Some("2026-10-18"), 7, 366),
            Err("start_date (2026-10-24) is greater than end_date (2026-10-18)".into())
        );
    }

    #[test]
    fn no_arguments_means_no_filters() {
        let q = query(serde_json::json!({})).unwrap();
        assert_eq!(
            q,
            ActivitiesQuery {
                limit: 5,
                ..Default::default()
            }
        );
    }

    #[test]
    fn limit_is_clamped() {
        assert_eq!(query(serde_json::json!({"limit": 0})).unwrap().limit, 1);
        assert_eq!(query(serde_json::json!({"limit": 500})).unwrap().limit, 100);
    }

    #[test]
    fn bad_input_gets_a_clear_error() {
        let err = |json| query(json).unwrap_err();
        assert_eq!(
            err(serde_json::json!({"start_date": "23.10.2026"})),
            "invalid date \"23.10.2026\": expected YYYY-MM-DD"
        );
        assert_eq!(
            err(serde_json::json!({"start_date": "2026-10-15", "end_date": "2026-10-01"})),
            "start_date (2026-10-15) is greater than end_date (2026-10-01)"
        );
        assert_eq!(
            err(serde_json::json!({"min_distance_km": 10, "max_distance_km": 5})),
            "min_distance_km (10.0) is greater than max_distance_km (5.0)"
        );
        assert_eq!(
            err(serde_json::json!({"min_duration_min": -1})),
            "min_duration_min must not be negative, got -1"
        );
    }
}
