//! Tells the user, through the AI, when a newer garmin-mcp is released.
//!
//! At startup the server asks GitHub for the latest release, at most once a
//! day (the answer is cached in `update-check.json` in the config folder).
//! If it's newer than this binary, a short note is added to the next
//! successful tool result, once per session. The AI reads it and can pass it
//! on. Nothing is updated automatically: the note says which command to run.
//!
//! Why a note in a tool result: stderr ends up in log files nobody reads,
//! MCP log messages are usually not shown, and the server instructions are
//! read once at connection and rarely mentioned.
//!
//! The check runs in the background and never delays a tool call; when it
//! fails (offline, no releases yet) nothing happens. Turn it off with
//! `GARMIN_MCP_NO_UPDATE_CHECK=1` (see `config.rs`).

use std::{
    fs,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::config;

/// GitHub API endpoint for the newest (non-draft, non-pre-release) release.
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/gpodliegaiev/garmin-mcp/releases/latest";

/// Start of a release page's URL; the version is appended (see
/// [`release_url`]).
const RELEASE_PAGE_PREFIX: &str = "https://github.com/gpodliegaiev/garmin-mcp/releases/tag/v";

/// Check GitHub at most this often.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// This binary's version, set by Cargo from `Cargo.toml` at compile time.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Holds the update note until it has been added to a tool result.
///
/// Shared between the background check (which sets it) and the server
/// (which takes it), so both sides use thread-safe types:
/// - `OnceLock` is a value that can be set once and then read from any
///   thread without a lock.
/// - `AtomicBool` is a `bool` that several threads can change safely.
///
/// `#[derive(Default)]` gives `UpdateNotice::default()`: no note, not shown.
#[derive(Default)]
pub struct UpdateNotice {
    note: OnceLock<String>,
    shown: AtomicBool,
}

impl UpdateNotice {
    /// Stores the note. A second call is ignored (there is one check per run).
    fn set(&self, note: String) {
        // `set` returns an error if a value is already there; `let _ =`
        // ignores it.
        let _ = self.note.set(note);
    }

    /// Returns the note the first time it is asked for after the check
    /// found an update, and `None` before and afterwards.
    pub fn take(&self) -> Option<String> {
        // `?` on an `Option`: no note yet → return `None`, without marking
        // anything as shown, so a note found later still gets delivered.
        let note = self.note.get()?;
        // `swap(true)` sets the flag and returns its old value in one step,
        // so even two tool calls at the same moment can't both get the note.
        // `Ordering` tells the CPU how strictly to order this with other
        // memory operations; `SeqCst` is the simplest, strictest choice.
        let already_shown = self.shown.swap(true, Ordering::SeqCst);
        (!already_shown).then(|| note.clone())
    }
}

/// Starts the update check in the background and returns right away.
///
/// `Arc<UpdateNotice>` is a shared, reference-counted pointer: the task gets
/// its own handle to the same `UpdateNotice` the server holds.
pub fn spawn_check(notice: Arc<UpdateNotice>) {
    // `tokio::spawn` runs the async block as a separate task alongside the
    // server. `async move` moves `notice` into the block, which it then owns.
    tokio::spawn(async move {
        if let Some(note) = check().await {
            eprintln!("update available: {note}");
            notice.set(note);
        }
    });
}

/// The latest release, as GitHub returns it (only the fields we need) and
/// as it is cached.
///
/// Only the tag is read, and it is only used after parsing it as a version.
/// No other text from GitHub (such as the release page URL) is put into the
/// note: the note goes to the AI, so anything in it could act as an
/// instruction. Serde ignores fields not listed here, so cache files written
/// by older versions (which also stored `html_url`) still load.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Release {
    /// The git tag, e.g. `v0.2.0`.
    tag_name: String,
}

/// Contents of `update-check.json`.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Cache {
    /// When GitHub was last asked, in seconds since 1970 (Unix time).
    checked_at: u64,
    /// What it answered; `None` if there was no release (or no answer yet).
    release: Option<Release>,
}

