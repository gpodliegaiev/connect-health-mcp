# garmin-mcp

An [MCP](https://modelcontextprotocol.io) server, written in Rust, that gives AI assistants such as Claude, ChatGPT (desktop app and Codex) and Gemini CLI read access to your Garmin Connect data through the [Garmin Connect personal API](https://developer.garmin.com/gc-developer-program/personal/personal-api/).

## Tools

| Tool | What it returns | Arguments |
|---|---|---|
| `list_activities` | Your most recent activities, newest first, with every metric Garmin recorded: time, distance, pace, elevation, heart rate and time in each HR zone, power (zones, power curve, normalized power, FTP, TSS), cadence, running dynamics, fastest splits, training effect and load, VO2 max, body battery, respiration, stress, calories, sweat loss, swim/paddle/dive/MTB metrics, strength exercises (sets, reps, weight, volume per exercise), and split totals. | `start` (default 0), `limit` (default 5, max 100), and optional filters: `activity_type` (see below), `start_date` / `end_date` (`YYYY-MM-DD`), `min_distance_km` / `max_distance_km`, `min_duration_min` / `max_duration_min`, `min_elevation_gain_m` / `max_elevation_gain_m` |
| `get_activity` | One activity by id. Most metrics are already in `list_activities`; this adds the time zone, min and recovery heart rate, average moving pace, stamina, and heart rate per split type. | `activity_id` (from `list_activities`) |
| `get_body_battery` | Body Battery through the day: charged and drained, the day's low and high, the level curve (low and high of each interval with local time), Garmin's feedback, and every event that charged or drained it (sleep, activity, recovery, stress, nap) with its impact. | `start_date` / `end_date` as `YYYY-MM-DD` (default the last 7 days, up to 30 days apart), `intervals_per_day` 1–96 (default 12, i.e. 2 hours) |
| `get_fitness_age` | Garmin Fitness Age as of a date: chronological, current, previous and achievable fitness age, and each input (vigorous days and minutes per week, resting heart rate, BMI or body fat) with Garmin's target, the fitness age reachable by hitting it, and its priority. Inputs without recent data are marked stale. | `date` as `YYYY-MM-DD` (default today); returns the latest values on or before it |
| `get_daily_summary` | All-day wellness per day: steps, floors, calories, resting and min/max heart rate, intensity minutes, stress (level and minutes per category), Body Battery with every event that charged or drained it, respiration, SpO2, hydration. For a single day, also Body Battery at wake and during sleep, 7-day resting HR, time per activity level, and the activity behind each Body Battery event. | `start_date` / `end_date` as `YYYY-MM-DD` (default the last 7 days, up to 31 days apart) |
| `get_heart_rate_day` | Heart rate through one day (all-day wrist readings every 2 minutes): resting HR and its 7-day average, min and max, and the readings per interval as average, lowest and highest with local time (gaps where the watch was off are left out). | `date` as `YYYY-MM-DD` (default today), `interval_min` 2–60 (default 15; 2 returns every reading) |
| `get_heart_rate_zones` | Your heart rate zones per sport: each zone as a `[low, high]` bpm range, the training method, and the max, resting and lactate threshold heart rates used. Explains the `hr_zones_min` of activities. | none |
| `get_hrv` | Overnight heart rate variability per night: last night's average and 5-minute high, 7-day average, personal baseline (balanced range, low limit), HRV status and feedback code, in ms. For a single day, also the measured window and every 5-minute reading. | `start_date` / `end_date` as `YYYY-MM-DD` (default the last 7 nights, up to 366 days apart) |
| `get_sleep` | Sleep per night with every metric Garmin recorded: bed and wake times, deep/light/REM/awake durations and naps, sleep score with each factor's rating and optimal range, heart rate, respiration, SpO2 and breathing disruption (when tracked), stress, sleep need with its adjustments, recommended bedtime for this and the next night, and sleep-timing alignment. | `date` as `YYYY-MM-DD` (default today) for one night, or `start_date` / `end_date` for a list of nights (default the last 7, up to 31 days apart) |
| `get_weight` | Weight and body composition per day with a weigh-in: weight, change from the previous weigh-in, BMI, body fat and body water percent, muscle and bone mass (plus visceral fat, physique rating and metabolic age if your scale reports them), each weigh-in when there were several in a day, the average over the range, the closest weigh-ins before and after it, and your weight goal. | `start_date` / `end_date` as `YYYY-MM-DD` (default the last 30 days, up to 366 days apart) |

Each field name includes its unit (`duration_min`, `distance_km`, `elevation_gain_m`, `calories_kcal`). Heart rate is in bpm. Speed is given in the usual way for each sport:

| Sport | Distance | Speed |
|---|---|---|
| Running, walking, hiking | `distance_km` | `avg_pace_per_km`, e.g. `"6:23"` |
| Swimming | `distance_m` | `avg_pace_per_100m`, e.g. `"2:05"` |
| Cycling and other sports | `distance_km` | `avg_speed_kmh` |

Every tool returns all useful fields Garmin sends. Fields that don't apply or weren't recorded are left out. Examples: distance for strength training, moving time for indoor activities, power zones without a power meter. Sleep durations are in minutes.

**Filters** can be combined, and all ranges are inclusive. Dates refer to each activity's local start date. Distance is in km for every sport, including swimming (1.5 means 1500 m). Duration is timer time, excluding pauses, the same as `duration_min` in the result (not `moving_duration_min`). Only `limit` activities come back, so raise it to get everything in a range.

**`activity_type` values:** `running`, `cycling`, `walking`, `hiking`, `swimming`, `fitness_equipment`, `other`, `multi_sport`, `winter_sports`, `water_sports`, `diving`, `team_sports`, `racket_sports`.

Garmin filters by sport group, not by exact type:
- `running` includes treadmill runs.
- Strength training, stair climbing and elliptical are under `fitness_equipment`.
- Floor climbing and breathwork are under `other`.

**Sleep dates:** Garmin files a night under the day you woke up, so last night's sleep is today's date.

## Install

You need a Garmin Connect personal API key (see [Garmin's guide](https://developer.garmin.com/gc-developer-program/personal/personal-api/)).

### 1. Install the binary

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/gpodliegaiev/garmin-mcp/releases/latest/download/garmin-mcp-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/gpodliegaiev/garmin-mcp/releases/latest/download/garmin-mcp-installer.ps1 | iex"
```

The installer downloads the right build for your system and puts `garmin-mcp` and `garmin-mcp-update` in `~/.local/bin` (`%USERPROFILE%\.local\bin` on Windows). If that folder isn't on your `PATH` yet, it adds it to your shell profile; open a new terminal afterwards. No Rust or other tools are needed.

Builds exist for macOS (Apple Silicon and Intel), Linux x86_64 and Windows x86_64. You can also download an archive from the [releases page](https://github.com/gpodliegaiev/garmin-mcp/releases) and put the binary anywhere. On macOS, a binary downloaded with a browser is blocked by Gatekeeper the first time; allow it with `xattr -d com.apple.quarantine garmin-mcp`. The installer doesn't have this problem.

### 2. Save your API key

```sh
garmin-mcp setup
```

`setup` asks for your API key, without showing it on screen, and checks it with Garmin. It then saves the key to `~/.config/garmin-mcp/.env` (`%APPDATA%\garmin-mcp\.env` on Windows), readable only by you, and prints the exact commands to add the server to Claude Code and Claude Desktop, with the binary's full path. Run it again to replace the key.

### 3. Connect your AI app

Paste the command or config entry that `setup` printed (for Claude Code and Claude Desktop). For ChatGPT, Codex, Gemini CLI, Cursor, VS Code and others, see [Connecting a client](#connecting-a-client).

Other commands: `garmin-mcp --help` and `garmin-mcp --version`. Without arguments, the binary is the MCP server; your AI app starts it.

### Build from source

Needs Rust 1.88 or newer (install with [rustup](https://rustup.rs)):

```sh
git clone https://github.com/gpodliegaiev/garmin-mcp.git
cd garmin-mcp
cargo build --release
target/release/garmin-mcp setup
```

The binary is written to `target/release/garmin-mcp`. Rebuild after pulling changes.

### Uninstall

Delete `garmin-mcp` and `garmin-mcp-update` from `~/.local/bin`, and the folder `~/.config/garmin-mcp` (your key, the update-check time and the installer's receipt). On Windows, the folders are `%USERPROFILE%\.local\bin`, `%APPDATA%\garmin-mcp` and `%LOCALAPPDATA%\garmin-mcp`. Then remove the server from your AI app (`claude mcp remove -s user garmin`, or the `"garmin"` entry in Claude Desktop's config).

### Configuration

| Variable | Required | Default |
|---|---|---|
| `GARMIN_API_KEY` | yes | none |
| `GARMIN_MCP_NO_UPDATE_CHECK` | no | unset (set to `1` to turn off the update check, see [Updates](#updates)) |

The server looks for each variable in these places, in order, and uses the first value it finds:

1. An environment variable, for example set in the MCP client's config (see below).
2. A `.env` file in the directory the server is started from, or one of its parents. Useful for development inside this repository.
3. **`~/.config/garmin-mcp/.env`** (`$XDG_CONFIG_HOME/garmin-mcp/.env` if that is set; `%APPDATA%\garmin-mcp\.env` on Windows). This is the recommended place. It works wherever the binary is and whichever directory the client starts it from, and it survives updates.
4. A `.env` file next to the `garmin-mcp` binary.

`garmin-mcp setup` writes the recommended place for you. To write it by hand instead:

```sh
mkdir -p ~/.config/garmin-mcp
echo 'GARMIN_API_KEY=<your-key>' > ~/.config/garmin-mcp/.env
chmod 600 ~/.config/garmin-mcp/.env    # only you can read it
```

If no key is found, the server exits with `GARMIN_API_KEY is not set` and lists every place it looked.

The repository's `.env` is git-ignored. Never commit your API key.

## Connecting a client

### Claude Code

To make the server available in every project, with the key in `~/.config/garmin-mcp/.env`:

```sh
claude mcp add -s user garmin -- ~/.local/bin/garmin-mcp
```

If you built from source or put the binary elsewhere, use that path instead (`garmin-mcp setup` prints the right one).

Or pass the key directly instead:

```sh
claude mcp add -s user garmin -e GARMIN_API_KEY=<your-key> -- ~/.local/bin/garmin-mcp
```

Check the connection with `claude mcp list`, or with `/mcp` inside a session. Then ask, for example, "What were my last 3 runs?" or "How did I sleep last night?"

### Claude Desktop

Add the server to Claude Desktop's config file and restart the app. The file is `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS and `%APPDATA%\Claude\claude_desktop_config.json` on Windows (Settings → Developer → Edit Config opens it).

```json
{
  "mcpServers": {
    "garmin": {
      "command": "/Users/<you>/.local/bin/garmin-mcp"
    }
  }
}
```

On Windows the command is `C:\\Users\\<you>\\.local\\bin\\garmin-mcp.exe` (backslashes doubled in JSON). Use the binary's full path, without `~`: Claude Desktop doesn't use your shell's `PATH` or expand `~`. `garmin-mcp setup` prints this entry with your path filled in. With the key in `~/.config/garmin-mcp/.env`, no `"env"` block is needed. To pass the key here instead, add `"env": { "GARMIN_API_KEY": "<your-key>" }`.

### ChatGPT and Codex (OpenAI)

The ChatGPT **desktop app** and the **Codex CLI** can start local MCP servers; they share the config file `~/.codex/config.toml`. ChatGPT on the web and on phones can only reach remote servers over the internet, so this server doesn't work there.

- ChatGPT desktop app: Settings → MCP servers → add a server, choose **STDIO**, and enter the binary's full path as the command.
- Codex CLI:

  ```sh
  codex mcp add garmin -- ~/.local/bin/garmin-mcp
  ```

Or add it to `~/.codex/config.toml` by hand:

```toml
[mcp_servers.garmin]
command = "/Users/<you>/.local/bin/garmin-mcp"
```

Check it with `codex mcp list`, or `/mcp` inside Codex.

### Gemini CLI (Google)

```sh
gemini mcp add -s user garmin ~/.local/bin/garmin-mcp
```

`-s user` makes it available in every project (without it, the server is added to the current project only). Or add it to `~/.gemini/settings.json` by hand:

```json
{
  "mcpServers": {
    "garmin": {
      "command": "/Users/<you>/.local/bin/garmin-mcp"
    }
  }
}
```

Check it with `/mcp list` inside Gemini CLI. The Gemini web and mobile apps can't start local servers, so this server doesn't work there.

### Other clients

Cursor, VS Code (GitHub Copilot agent mode), Windsurf and most other MCP clients use the same entry: a server named `garmin` whose command is the binary's full path, with no arguments. For example:

- Cursor: add the JSON entry shown for Gemini CLI to `~/.cursor/mcp.json`.
- VS Code: `code --add-mcp '{"name":"garmin","command":"/Users/<you>/.local/bin/garmin-mcp"}'`.
- Windsurf: add the same JSON entry to `~/.codeium/windsurf/mcp_config.json`.

On Windows, use `C:\\Users\\<you>\\.local\\bin\\garmin-mcp.exe` in JSON, and the same path with single backslashes in TOML strings written with single quotes (`'C:\Users\<you>\.local\bin\garmin-mcp.exe'`).

With every client, the key in `~/.config/garmin-mcp/.env` is found automatically, so the entry needs no environment variables. A key passed in a client's config instead is stored there in plain text.

## How it works

1. On the first tool call, the server exchanges your API key for a bearer token:
   `POST /personalapitoken-service/personal-api/personal-api-key/token` with the header `Authorization: ApiKey <key>`.
2. The token is valid for 24 hours. It is kept in memory and refreshed 5 minutes before it expires.
3. Every API request is sent with `Authorization: Bearer <token>`. If Garmin answers `401`, the server fetches a new token and retries once.
4. Responses are reduced to the useful fields, which makes results about 80% smaller than Garmin's raw JSON.

The server talks to the client over stdin/stdout. Logs go to stderr.

## Updates

At startup, the server asks GitHub for the latest release, at most once a day. If a newer version exists, it adds a short note to the next tool result, once per session, so your AI app can tell you. The note says how to update, depending on how you installed it:

| Installed with | Update command |
|---|---|
| Homebrew | `brew upgrade garmin-mcp` |
| The install script | `garmin-mcp-update` |
| Built from source | `git pull`, then `cargo build --release` |
| Downloaded by hand | download the new release and replace the binary |

Then restart your AI app. Nothing is updated automatically.

The check runs in the background and never slows down a tool call. If GitHub can't be reached, nothing happens. The time of the last check is stored in `~/.config/garmin-mcp/update-check.json`. To turn the check off, set `GARMIN_MCP_NO_UPDATE_CHECK=1` in the environment or in `~/.config/garmin-mcp/.env`.

## Development

```sh
cargo build                 # debug build
cargo run                   # run on stdio (waits for an MCP client)
cargo test                  # unit tests (no network needed)
cargo clippy --all-targets  # lint
cargo fmt                   # format
```

### Project layout

```
src/
├── main.rs           entry point: runs the server on stdio, or `setup` / `--help` / `--version`
├── config.rs         reads GARMIN_API_KEY from env vars and .env files
├── setup.rs          `garmin-mcp setup`: checks and saves the API key
├── update.rs         checks GitHub for a newer release and adds a note to a tool result
├── server.rs         MCP tools, one #[tool] method each (see the table above)
└── garmin/
    ├── activity.rs         activity list entries (Activity)
    ├── activity_detail.rs  one activity's full details (ActivityDetail)
    ├── auth.rs             bearer token fetch and cache
    ├── body_battery.rs     Body Battery levels and events per day (BodyBatteryReport)
    ├── client.rs           authenticated HTTP requests and API calls
    ├── daily_summary.rs    daily wellness summaries (DailySummaries, UserDailySummary)
    ├── fitness_age.rs      Fitness Age and its inputs (FitnessAge)
    ├── heart_rate_day.rs   all-day heart rate readings (DailyHeartRate)
    ├── heart_rate_zones.rs heart rate zones per sport (HeartRateZones)
    ├── hrv.rs              heart rate variability per night (HrvRange, HrvNight)
    ├── sleep.rs            one night's sleep (DailySleep)
    ├── types.rs            shared unit helpers, ActivityTypeFilter
    └── weight.rs           weight, body composition and weight goal (WeightRange, WeightGoals)
```

To add a tool:
1. Add the API call to `garmin/client.rs`.
2. Add its response types in a new file under `garmin/`. Output every useful field Garmin returns (see CLAUDE.md, "Response fields").
3. Add a `#[tool]` method in `server.rs`.

The code is commented heavily, as a Rust learning resource.

### Releasing

Releases are built by [cargo-dist](https://github.com/axodotdev/cargo-dist) in GitHub Actions (`.github/workflows/release.yml`): pushing a tag like `v0.3.0` builds every platform, and publishes the archives, installers and the matching `CHANGELOG.md` section as a GitHub release. See CLAUDE.md, "Releases", for the steps.

### Garmin API docs

Garmin's [swagger docs](https://developer.garmin.com/gc-developer-program/personal/swagger-docs/) belong to Garmin and are not included in this repository. To get a local copy, run `python3 docs/update_garmin_spec.py`. It writes two git-ignored files:
- `docs/garmin-api.md`: a readable index of every endpoint, marking which ones this server implements and the MCP tool for each.
- `docs/garmin-openapi.json`: the full OpenAPI spec.

After adding a tool, run it with `--offline` to update only the implementation status from the local spec.

## License

[MIT](LICENSE)
