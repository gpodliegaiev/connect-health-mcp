//! Runtime configuration, read from environment variables and `.env` files.
//!
//! Each setting is looked up in this order; the first non-empty value wins:
//! 1. A real environment variable, e.g. set in the MCP client's server
//!    config (`claude mcp add -e ...`, `"env"` in Claude Desktop's config).
//! 2. A `.env` file in the current directory or one of its parents. Handy for
//!    development inside the repository.
//! 3. `garmin-mcp/.env` in the user's config folder: `~/.config` on macOS and
//!    Linux (`$XDG_CONFIG_HOME` if set), `%APPDATA%` on Windows. The
//!    recommended place for users: it doesn't depend on where the binary is
//!    or which directory the MCP client starts it from, and it survives
//!    updates and reinstalls.
//! 4. A `.env` file next to the binary, for users who just downloaded it.
//!
//! The files are only read, never loaded into the process environment, so
//! the lookup is the same everywhere and easy to test.

// `fmt` is the standard library's formatting module; we use it to write a
// custom `Debug` implementation below. `Path` is a borrowed file path (like
// `&str` for strings), `PathBuf` an owned one (like `String`). `OsString` is
// a string in the operating system's own encoding, which is what
// `env::var_os` returns (a path need not be valid UTF-8).
use std::{
    env,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

// `Context` is an `anyhow` trait that adds `.context("...")` to `Result` and
// `Option`, attaching a human-readable message to an error.
use anyhow::Context;

/// Base URL used when `GARMIN_BASE_URL` is not set.
// `const` is a compile-time constant. `&str` is a borrowed string slice; a
// string literal like this one lives for the whole program (`'static`).
const DEFAULT_BASE_URL: &str = "https://personal.connectapi.garmin.com";

/// Name of this app's folder inside the user's config folder.
const APP_DIR: &str = "garmin-mcp";

/// Settings the server needs to talk to the Garmin API.
// No `#[derive(Debug)]` here on purpose: the derived version would print the
// API key. We write our own `Debug` below that hides it.
#[derive(Clone)]
pub struct Config {
    /// Root of all API URLs, without a trailing slash.
    pub base_url: String,
    /// Personal API key, sent as `Authorization: ApiKey <key>` to get a
    /// bearer token. Secret: never log it.
    pub api_key: String,
}

impl Config {
    /// Loads the configuration from environment variables and `.env` files
    /// (see the module docs for the order).
    ///
    /// Returns an error if no `GARMIN_API_KEY` is found anywhere, so the
    /// server stops at startup instead of failing on the first tool call.
    pub fn from_env() -> anyhow::Result<Self> {
        // `|name| env::var(name).ok()` is a closure (an inline anonymous
        // function). `env::var` returns `Result<String, VarError>`; `.ok()`
        // turns it into an `Option<String>` (`None` if unset).
        Self::resolve(|name| env::var(name).ok(), &env_files())
    }

    /// Does the actual lookup. `env_var` reads one environment variable and
    /// `files` are the `.env` files in priority order.
    ///
    /// Taking these as arguments (instead of reading the real environment
    /// here) lets tests pass in their own values.
    ///
    /// `impl Fn(&str) -> Option<String>` means "any function or closure with
    /// this signature". `&[PathBuf]` is a borrowed slice: a view into a list
    /// owned by the caller.
    fn resolve(
        env_var: impl Fn(&str) -> Option<String>,
        files: &[PathBuf],
    ) -> anyhow::Result<Self> {
        // Read every file once, up front. Missing files give empty lists.
        // `.map(|path| read_env_file(path))` can be written as just
        // `.map(read_env_file)` because the function has the right shape.
        let contents: Vec<Vec<(String, String)>> =
            files.iter().map(|path| read_env_file(path)).collect();

        // Finds one setting. A closure can use variables around it, here
        // `env_var` and `contents`; it borrows them, so they stay usable.
        let lookup = |name: &str| -> Option<String> {
            // Removes surrounding spaces (easy to paste by accident) and
            // treats an empty value as missing.
            let clean = |value: &str| {
                let value = value.trim();
                (!value.is_empty()).then(|| value.to_string())
            };
            // `and_then` runs `clean` only if the variable is set.
            // `or_else` runs its closure only if the result so far is `None`:
            // then each file is searched in order. `.flatten()` walks through
            // the pairs of every file as one long list.
            env_var(name).and_then(|value| clean(&value)).or_else(|| {
                contents
                    .iter()
                    .flatten()
                    .filter(|(key, _)| key == name)
                    .find_map(|(_, value)| clean(value))
            })
        };

        // `.with_context(|| ...)` turns `None` into an error, building the
        // message only when needed; `?` returns that error early.
        let api_key = lookup("GARMIN_API_KEY").with_context(|| missing_key_message(files))?;

        // A missing base URL falls back to the default.
        // `.unwrap_or_else(...)` calls the closure only when the value is
        // `None`; `.to_string()` turns the `&str` constant into an owned
        // `String`.
        let base_url = lookup("GARMIN_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Self {
            // Strip a trailing `/` so we can always build URLs as
            // `format!("{base_url}{path}")` with `path` starting with `/`.
            base_url: base_url.trim_end_matches('/').to_string(),
            // Field init shorthand: `api_key` means `api_key: api_key`.
            api_key,
        })
    }
}

