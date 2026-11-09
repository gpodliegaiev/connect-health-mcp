# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`garmin-mcp` is an MCP (Model Context Protocol) server in Rust (edition 2024) that exposes tools for reading Garmin Connect data, so an AI can evaluate the user's training and health. Early stage. Tools: `list_activities`, `get_activity`, `get_sleep`, `get_fitness_age`, `get_hrv`, `get_daily_summary`, `get_heart_rate_zones`, `get_weight`, `get_body_battery`, `get_heart_rate_day`.

## Commands

```sh
cargo build                 # build
cargo run                   # run server on stdio (waits for an MCP client)
cargo clippy --all-targets  # lint
cargo fmt                   # format
cargo test                  # all tests
cargo test <name>           # single test by name substring
dist plan                   # check the release config (see Releases)
```

## Architecture

- Built on the `rmcp` crate (feature `transport-io`) with a `tokio` multi-thread runtime.
- `src/main.rs` — entry point; dispatches on the first argument: none → `serve()` (loads `Config`, builds `GarminClient`, serves `GarminServer` over stdio; prints a hint if stdin is a terminal), `setup` → `setup::run()`, `--version`, `--help`; anything else is an error with the usage text.
- `src/update.rs` — update notice. `serve()` calls `update::spawn_check` (unless `Config::update_check` is false, i.e. `GARMIN_MCP_NO_UPDATE_CHECK` is set to anything but `0`/`false`/`no`/`off`): a background task reads `config_dir()/update-check.json` and asks `api.github.com/repos/gpodliegaiev/garmin-mcp/releases/latest` only if the cache is older than 24 h (the attempt is cached even when it fails; 404 = no release yet → `release: null`). If the tag (`v` stripped, `semver`) is newer than `CARGO_PKG_VERSION`, the note goes into a shared `UpdateNotice` (`OnceLock` + `AtomicBool`); `GarminServer::call_tool` (hand-written, replacing the one `#[tool_handler]` would generate) appends it as an extra text block to the first non-error tool result, once per session. The update command is chosen from `current_exe()`: `/Cellar/` → brew, `garmin-mcp-update` next to it → that, `target/release|debug` → rebuild, else the release page. Never updates anything itself.
- `src/setup.rs` — `garmin-mcp setup`: hidden prompt (`rpassword`), checks the key with `GarminClient::check_key` (4xx except 429 = rejected, nothing saved; network error / 5xx / 429 → ask "save anyway?"), writes `config_dir()/.env` keeping its other lines (old `GARMIN_API_KEY` lines replaced; quoted with `'...'` if it has `#`, `$`, `\` etc.) via a `0600` temp file + rename, warns if `config::api_key_source()` shows a higher-priority key, then prints Claude Code / Claude Desktop entries with `current_exe()`'s full path. The key is never printed.
- `src/config.rs` — `Config` (`GARMIN_API_KEY` required, `GARMIN_MCP_NO_UPDATE_CHECK` optional). The API address is the `BASE_URL` constant in `client.rs`, deliberately not configurable: a `.env` in any project the user opens is read, so a configurable URL would let it redirect the key. Each value: env var → `.env` in the cwd or a parent → `config_dir()/.env` (`~/.config/garmin-mcp`, `$XDG_CONFIG_HOME` if absolute, `%APPDATA%` on Windows) → `.env` next to the binary; first non-empty (trimmed) wins. Files are parsed with `dotenvy::from_path_iter`, never loaded into the process env; `Config::resolve` takes the env lookup and file list as arguments for tests; the lookup itself is `Sources::find`, which also reports the `Source` (used by `api_key_source()` for setup's priority warning). The missing-key error lists every place checked. Parse warnings print only the path (dotenvy errors contain the line, which may be the key). Custom `Debug` hides the key.
- `src/garmin/auth.rs` — `TokenManager`: `POST /personalapitoken-service/personal-api/personal-api-key/token` with `Authorization: ApiKey <key>` returns a bearer JWT as plain text, valid 24h. Cached behind a tokio `Mutex`, refreshed 5 min before expiry. Uses `SystemTime`, not `Instant` (macOS `Instant` pauses during sleep). `check_key` returns `KeyCheck::Valid` / `Rejected(status)` without caching (an unknown key gets 401, an empty one 400).
- `src/garmin/client.rs` — `GarminClient`: authenticated GETs with `Authorization: Bearer <token>`; on 401 invalidates the token and retries once. API calls (`list_activities`, `activity_detail`, `daily_sleep`, `fitness_age`, `hrv_range`, `hrv_night`, `daily_summaries`, `daily_summary`, `sleep_range`, `heart_rate_zones`, `weight`, `body_battery`, `heart_rate_day`) return typed structs via generic `get_json<T, Q>` (`Q` = any `Serialize` query, e.g. `&[("date", "2026-10-23")]`). Endpoints that answer 204 (empty body) for "no data" use `get_optional_json`, which returns `Option<T>`.
- Response types all follow one pattern: a raw struct (Deserialize from Garmin camelCase, Garmin units, every field `Option`, unknown Garmin fields ignored) → `into_summary()` → an output struct (Serialize as snake_case, unit in every field name, `None` skipped).
- `src/garmin/types.rs` — shared pieces: `ActivityTypeFilter`, `SpeedStyle`, and `pub(super)` helpers used by both activity modules: `round`, `format_pace` ("m:ss"), `format_time` ("m:ss" or "h:mm:ss"), `positive` / `rounded` / `count` (0 → `None`), `to_minutes` (s → min, 2 dp), `only_true` (keep a flag only if `true`), `benefit` (strip training-effect message number), `event_type` (drop `uncategorized`). `null_as_default` (`#[serde(deserialize_with = ...)]` on every raw `Vec` field: struct-level `default` only covers a *missing* field, and Garmin sends `null` lists, e.g. `dailyNapDTOS` in the sleep range).   - Activity conversions: durations → minutes (2 dp); distance → km (3 dp), but meters for swimming; speed → `*_pace_per_km` "m:ss" for run/walk/hike, `*_pace_per_100m` for swim, `*_speed_kmh` otherwise (chosen by `SpeedStyle::for_activity` from the `typeKey`). Zero values (strength, indoor) mean "not tracked" → omitted. Kept even when 0: training effect, `difference_body_battery`; altitude and temperature can be ≤ 0, so they are not filtered.
  - Garmin stores many values as float32, so they arrive with noise (`2.4000000953674316`). Round only to real precision (training effect 1 dp, training load 1 dp, cadence and running dynamics 1 dp) via `round()`. Never round below what matters for health evaluation; leave already-clean fields (HR, power, calories, sleep metrics) unrounded. Sleep durations are converted seconds → minutes (1 dp).
  - `ActivityTypeFilter` enum = values accepted by the activity list's `activityType` param. Garmin accepts only *parent* types (`running` includes `treadmill_running`; strength training and `stair_climbing` are `fitness_equipment`, but `floor_climbing` and `breathwork` are `other`); subtypes, wrong case or unknown values get HTTP 400 `[]`. Used directly as tool input (`JsonSchema`) and as query value (`Serialize`), via the `ActivitiesQuery` struct in `client.rs`.
  - `ActivitiesQuery` (`client.rs`) holds every activity-search param in Garmin units: dates as `NaiveDate` (chrono `serde` feature → `YYYY-MM-DD`), distance in m, duration in s, elevation in m, all whole numbers (Garmin returns 404 for decimals). `ListActivitiesRequest::into_query` in `server.rs` converts the tool's units (km, minutes) into it, clamps `limit`, and rejects bad dates, negative values and reversed ranges. A reversed range gets no error from Garmin, just an empty list.
  - `SpeedStyle::convert` (m/s → pace/km, pace/100 m or km/h) and `SpeedStyle::distance` (m → km, or m for swimming) are used by both activity outputs.
- `src/garmin/activity.rs`: activity list entry `Activity` → `ActivitySummary`. Garmin's richest activity record (`SummarizedActivityDTO`, ~110 keys per real entry, ~230 in the spec); every workout metric is output, for full AI evaluation. Left out on purpose: owner/profile, privacy, social (likes, comments), media flags, GPS coordinates, device manufacturer, ids/UUIDs, duplicate flags (`pr` vs `isPR`) and repeats (`startTimeGMT`, `beginTimestamp`, `maxDoubleCadence`, `minActivityLapDuration`). Quirks:
  - Names with `_N` suffixes (`hrTimeInZone_1`, `powerTimeInZone_1`, `maxAvgPower_60`, `fastestSplit_1609`) need explicit `rename`. HR zones are always sent (0.0 if empty); time below zone 1 is in no zone. Output as `hr_zones_min` / `power_zones_min` objects, omitted when every zone is 0.
  - `maxAvgPower_*` (power curve) and `fastestSplit_*` (seconds, names are meters: `1609` = mile, `91` = 100 yd) are read with `#[serde(flatten)]` into sub-structs.
  - Odd units per the spec: `poolLength` cm, `getlactateThresholdSpeed` (sic) and `concentricMeanVelocity` cm/ms, `maxDepth` cm, `surfaceInterval` ms, `caloriesEstimated` kJ; exercise-set `duration` ms, `maxWeight` and `volume` g.
  - `summarizedExerciseSets` → `exercises` (per-exercise sets, reps, max weight, volume); body-weight exercises have weight 0 → omitted.
  - `elapsed_duration_min` is only output when it differs from `duration_min` (i.e. there were pauses); `laps` only when > 1; `event_type` `uncategorized` is dropped.
  - `splitSummaries` use the same `RawSplit` / `SplitSummary` as details (list splits have step and ski fields, detail splits have HR).
- `src/garmin/activity_detail.rs`: `GET /activity-service/activity/{id}` → `ActivityDetail` (raw: `activityTypeDTO`, `eventTypeDTO`, `timeZoneUnitDTO`, `summaryDTO`, `splitSummaries`; the `DTO` names need explicit `rename`) → `into_summary()` → flat `ActivityDetailSummary`, following the same unit and rounding rules. Quirks:
  - `startTimeLocal` here is `"YYYY-MM-DDTHH:MM:SS.0"`, so it is normalized to the list's format.
  - `avgElevation: -500.0` is a "no elevation" sentinel (indoor), so it is not output.
  - Many untracked metrics are `0.0`, so they are filtered out with `positive()`.
  - Nearly everything here is also in the list entry; only time zone, min/recovery HR, average moving speed, stamina, and per-split HR are extra.
  - `summaryDTO.totalSets` counts rest periods, so `activeSets` (equal to the list's `totalSets`) is output as `total_sets`.
  - Training-effect messages lose their trailing variant number (`IMPACTING_TEMPO_22` becomes `IMPACTING_TEMPO`).
  - An empty `description` (`""`) is dropped.
  - An unknown id returns 404 `{}`.
  - The response has no per-exercise strength sets; the list's `summarizedExerciseSets` does.
- `src/garmin/sleep.rs`: `dailySleep` → `DailySleep` (raw, nested, all `Option`) → `into_summary()` → flat `SleepSummary`. `dailySleepSummariesByDate?startDate&endDate` returns a list of the same `DailySleep` objects; `get_sleep` uses it when `start_date`/`end_date` are given (one object for `date`, a list for a range). Quirks:
  - The range allows at most 31 days between the dates (spec says "31 days"; 400 `{}` beyond, reversed, or without dates). Nights without data are left out or sent as all-`null` entries (dropped); no data at all is `[]`. Some nights send `"dailyNapDTOS": null`.
  - Needs `date=YYYY-MM-DD` (filed under the wake-up day); a day without data returns 200 with every field `null` (→ `None`), a malformed date returns 404.
  - `...TimestampLocal` fields are epoch ms pre-shifted to local time, so format them as UTC. Naps (`dailyNapDTOS`) have GMT text times plus an offset in minutes.
  - `sleepScores` factors have `qualifierKey` and `optimalStart`/`optimalEnd` (output as `[low, high]`; `totalDuration` in seconds → minutes); only overall and stage percentages have a `value`.
  - `sleepNeed` / `nextSleepNeed` / `sleepAlignment` times (`recommendedBedtimeStartMins`, `optimalSleepWindowStartMins`, `...MidpointMins`) are minutes after local midnight → "HH:MM" (wrapping for negative values).
  - "Nothing to report" values are dropped: `NOT_AVAILABLE` insight, `NO_CHANGE` adjustments, 0 s durations. SpO2 fields are only sent when SpO2 tracking was on.
- `src/garmin/fitness_age.rs`: `GET /fitnessage-service/fitnessage/v2/{date}` → `FitnessAge` → `into_summary()` → `FitnessAgeSummary` (ages 1 dp, `components` renamed with units: `vigorous_days_per_week`, `vigorous_minutes_per_week`, `resting_hr_bpm`, `bmi`, `body_fat_percent`). Quirks:
  - The response is one object, not the array the spec says.
  - It is the latest Fitness Age on or before the date (a future date returns today's); `lastUpdated` (always midnight) is output as a date.
  - `bodyFat` replaces `bmi` when there is a smart-scale reading.
  - A day without Fitness Age still returns 200: only `chronologicalAge` and every component `stale: true` with `staleInputs`.
  - `achievableFitnessAge` and per-component `targetValue`, `improvementValue`, `potentialAge`, `priority` only appear when Garmin has a suggestion.
  - `fullHistoryScan` made no difference in probes, so it isn't sent. A malformed date returns 404 `{}`.
- `src/garmin/daily_summary.rs`: `GET /usersummary-service/v2/dailysummaries?startDate&endDate` → `DailySummaries` (nested groups) and `GET /usersummary-service/usersummary/daily?calendarDate` → `UserDailySummary` (flat), both → `DaySummary`. `get_daily_summary` uses the single-day endpoint when `start_date == end_date`; it adds Body Battery at wake / during sleep, 7-day resting HR, absolute min/max HR, activity-level minutes, total distance, and activity names on Body Battery events. Quirks:
  - Garmin allows 365 days between the dates (400 `{}` beyond, reversed, or without dates), but a year of output is ~500 KB, so the tool caps ranges at 31 days.
  - No data: range returns `{"data": []}`; single day returns 200 with `includes*Data: false` and nothing else (→ `None`).
  - Range `heartRate.minValue/maxValue` are 2-minute averages; `respiration.avgValue` is the waking average. Stress durations are ms in the range response, seconds in the single day; floors are fractional in the single day.
  - Hydration has a goal every day but `consumed` 0 unless logged, so it is only output when logged. Stress qualifier is upper-cased (range sends lower case), `UNKNOWN` dropped.
  - Single-day Body Battery events have GMT start + `timezoneOffset` (ms); range events have a local start.
  - A partial day (< 24 h of data, e.g. today) gets `tracked_hours` and `tracked_until_local`.
- `src/garmin/heart_rate_day.rs`: `GET /wellness-service/wellness/dailyHeartRate?date` → `DailyHeartRate` → `into_summary(interval_min)` → `HeartRateDay` (resting/min/max HR and the 2-minute readings grouped by `group()` into `intervals_bpm: [HH:MM, avg, min, max]` aligned to local midnight, or kept as `readings_bpm` with `interval_min` 2). Quirks:
  - One `date` per call; `startDate`/`endDate` or no date 400 `{}`, malformed 404 `{}`; a day without data is 200 with every field `null`.
  - Reading timestamps are real UTC; Garmin uses one offset (`startTimestampLocal − startTimestampGMT`) for the whole day, even on a daylight-saving day.
  - For today `endTimestampLocal` still says midnight while `endTimestampGMT` is the last sync, so `tracked_until_local` is computed from the GMT end.
  - A `null` reading marks the start of a gap (watch off). Abnormal-HR thresholds and values are in the spec only.
- `src/garmin/heart_rate_zones.rs`: `GET /biometric-service/heartRateZones/v2` → `Vec<HeartRateZones>` → `Vec<ZoneSet>`, one per sport. Zone floors become `zones_bpm.zone_N: [low, high]` (high = next floor − 1; zone 5 ends at max HR), same keys as activity `hr_zones_min`. `generateDefaults=false` made no difference, so it isn't sent.
- `src/garmin/hrv.rs`: `GET /hrv-service/hrv/daily/{from}/{to}` → `HrvRange` and `GET /hrv-service/hrv/{date}` → `HrvNight`, both → `Vec<HrvDay>` (values in ms; baseline as `baseline_balanced_ms: [low, high]` + `baseline_low_upper_ms`). `get_hrv` uses the single-day endpoint when `start_date == end_date`, adding the measured window and 5-minute readings as `[HH:MM, ms]`. Quirks:
  - No data returns 204 with an empty body (both endpoints).
  - The range may be at most 366 days between the dates; longer or reversed returns 400 `{}`. Days without data are absent.
  - Onboarding nights have `weeklyAvg` / `baseline` `null` and status `NONE` (dropped), feedback `ONBOARDING_1`.
  - Feedback phrase numbers are kept (unlike training-effect messages): their meaning isn't documented.
- `src/garmin/body_battery.rs`: `GET /wellness-service/wellness/bodyBattery/reports/daily?startDate&endDate&chunks` → `Vec<BodyBatteryReport>` → `Vec<BodyBatteryDay>` (charged, drained, lowest, highest, `levels` as `[HH:MM, level]`, feedback, events). Reuses `RawEventV1`, `RawFeedback` and `BodyBatteryEvent` from `daily_summary.rs` (same event shape). Quirks:
  - `chunks=N` → 2N `[utc_ms, level]` pairs: the low and high of each of N equal intervals (default 3 when absent or 0). The tool sends 1–96, default 12.
  - Pair timestamps are real UTC (unlike sleep's local-shifted ms); the offset comes from `startTimestampLocal − startTimestampGMT`, and on a daylight-saving day the end offset is used from 03:00 local on.
  - At most 31 days inclusive (30 apart); beyond, reversed or missing dates 400 `{}`, malformed 404 `{}`. Days without data are still listed with `null` levels and fake timestamps (dropped). Today ends at the last sync → `tracked_until_local`; its intervals split only the hours so far.
- `src/garmin/weight.rs`: `GET /weight-service/weight/range/{start}/{end}?includeAll=true` → `WeightRange` and `GET /goal-service/goal/user/effective/weightgoal/{start}/{end}` → `WeightGoals`, fetched together (`tokio::try_join!`) and merged by `WeightRange::into_summary(goals)` → `WeightReport` (goals, range average, days newest first with each weigh-in when there were several, closest weigh-ins before/after the range). Quirks:
  - Weights and masses are grams (the spec says kg). `weightDelta` is grams, computed from weights rounded to 0.1 kg; `latestWeight`'s compares with the previous day's weigh-in, `allWeightMetrics` entries with the weigh-in just before.
  - `date` is epoch ms pre-shifted to local time (format as UTC).
  - Range of 10+ years works; reversed 400 `{}`, malformed date 404 `{}`; no weigh-in → `dailyWeightSummaries: []` and an all-`null` average. The tool caps ranges at 366 days (a year is ~16 KB of output).
  - `/weight-service/weight/all/...` answers 400 `{}` to every range, so it isn't used.
  - The goal endpoint ignores its dates (always returns the current effective goal).
- `src/server.rs` — `GarminServer`, the MCP handler:
  - Tools are async methods in the `#[tool_router] impl GarminServer` block, each annotated `#[tool(description = ...)]`.
  - Tool inputs are structs deriving `Deserialize` + `JsonSchema` (via `rmcp::schemars`), received as `Parameters<T>`. Doc comments on fields become the JSON schema descriptions shown to clients.
  - `#[tool_handler]` on the `ServerHandler` impl wires the router in; `get_info` declares capabilities, server info, and instructions.
- `date_range(start, end, default_days, max_days)` turns optional `start_date` / `end_date` arguments into a checked range (defaults to the last `default_days` ending today); use it for every range tool.
- To add a tool: define a request struct, then add a `#[tool]` method to the `#[tool_router]` impl.

## Response fields

Every tool must return **all useful fields** Garmin sends, so the AI can do a full evaluation without another call. When adding or changing an endpoint:

1. Fetch real responses (several days or sports) and read the spec schema in `docs/garmin-openapi.json`. Nested objects are often untyped in the spec, so real data is the main source.
2. Output every field about the user's training, health or body, including nested objects, lists and sport-specific fields seen only in the spec.
3. Leave out only what isn't about the data itself: profile/owner/device ids, UUIDs, privacy and social fields, media flags, GPS coordinates, upload/file metadata, GMT copies of local times, and duplicates of fields already output. List what was left out in the module's `//!` doc.
4. Hide "nothing to report" values with `skip_serializing_if`: `null`, placeholder `0`, empty strings and lists, `false` flags, and codes like `uncategorized`, `NOT_AVAILABLE`, `NO_CHANGE`. Keep a `0` that is a real result (training effect 0.0, body battery change 0, awake count 0, 0.0 inside a zone breakdown, altitude/temperature ≤ 0).
5. Convert to readable units with the unit in the field name, following the conversion and rounding rules above.
6. Update the tool's `description` and the README tool table to name the field groups returned.

## Garmin API reference

Local copy of Garmin's swagger docs (the online page renders client-side and can't be fetched as a spec). Both files are Garmin's and git-ignored; never commit them. If missing, run `python3 docs/update_garmin_spec.py`.

- `docs/garmin-api.md`: endpoint index with params, response schema names, and implementation status (✅, MCP tool, `GarminClient` method), found by scanning `src/` for endpoint paths. Check it first when adding a tool or param.
- `docs/garmin-openapi.json`: full OpenAPI 3.0 spec, including response schemas (`components.schemas`, e.g. `SummarizedActivityDTO`, `DailySleepDTO`).
- `python3 docs/update_garmin_spec.py` regenerates both from developer.garmin.com. After adding or removing an endpoint call, run it with `--offline` to refresh the status from the local spec.

The spec omits some behavior found by probing (see the quirks under Architecture), and it doesn't list the token endpoint.

## Code comments

The maintainer is new to Rust. Comment code generously so it can be read as a learning resource:

- Every function, struct, enum, and module gets a `///` doc comment saying what it does and why it exists, in plain language.
- Explain Rust-specific concepts where they appear, briefly: ownership/borrowing (`&`, `&mut`, moves), `Result`/`Option` and the `?` operator, `async`/`.await`, traits and `impl` blocks, derive macros, lifetimes, and pattern matching/destructuring (e.g. `Parameters(req)`).
- Explain what macros like `#[tool]`, `#[tool_router]`, `#[tool_handler]`, and `#[tokio::main]` do behind the scenes.
- Inside function bodies, add `//` comments for each non-obvious step, describing what it does and what data flows through it.
- Keep comments accurate when changing code; update or remove stale ones.

## Configuration

Copy `.env.example` to `.env` and set `GARMIN_API_KEY`. `.env` is git-ignored. Real env vars (e.g. from the MCP client config) override every `.env` file. Users install with the cargo-dist installer and run `garmin-mcp setup`, which puts the key in `~/.config/garmin-mcp/.env` (see `src/config.rs` for the full lookup order).

## Releases

Built by [cargo-dist](https://github.com/axodotdev/cargo-dist) (the `dist` CLI, version pinned in `dist-workspace.toml`):

- `dist-workspace.toml` holds the config: targets (macOS arm64/x86_64, `x86_64-unknown-linux-musl`, Windows x86_64 MSVC), `.tar.gz` archives on Unix and `.zip` on Windows, shell + PowerShell installers, `install-path = "~/.local/bin/"`, `install-updater = true` (ships `garmin-mcp-update`, which `update.rs` points to). `LICENSE`, `README.md` and `CHANGELOG.md` go into every archive.
- `.github/workflows/release.yml` is **generated**: never edit it by hand. After changing `dist-workspace.toml` or upgrading dist, run `dist generate` and commit the result (CI fails if the workflow is out of date).
- On a pushed tag `vX.Y.Z` the workflow builds every target, then publishes a GitHub release with the archives, installers, checksums, and the `## X.Y.Z` section of `CHANGELOG.md` as release notes. On pull requests it only runs `dist plan`.
- The installer URLs in the README use `releases/latest/download/...`, so they always fetch the newest release.

To cut a release:

1. Bump `version` in `Cargo.toml` (the tag must match it) and run `cargo build` to update `Cargo.lock`.
2. Move the `## Unreleased` notes in `CHANGELOG.md` under a new `## X.Y.Z - YYYY-MM-DD` heading.
3. Check locally: `dist plan` (lists the artifacts, reads the changelog) and optionally `dist build --artifacts=local` (builds the host archive into `target/distrib/`).
4. Commit, then `git tag vX.Y.Z && git push && git push origin vX.Y.Z`. Pushing a tag publishes to users, so ask the maintainer first.

Users on older versions see the new release through the update notice within a day.

## Constraints

- **stdout is the MCP protocol channel.** Never `println!` or write anything else to stdout; log to stderr (`eprintln!`). The terminal commands (`setup`, `--help`, `--version`) print to stderr too, so the rule has no exceptions.
- Never log or print the API key or bearer token.
