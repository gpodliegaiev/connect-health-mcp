//! Runtime configuration, read from environment variables and `.env` files.
//!
//! Settings: `GARMIN_API_KEY` (required) and `GARMIN_MCP_NO_UPDATE_CHECK`
//! (optional, turns off the update check). The Garmin API address is fixed
//! in `garmin/client.rs`, not a setting: source 2 below can be a `.env` in
//! any project the user opens, and must never be able to redirect the key.
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

/// Name of this app's folder inside the user's config folder.
// `const` is a compile-time constant. `&str` is a borrowed string slice; a
// string literal like this one lives for the whole program (`'static`).
const APP_DIR: &str = "garmin-mcp";

/// Settings the server needs to talk to the Garmin API.
// No `#[derive(Debug)]` here on purpose: the derived version would print the
// API key. We write our own `Debug` below that hides it.
#[derive(Clone)]
pub struct Config {
    /// Personal API key, sent as `Authorization: ApiKey <key>` to get a
    /// bearer token. Secret: never log it.
    pub api_key: String,
    /// Whether to look for a newer release at startup (see `update.rs`).
    /// `false` when `GARMIN_MCP_NO_UPDATE_CHECK` is set to anything but
    /// `0`, `false`, `no` or `off`.
    pub update_check: bool,
}

impl Config {
    /// Loads the configuration from environment variables and `.env` files
    /// (see the module docs for the order).
    ///
    /// Returns an error if no `GARMIN_API_KEY` is found anywhere, so the
    /// server stops at startup instead of failing on the first tool call.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::resolve(real_env_var, &env_files())
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
        let sources = Sources::new(env_var, files);

        // `.map(|(value, _)| value)` keeps the value and drops where it came
        // from (`_` ignores a part when destructuring a tuple).
        // `.with_context(|| ...)` turns `None` into an error, building the
        // message only when needed; `?` returns that error early.
        let api_key = sources
            .find("GARMIN_API_KEY")
            .map(|(value, _)| value)
            .with_context(|| missing_key_message(files))?;

        Ok(Self {
            // Field init shorthand: `api_key` means `api_key: api_key`.
            api_key,
            // `is_none_or` is `true` when the setting is missing or the
            // closure returns `true` for its value.
            update_check: sources
                .find("GARMIN_MCP_NO_UPDATE_CHECK")
                .is_none_or(|(value, _)| is_off(&value)),
        })
    }
}

/// Where a setting's value was found. Used by `garmin-mcp setup` to warn
/// when another key takes priority over the one it saved.
#[derive(Debug, PartialEq)]
pub enum Source {
    /// A real environment variable, with its name.
    EnvVar(&'static str),
    /// A `.env` file.
    File(PathBuf),
}

/// How a [`Source`] is shown to the user, e.g. in "the key from ... takes
/// priority". Implementing `Display` is what makes `{source}` work in
/// `format!` and `eprintln!`.
impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `match` on `self` picks the branch for the variant we have, and
        // binds its contents (`name`, `path`) to variables.
        match self {
            Source::EnvVar(name) => write!(f, "the {name} environment variable"),
            Source::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// Where the API key would currently be read from, or `None` if there is
/// none. The key itself is not returned.
pub fn api_key_source() -> Option<Source> {
    Sources::new(real_env_var, &env_files())
        .find("GARMIN_API_KEY")
        .map(|(_, source)| source)
}

/// Reads a real environment variable. `env::var` returns
/// `Result<String, VarError>`; `.ok()` turns it into an `Option<String>`
/// (`None` if unset).
fn real_env_var(name: &str) -> Option<String> {
    env::var(name).ok()
}

/// Every place settings are read from, with the `.env` files already parsed.
///
/// `<F>` makes the struct *generic*: `F` stands for whatever function type is
/// passed in as `env_var` (a real lookup or a test closure). The `where`
/// clause on the `impl` says what `F` must be able to do.
struct Sources<F> {
    env_var: F,
    /// Each file's path with its `KEY=value` pairs, in priority order.
    files: Vec<(PathBuf, Vec<(String, String)>)>,
}

impl<F> Sources<F>
where
    F: Fn(&str) -> Option<String>,
{
    /// Reads every file once, up front. Missing files give empty lists.
    fn new(env_var: F, paths: &[PathBuf]) -> Self {
        let files = paths
            .iter()
            .map(|path| (path.clone(), read_env_file(path)))
            .collect();
        Self { env_var, files }
    }

    /// The first non-empty value of setting `name`, and where it was found:
    /// the environment variable first, then each file in order.
    fn find(&self, name: &'static str) -> Option<(String, Source)> {
        // `(self.env_var)(name)` calls the function stored in the field (the
        // parentheses tell Rust it's a field, not a method named `env_var`).
        // `and_then` runs `clean` only if the variable is set.
        if let Some(value) = (self.env_var)(name).and_then(|value| clean(&value)) {
            return Some((value, Source::EnvVar(name)));
        }
        // `find_map` returns the first `Some` the closure produces. The
        // outer one walks the files, the inner one that file's pairs.
        self.files.iter().find_map(|(path, pairs)| {
            pairs
                .iter()
                .filter(|(key, _)| key == name)
                .find_map(|(_, value)| clean(value))
                .map(|value| (value, Source::File(path.clone())))
        })
    }
}

/// Removes surrounding spaces (easy to paste by accident) and treats an
/// empty value as missing.
fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    // `bool::then` gives `Some(...)` when the condition is true, else `None`.
    (!value.is_empty()).then(|| value.to_string())
}

/// Whether a flag's value means "off" (`0`, `false`, `no`, `off`, any case).
fn is_off(value: &str) -> bool {
    matches!(value.to_lowercase().as_str(), "0" | "false" | "no" | "off")
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
        "\nRun `garmin-mcp setup` to save your key to {suggested}, \
         or pass GARMIN_API_KEY in the MCP client config."
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
            .field("api_key", &"***")
            .field("update_check", &self.update_check)
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
    }