/// This app's folder inside the user's config folder, e.g.
/// `/Users/you/.config/garmin-mcp`. `None` only if the home folder is
/// unknown.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        env::var_os("APPDATA"),
        env::var_os("XDG_CONFIG_HOME"),
        env::home_dir(),
    )
}

/// The logic of [`config_dir`], with the environment passed in for tests.
///
/// - Windows: `%APPDATA%\garmin-mcp` (`C:\Users\you\AppData\Roaming\...`).
/// - macOS and Linux: `$XDG_CONFIG_HOME/garmin-mcp` if that variable is set
///   to an absolute path (the XDG standard says to ignore a relative one),
///   otherwise `~/.config/garmin-mcp`. macOS's native folder would be
///   `~/Library/Application Support`, but `~/.config` is easier to type and
///   is what many command-line tools use there too.
fn config_dir_from(
    appdata: Option<OsString>,
    xdg_config_home: Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    // `cfg!(windows)` is `true` when compiling for Windows. Unlike
    // `#[cfg(...)]`, both branches are still compiled (and type-checked).
    let base = if cfg!(windows) {
        appdata.map(PathBuf::from)
    } else {
        xdg_config_home
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.map(|home| home.join(".config")))
    };
    base.map(|base| base.join(APP_DIR))
}

/// The `.env` files to read, in priority order (sources 2 to 4 in the module
/// docs). Files that don't exist are still listed, so the "key not found"
/// error can say where it looked.
fn env_files() -> Vec<PathBuf> {
    let mut files = Vec::new();

    // 2. The working directory or its closest parent that has a `.env`.
    // `.ancestors()` yields the folder itself, then its parent, and so on up
    // to the root. If none has one, list the working directory's.
    if let Ok(cwd) = env::current_dir() {
        let found = cwd
            .ancestors()
            .map(|dir| dir.join(".env"))
            .find(|path| path.is_file());
        files.push(found.unwrap_or_else(|| cwd.join(".env")));
    }

    // 3. The user's config folder.
    if let Some(dir) = config_dir() {
        files.push(dir.join(".env"));
    }

    // 4. Next to the binary. `current_exe()` is the running program's path,
    // e.g. `/Users/you/.local/bin/garmin-mcp`; `.parent()` is its folder.
    if let Some(dir) = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        files.push(dir.join(".env"));
    }

    // Drop repeats (e.g. running the binary from inside its own folder),
    // keeping the first, higher-priority copy. `retain` keeps the items for
    // which the closure returns `true`.
    let mut seen = Vec::new();
    files.retain(|path| {
        let new = !seen.contains(path);
        seen.push(path.clone());
        new
    });
    files
}

/// Reads the `KEY=value` pairs of one `.env` file. A missing file gives an
/// empty list. Lines that can't be parsed are skipped with a warning on
/// stderr.
fn read_env_file(path: &Path) -> Vec<(String, String)> {
    if !path.is_file() {
        return Vec::new();
    }
    // `from_path_iter` parses the file without touching the process
    // environment. Each item is a `Result`, one per variable.
    match dotenvy::from_path_iter(path) {
        Ok(items) => items
            .filter_map(|item| match item {
                Ok(pair) => Some(pair),
                // The parse error contains the line's text, which may be the
                // key itself, so only the path is printed.
                Err(_) => {
                    eprintln!(
                        "warning: skipped a line of {} that could not be parsed",
                        path.display()
                    );
                    None
                }
            })
            .collect(),
        Err(_) => {
            eprintln!("warning: could not read {}", path.display());
            Vec::new()
        }
    }
}

