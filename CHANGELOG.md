# Changelog

All notable changes to garmin-mcp are listed here. Versions follow [Semantic Versioning](https://semver.org/).

The release workflow copies the section for a version into its GitHub release notes, so each heading must be the plain version number (`## 0.2.0`).

## Unreleased

## 0.2.0 - 2026-11-09

First public release, with ready-made binaries for macOS (Apple Silicon and Intel), Linux (x86_64) and Windows (x86_64).

### Install

- One-line installers: `garmin-mcp-installer.sh` for macOS and Linux, `garmin-mcp-installer.ps1` for Windows. They put `garmin-mcp` and `garmin-mcp-update` in `~/.local/bin`.
- `garmin-mcp setup` asks for your Garmin personal API key with hidden input, checks it with Garmin, and saves it to `~/.config/garmin-mcp/.env` (`%APPDATA%\garmin-mcp\.env` on Windows), readable only by you. It then prints ready-to-paste entries for Claude Code and Claude Desktop.
- Every archive has a signed build attestation: `gh attestation verify <archive> -R gpodliegaiev/garmin-mcp`.

### Features

- Tools: `list_activities`, `get_activity`, `get_sleep`, `get_fitness_age`, `get_hrv`, `get_daily_summary`, `get_heart_rate_zones`, `get_weight`, `get_body_battery`, `get_heart_rate_day`.
- The API key is read from the `GARMIN_API_KEY` environment variable or a `.env` file (working folder, `~/.config/garmin-mcp/`, or next to the binary).
- Update notice: once a day the server checks GitHub for a newer release. If there is one, the first tool result of a session includes a short note for the AI to pass on, with the update command for your install method. Nothing is updated automatically. Turn the check off with `GARMIN_MCP_NO_UPDATE_CHECK=1`.
- `garmin-mcp --version` and `garmin-mcp --help`.

### Notes

- Unofficial: not affiliated with or endorsed by Garmin. See the README's "Privacy and disclaimer" section for where your data goes.
- License notices of the bundled open-source libraries are in `THIRD-PARTY-LICENSES.html` in each archive.