    #[test]
    fn first_file_with_a_value_wins() {
        let missing = env::temp_dir().join("garmin-mcp-test-does-not-exist/.env");
        let empty = temp_env_file("first_empty", "GARMIN_API_KEY=\n");
        let second = temp_env_file("first_second", "# comment\nGARMIN_API_KEY=  second  \n");
        let third = temp_env_file("first_third", "GARMIN_API_KEY=third\n");
        // An empty env var is treated as missing too.
        let env = |name: &str| (name == "GARMIN_API_KEY").then(String::new);
        let config = Config::resolve(env, &[missing, empty, second, third]).unwrap();
        // Spaces are trimmed.
        assert_eq!(config.api_key, "second");
    }

    #[test]
    fn find_reports_where_the_value_came_from() {
        let file = temp_env_file("source", "GARMIN_API_KEY=from-file\n");
        let from_file = Sources::new(no_env, std::slice::from_ref(&file));
        assert_eq!(
            from_file.find("GARMIN_API_KEY"),
            Some(("from-file".to_string(), Source::File(file.clone())))
        );
        let env = |_: &str| Some("from-env".to_string());
        let from_env = Sources::new(env, &[file]);
        assert_eq!(
            from_env.find("GARMIN_API_KEY"),
            Some(("from-env".to_string(), Source::EnvVar("GARMIN_API_KEY")))
        );
    }

    #[test]
    fn missing_key_lists_every_place_checked() {
        let missing = env::temp_dir().join("garmin-mcp-test-does-not-exist/.env");
        let other = temp_env_file("missing_other", "GARMIN_MCP_NO_UPDATE_CHECK=1\n");
        let error = Config::resolve(no_env, &[missing.clone(), other.clone()]).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("the GARMIN_API_KEY environment variable"));
        assert!(message.contains(&missing.display().to_string()));
        assert!(message.contains(&other.display().to_string()));
    }

    #[test]
    fn update_check_is_on_unless_turned_off() {
        let key = temp_env_file("update_check", "GARMIN_API_KEY=k\n");
        let with = |value: &'static str| {
            let env =
                move |name: &str| (name == "GARMIN_MCP_NO_UPDATE_CHECK").then(|| value.to_string());
            Config::resolve(env, std::slice::from_ref(&key))
                .unwrap()
                .update_check
        };
        assert!(
            Config::resolve(no_env, std::slice::from_ref(&key))
                .unwrap()
                .update_check
        );
        assert!(!with("1"));
        assert!(!with("true"));
        assert!(with("0"));
        assert!(with("False"));
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