/// The error shown when no key is found: every place that was checked, and
/// how to fix it.
fn missing_key_message(files: &[PathBuf]) -> String {
    let mut message = String::from(
        "GARMIN_API_KEY is not set. Looked in:\n  - the GARMIN_API_KEY environment variable",
    );
    for path in files {
        // `push_str` appends to the `String` in place (it needs `mut`).
        message.push_str(&format!("\n  - {}", path.display()));
    }
    let suggested = config_dir()
        .map(|dir| dir.join(".env").display().to_string())
        .unwrap_or_else(|| "~/.config/garmin-mcp/.env".to_string());
    message.push_str(&format!(
        "\nAdd a line GARMIN_API_KEY=<your key> to {suggested}, or pass it in the MCP client config."
    ));
    message
}

/// Custom `Debug` output that hides the API key.
///
/// Implementing a trait by hand (instead of `#[derive]`) lets us control
/// exactly what it does. `{:?}` on a `Config` now prints the key as `***`.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `debug_struct` is a helper that prints `Config { field: value, .. }`.
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("api_key", &"***")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Writes `content` to a fresh file in the system temp folder and returns
    /// its path. The process id keeps parallel test runs apart.
    fn temp_env_file(name: &str, content: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("garmin-mcp-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    /// An environment with no variables set.
    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn env_var_wins_over_files() {
        let file = temp_env_file("env_wins", "GARMIN_API_KEY=from-file\n");
        let env = |name: &str| (name == "GARMIN_API_KEY").then(|| "from-env".to_string());
        let config = Config::resolve(env, &[file]).unwrap();
        assert_eq!(config.api_key, "from-env");
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn first_file_with_a_value_wins() {
        let missing = env::temp_dir().join("garmin-mcp-test-does-not-exist/.env");
        let empty = temp_env_file("first_empty", "GARMIN_API_KEY=\n");
        let second = temp_env_file(
            "first_second",
            "# comment\nGARMIN_API_KEY=  second  \nGARMIN_BASE_URL=http://localhost:9000/\n",
        );
        let third = temp_env_file("first_third", "GARMIN_API_KEY=third\n");
        // An empty env var is treated as missing too.
        let env = |name: &str| (name == "GARMIN_API_KEY").then(String::new);
        let config = Config::resolve(env, &[missing, empty, second, third]).unwrap();
        // Spaces are trimmed; the trailing `/` of the URL is removed.
        assert_eq!(config.api_key, "second");
        assert_eq!(config.base_url, "http://localhost:9000");
    }

    #[test]
    fn missing_key_lists_every_place_checked() {
        let missing = env::temp_dir().join("garmin-mcp-test-does-not-exist/.env");
        let other = temp_env_file("missing_other", "GARMIN_BASE_URL=http://x\n");
        let error = Config::resolve(no_env, &[missing.clone(), other.clone()]).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("the GARMIN_API_KEY environment variable"));
        assert!(message.contains(&missing.display().to_string()));
        assert!(message.contains(&other.display().to_string()));
    }

    #[test]
    fn debug_hides_the_key() {
        let file = temp_env_file("debug", "GARMIN_API_KEY=secret-value\n");
        let config = Config::resolve(no_env, &[file]).unwrap();
        assert!(!format!("{config:?}").contains("secret-value"));
    }

    // The Windows branch is only checked when compiling for Windows.
    #[cfg(not(windows))]
    #[test]
    fn config_dir_prefers_absolute_xdg_then_home() {
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(
            config_dir_from(None, None, home.clone()),
            Some(PathBuf::from("/home/me/.config/garmin-mcp"))
        );
        assert_eq!(
            config_dir_from(None, Some("/xdg".into()), home.clone()),
            Some(PathBuf::from("/xdg/garmin-mcp"))
        );
        // A relative XDG_CONFIG_HOME is ignored, as the standard says.
        assert_eq!(
            config_dir_from(None, Some("relative".into()), home),
            Some(PathBuf::from("/home/me/.config/garmin-mcp"))
        );
        assert_eq!(config_dir_from(None, None, None), None);
    }
}