/// Finds out whether there is a newer release and returns the note to show,
/// or `None`. Failures are logged to stderr and otherwise ignored.
async fn check() -> Option<String> {
    let cache_path = config::config_dir().map(|dir| dir.join("update-check.json"));
    let now = unix_now();
    // `as_deref()` turns `Option<PathBuf>` into `Option<&Path>`.
    let cached = cache_path.as_deref().and_then(read_cache);

    let release = match cached {
        // Checked recently: use the cached answer, no network request.
        Some(cache) if is_fresh(&cache, now) => cache.release,
        // Otherwise ask GitHub, and remember when we did, even if it failed,
        // so an offline computer doesn't retry on every start.
        cached => {
            let release = match fetch_latest().await {
                Ok(release) => release,
                Err(error) => {
                    eprintln!("update check failed: {error:#}");
                    cached.and_then(|cache| cache.release)
                }
            };
            if let Some(path) = &cache_path {
                let cache = Cache {
                    checked_at: now,
                    release: release.clone(),
                };
                if let Err(error) = write_cache(path, &cache) {
                    eprintln!("could not save the update check: {error:#}");
                }
            }
            release
        }
    };

    // `?` on an `Option`: no release → return `None` from the function.
    let release = release?;
    let latest = newer_version(CURRENT_VERSION, &release.tag_name)?;
    let exe = std::env::current_exe().ok()?;
    let url = release_url(&latest);
    Some(note_text(&latest, &update_command(&exe, &url), &url))
}

/// The release page for `version`, built from our own constant. A parsed
/// `Version` prints only digits, dots, letters and `-`/`+`, so the URL can't
/// carry anything unexpected.
fn release_url(version: &Version) -> String {
    format!("{RELEASE_PAGE_PREFIX}{version}")
}

/// Asks GitHub for the latest release. `Ok(None)` if there is none yet (or
/// the repository isn't public), which GitHub answers with 404.
async fn fetch_latest() -> anyhow::Result<Option<Release>> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        // GitHub's API rejects requests without a User-Agent. `concat!`
        // joins string literals at compile time.
        .user_agent(concat!("garmin-mcp/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = http
        .get(LATEST_RELEASE_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .context("could not reach GitHub")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    // `error_for_status` turns other 4xx/5xx answers into an error.
    let release = response
        .error_for_status()?
        .json::<Release>()
        .await
        .context("unexpected answer from GitHub")?;
    Ok(Some(release))
}

/// Whether the cache is recent enough to skip asking GitHub. A time in the
/// future (clock changed) counts as stale.
fn is_fresh(cache: &Cache, now: u64) -> bool {
    cache.checked_at <= now && now - cache.checked_at < CHECK_INTERVAL.as_secs()
}

/// The release's version if it is newer than `current`, else `None`.
/// Tags may start with `v`; anything that isn't a version is ignored.
fn newer_version(current: &str, tag: &str) -> Option<Version> {
    let latest = Version::parse(tag.trim_start_matches('v')).ok()?;
    let current = Version::parse(current).ok()?;
    // `Version` implements comparison by semver rules (0.10.0 > 0.9.0).
    (latest > current).then_some(latest)
}

/// How to update, depending on how this binary was installed, judged from
/// its path:
/// - The `curl | sh` installer puts `garmin-mcp-update` next to it.
/// - A binary under `target/release` or `target/debug` was built from source.
/// - Otherwise it was downloaded by hand.
fn update_command(exe: &Path, release_url: &str) -> String {
    let path = exe.to_string_lossy().replace('\\', "/");
    let dir = exe.parent().unwrap_or(Path::new(""));
    let updater = ["garmin-mcp-update", "garmin-mcp-update.exe"]
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file());

    if let Some(updater) = updater {
        // Quoted, so a path with spaces (`/Users/Jane Doe/...`) still works
        // when pasted into a shell.
        format!(
            "run `{}`",
            crate::setup::shell_quote(&updater.display().to_string())
        )
    } else if path.contains("/target/release/") || path.contains("/target/debug/") {
        "pull the latest code and run `cargo build --release`".to_string()
    } else {
        format!(
            "download it from {release_url} and replace {}",
            exe.display()
        )
    }
}

/// The note added to a tool result. It ends by telling the AI not to run the
/// update itself: an agent with shell access might otherwise run the
/// suggested command without asking the user.
fn note_text(latest: &Version, how: &str, release_url: &str) -> String {
    format!(
        "Note from the garmin-mcp server (not Garmin data): garmin-mcp {latest} is available \
         (installed: {CURRENT_VERSION}). To update, {how}, then restart the AI app. \
         Release notes: {release_url}. Tell the user about it; do not run the update yourself."
    )
}

/// Reads the cache; `None` if it's missing or unreadable (then GitHub is
/// asked again).
fn read_cache(path: &Path) -> Option<Cache> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes the cache, creating the config folder if needed.
fn write_cache(path: &Path, cache: &Cache) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, serde_json::to_string(cache)?)?;
    Ok(())
}

