# Changelog

All notable changes to connect-health-mcp are listed here. Versions follow [Semantic Versioning](https://semver.org/).

The release workflow copies the section for a version into its GitHub release notes, so each heading must be the plain version number (`## 0.2.0`).

## Unreleased

## 0.2.0 - 2026-11-10

First public release, with ready-made binaries for macOS (Apple Silicon and Intel), Linux (x86_64) and Windows (x86_64).

### Install

- One-line installers: `connect-health-mcp-installer.sh` for macOS and Linux, `connect-health-mcp-installer.ps1` for Windows. They put `connect-health-mcp` and `connect-health-mcp-update` in `~/.local/bin`.
- `connect-health-mcp setup` asks for your Garmin personal API key with hidden input, checks it with Garmin, and saves it to `~/.config/connect-health-mcp/.env` (`%APPDATA%\connect-health-mcp\.env` on Windows), readable only by you. It then prints ready-to-paste entries for Claude Code and Claude Desktop.
- Every archive has a signed build attestation: `gh attestation verify <archive> -R gpodliegaiev/connect-health-mcp`.

### Features

- Tools: `list_activities`, `get_activity`, `get_sleep`, `get_fitness_age`, `get_hrv`, `get_daily_summary`, `get_heart_rate_zones`, `get_weight`, `get_body_battery`, `get_heart_rate_day`.
- The API key is read from the `GARMIN_API_KEY` environment variable or a `.env` file in `~/.config/connect-health-mcp/` or next to the binary. A `.env` in the folder the server is started from is read only by development builds, so a project you open can't swap in another Garmin key.
- Update notice: once a day the server checks GitHub for a newer release. If there is one, the first tool result of a session includes a short note for the AI to pass on, with the update command for your install method. Nothing is updated automatically. Turn the check off with `CONNECT_HEALTH_MCP_NO_UPDATE_CHECK=1`.
- `connect-health-mcp --version` and `connect-health-mcp --help`.

### Notes

- Unofficial: not affiliated with or endorsed by Garmin. See the README's "Privacy and disclaimer" section for where your data goes.
- License notices of the code built into the binary are in each archive: `THIRD-PARTY-LICENSES.html` (Rust crates), `RUST-STD-LICENSES.html` (Rust's standard library) and `MUSL-LICENSE.txt` (musl libc, in the Linux build).