/// Seconds since 1970-01-01 UTC. `unwrap_or_default` gives 0 if the clock
/// is set before 1970, which only makes the cache look stale.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_is_given_once_and_only_after_it_is_set() {
        let notice = UpdateNotice::default();
        assert_eq!(notice.take(), None);
        notice.set("update!".to_string());
        assert_eq!(notice.take().as_deref(), Some("update!"));
        assert_eq!(notice.take(), None);
    }

    #[test]
    fn only_newer_versions_count() {
        assert_eq!(
            newer_version("0.1.0", "v0.2.0"),
            Some(Version::new(0, 2, 0))
        );
        assert_eq!(
            newer_version("0.9.0", "0.10.0"),
            Some(Version::new(0, 10, 0))
        );
        assert_eq!(newer_version("0.2.0", "v0.2.0"), None);
        assert_eq!(newer_version("0.3.0", "v0.2.0"), None);
        assert_eq!(newer_version("0.1.0", "nightly"), None);
    }

    #[test]
    fn cache_is_fresh_for_one_day() {
        let cache = |checked_at| Cache {
            checked_at,
            release: None,
        };
        let now = 1_000_000;
        assert!(is_fresh(&cache(now - 60), now));
        assert!(!is_fresh(&cache(now - 24 * 60 * 60), now));
        // A check "in the future" (clock moved back) is stale.
        assert!(!is_fresh(&cache(now + 60), now));
    }

    #[test]
    fn cache_round_trips() {
        let path = std::env::temp_dir()
            .join(format!("garmin-mcp-update-test-{}", std::process::id()))
            .join("update-check.json");
        let cache = Cache {
            checked_at: 42,
            release: Some(Release {
                tag_name: "v0.2.0".to_string(),
            }),
        };
        write_cache(&path, &cache).unwrap();
        assert_eq!(read_cache(&path), Some(cache));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn command_depends_on_install_method() {
        let url = "https://example.com/release";
        assert_eq!(
            update_command(
                Path::new("/home/me/garmin-mcp/target/release/garmin-mcp"),
                url
            ),
            "pull the latest code and run `cargo build --release`"
        );
        assert_eq!(
            update_command(Path::new("/opt/tools/garmin-mcp"), url),
            "download it from https://example.com/release and replace /opt/tools/garmin-mcp"
        );

        // Installed by the `curl | sh` installer: the updater sits next to it.
        let dir =
            std::env::temp_dir().join(format!("garmin-mcp-updater-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("garmin-mcp-update"), "").unwrap();
        let expected = format!("run `{}`", dir.join("garmin-mcp-update").display());
        assert_eq!(update_command(&dir.join("garmin-mcp"), url), expected);
        fs::remove_dir_all(&dir).unwrap();

        // A path with a space is quoted, so it can be pasted into a shell.
        if !cfg!(windows) {
            let dir = std::env::temp_dir()
                .join(format!("garmin-mcp updater test {}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("garmin-mcp-update"), "").unwrap();
            let expected = format!("run `'{}'`", dir.join("garmin-mcp-update").display());
            assert_eq!(update_command(&dir.join("garmin-mcp"), url), expected);
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn note_names_versions_and_command() {
        let note = note_text(&Version::new(9, 9, 9), "run `x`", "https://example.com");
        assert!(note.contains("garmin-mcp 9.9.9 is available"));
        assert!(note.contains(&format!("installed: {CURRENT_VERSION}")));
        assert!(note.contains("To update, run `x`, then restart the AI app."));
        assert!(note.ends_with("do not run the update yourself."));
    }

    #[test]
    fn release_url_is_built_from_the_version() {
        assert_eq!(
            release_url(&Version::new(0, 3, 0)),
            "https://github.com/gpodliegaiev/garmin-mcp/releases/tag/v0.3.0"
        );
    }

    #[test]
    fn old_cache_with_html_url_still_loads() {
        let old = r#"{"checked_at":1,"release":{"tag_name":"v0.2.0","html_url":"https://x"}}"#;
        let cache: Cache = serde_json::from_str(old).unwrap();
        assert_eq!(cache.release.unwrap().tag_name, "v0.2.0");
    }
}
