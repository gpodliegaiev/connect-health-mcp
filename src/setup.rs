//! `garmin-mcp setup`: saves the user's API key, so nobody has to create a
//! `.env` file by hand.
//!
//! The steps:
//! 1. Ask for the key with hidden input, so it doesn't show on screen or end
//!    up in the shell history.
//! 2. Check it with Garmin (one token request), so a mistyped key is caught
//!    now instead of as a failing tool later.
//! 3. Save it to `garmin-mcp/.env` in the user's config folder (see
//!    `config.rs`), readable only by the user. Other settings already in
//!    that file are kept.
//! 4. Print how to add the server to Claude Code and Claude Desktop, with
//!    the binary's full path.
//!
//! Everything is printed to stderr, like the rest of the program, so stdout
//! stays reserved for the MCP protocol. The key is never printed.

use std::{
    env, fs,
    io::{self, Write},
    path::Path,
};

use anyhow::{Context, bail};
use reqwest::StatusCode;

use crate::{
    config::{self, Config, Source},
    garmin::{GarminClient, auth::KeyCheck},
};

/// Garmin's guide to creating a personal API key.
const KEY_GUIDE_URL: &str =
    "https://developer.garmin.com/gc-developer-program/personal/personal-api/";

/// Name of the setting this command writes.
const KEY_NAME: &str = "GARMIN_API_KEY";

/// Runs the whole setup. Returns an error (printed by `main`) if the key is
/// rejected or the file can't be written.
pub async fn run() -> anyhow::Result<()> {
    let dir = config::config_dir()
        .context("could not find your home folder, so there is nowhere to save the key")?;
    let path = dir.join(".env");

    // Read the current file, if any, to keep its other settings. A missing
    // file is normal (first setup); any other read error stops here.
    // `Err(e) if ...` is a match *guard*: the branch only matches when the
    // condition holds.
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("could not read {}", path.display())),
    };

    eprintln!(
        "This saves your Garmin personal API key to {}.",
        path.display()
    );
    eprintln!("To get a key, see {KEY_GUIDE_URL}\n");

    if has_key(&existing) && !confirm("A key is already saved there. Replace it?")? {
        eprintln!("Kept the saved key.");
        return Ok(());
    }

    // `prompt_password` reads from the terminal with echo turned off.
    let key = rpassword::prompt_password("Garmin personal API key (input hidden): ")
        .context("could not read the key from the terminal")?;
    // Shadowing: a new `key` variable replaces the old one, here with the
    // trimmed text.
    let key = key.trim().to_string();
    check_format(&key)?;

    // Check the key with the same client the server uses. `eprint!` (no
    // `ln`) leaves the cursor on the line, so the result follows it.
    eprint!("Checking the key with Garmin... ");
    let config = Config {
        api_key: key.clone(),
        // Not used by the key check.
        update_check: false,
    };
    // `true` if Garmin confirmed the key, `false` if it couldn't be checked.
    let checked = match GarminClient::new(&config)?.check_key().await {
        Ok(KeyCheck::Valid) => {
            eprintln!("OK");
            true
        }
        Ok(KeyCheck::Rejected(status)) if is_bad_key(status) => {
            eprintln!("rejected (HTTP {status})");
            bail!(
                "Garmin did not accept this key. Check that it was copied completely, \
                 then run `garmin-mcp setup` again."
            );
        }
        Ok(KeyCheck::Rejected(status)) => {
            eprintln!("Garmin answered HTTP {status}");
            false
        }
        // `{error:#}` prints the error with its causes on one line.
        Err(error) => {
            eprintln!("failed: {error:#}");
            false
        }
    };
    if !checked && !confirm("The key could not be checked. Save it anyway?")? {
        eprintln!("Nothing was saved.");
        return Ok(());
    }

    save(&dir, &path, &with_key(&existing, &key))?;
    eprintln!("Saved to {}", path.display());

    warn_if_overridden(&path);
    print_next_steps();
    Ok(())
}

/// Asks a yes/no question in the terminal. Anything but `y` or `yes`
/// (including just Enter) means no.
fn confirm(question: &str) -> anyhow::Result<bool> {
    eprint!("{question} [y/N] ");
    // stderr may hold back text without a newline; `flush` shows it now.
    io::stderr().flush()?;
    let mut answer = String::new();
    // `&mut answer` lends the string to `read_line` so it can append to it.
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Whether a `.env` file's text already sets a non-empty key.
fn has_key(content: &str) -> bool {
    // `from_read_iter` parses text from any reader; `.as_bytes()` lets a
    // `&str` act as one. `.flatten()` skips lines that failed to parse
    // (each item is a `Result`).
    dotenvy::from_read_iter(content.as_bytes())
        .flatten()
        .any(|(name, value)| name == KEY_NAME && !value.trim().is_empty())
}

/// Rejects input that can't be a key: nothing, or text with spaces or
/// quotes (usually a paste of more than the key).
fn check_format(key: &str) -> anyhow::Result<()> {
    if key.is_empty() {
        bail!("no key was entered");
    }
    if key
        .chars()
        .any(|c| c.is_whitespace() || c == '\'' || c == '"')
    {
        bail!("that doesn't look like an API key: it contains spaces or quotes");
    }
    Ok(())
}

/// A 4xx status (except 429 Too Many Requests) means Garmin looked at the
/// key and refused it. Anything else (5xx, 429) says nothing about the key.
fn is_bad_key(status: StatusCode) -> bool {
    status.is_client_error() && status != StatusCode::TOO_MANY_REQUESTS
}

/// Returns the new file text: `existing` without its old key lines, plus
/// the new key. A new file gets a short header comment.
fn with_key(existing: &str, key: &str) -> String {
    // `Vec<&str>`: the lines borrow from `existing`, nothing is copied.
    let mut lines: Vec<&str> = existing.lines().filter(|line| !defines_key(line)).collect();
    if lines.iter().all(|line| line.trim().is_empty()) {
        lines = vec![
            "# garmin-mcp settings, written by `garmin-mcp setup`.",
            "# Keep this file private: it holds your Garmin API key.",
        ];
    }
    let key_line = key_line(key);
    // `&key_line` borrows the `String` as a `&str` to match the other lines.
    lines.push(&key_line);
    lines.join("\n") + "\n"
}

/// Whether one line of a `.env` file sets the key, as `GARMIN_API_KEY=...`
/// or `export GARMIN_API_KEY=...` (spaces allowed around `=`).
fn defines_key(line: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
    // `strip_prefix` gives the rest of the line if it starts with the name.
    // `is_some_and` checks that rest; it excludes e.g. `GARMIN_API_KEY_OLD=`.
    line.strip_prefix(KEY_NAME)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The `GARMIN_API_KEY=...` line. A key with characters that `.env` parsers
/// treat specially (`#`, `$`, `\`) is wrapped in single quotes, which keep
/// the text exactly as is. `check_format` already ruled out quotes inside.
fn key_line(key: &str) -> String {
    let plain = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.~+/=:".contains(c));
    if plain {
        format!("{KEY_NAME}={key}")
    } else {
        format!("{KEY_NAME}='{key}'")
    }
}

/// Writes `content` to `path` so that only the user can read it.
///
/// It goes to a temporary file first, created with the right permissions,
/// which then replaces the real one. So the key is never readable by others,
/// not even for a moment, and a failed write leaves the old file intact.
fn save(dir: &Path, path: &Path, content: &str) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let temp = dir.join(".env.tmp");
    // A leftover from an interrupted run may exist; `let _ =` ignores the
    // result, since "not found" is the normal case.
    let _ = fs::remove_file(&temp);

    let mut options = fs::OpenOptions::new();
    // `create_new` fails if the file appeared in the meantime, instead of
    // reusing a file someone else may have created.
    options.write(true).create_new(true);
    // `#[cfg(unix)]` compiles this block only on macOS and Linux. Mode 0o600
    // (an octal number) means read and write for the owner, nothing for
    // anyone else. On Windows no permissions are set: the file gets those of
    // its folder, `%APPDATA%`, which is private to the user by default.
    #[cfg(unix)]
    {
        // Importing a trait inside a block makes its `.mode()` method
        // available only here.
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("could not create {}", temp.display()))?;

    // From here on the temp file is ours and may hold the key, so it must
    // not be left behind if a step fails (e.g. on Windows the rename fails
    // while an editor or antivirus holds `.env` open). The steps run in a
    // *closure* (`|| { ... }`, an inline function) so that `?` inside it
    // only leaves the closure, and its result can be checked below.
    let result = (|| {
        file.write_all(content.as_bytes())
            .with_context(|| format!("could not write {}", temp.display()))?;
        // `drop` closes the file now, before it is renamed.
        drop(file);
        fs::rename(&temp, path).with_context(|| format!("could not replace {}", path.display()))
    })();
    if result.is_err() {
        // Best effort: if even this fails, the error below is still shown
        // and the next `setup` run removes the leftover first.
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Warns when a key from somewhere with higher priority (an environment
/// variable, or in development builds a `.env` in the current directory)
/// would be used instead of the one just saved.
fn warn_if_overridden(saved: &Path) {
    match config::api_key_source() {
        Some(Source::File(path)) if path == saved => {}
        Some(other) => eprintln!(
            "\nNote: the key from {other} takes priority over the saved one. \
             Remove it there to use the saved key."
        ),
        None => eprintln!(
            "\nWarning: the key could not be read back from {}.",
            saved.display()
        ),
    }
}

/// Prints how to add the server to Claude Code and Claude Desktop, with the
/// full path of this binary. Claude Desktop is a GUI app and doesn't use the
/// shell's `PATH`, so a bare `garmin-mcp` wouldn't be found there.
fn print_next_steps() {
    // `current_exe()` is the running program's path, e.g.
    // `/Users/you/.local/bin/garmin-mcp`.
    let exe = env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "garmin-mcp".to_string());

    eprintln!("\nNext, add the server to your AI app (restart the app if it was already added).");
    eprintln!("\nClaude Code:");
    eprintln!("  claude mcp add -s user garmin -- {}", shell_quote(&exe));
    eprintln!(
        "\nClaude Desktop: Settings > Developer > Edit Config, then add under \"mcpServers\":"
    );
    // `{{` and `}}` print literal braces in a format string.
    eprintln!("  \"garmin\": {{ \"command\": {} }}", json_string(&exe));
    eprintln!(
        "\nA key passed in the app's config (`-e GARMIN_API_KEY=...` or an \"env\" block) \
         takes priority over the saved one."
    );
    // Say up front that the server contacts GitHub, so it isn't a surprise.
    eprintln!(
        "\nThe server checks GitHub for a newer version once a day (this sends your IP address \
         and the version number, no Garmin data). To turn it off, add \
         GARMIN_MCP_NO_UPDATE_CHECK=1 to the saved .env file."
    );
}

/// Quotes a path for pasting into a shell, if it needs it. Also used by
/// `update.rs` for the update command. `pub(crate)` makes it visible to the
/// rest of this program, but not outside it.
pub(crate) fn shell_quote(path: &str) -> String {
    let plain = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-~:\\".contains(c));
    if plain {
        path.to_string()
    } else if cfg!(windows) {
        format!("\"{path}\"")
    } else {
        // In single quotes nothing is special except `'` itself, which is
        // written as `'\''` (close quote, escaped quote, open quote).
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// A path as a JSON string, with quotes and escapes (Windows backslashes
/// become `\\`).
fn json_string(path: &str) -> String {
    serde_json::Value::from(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_gets_header_and_key() {
        assert_eq!(
            with_key("", "abc123"),
            "# garmin-mcp settings, written by `garmin-mcp setup`.\n\
             # Keep this file private: it holds your Garmin API key.\n\
             GARMIN_API_KEY=abc123\n"
        );
    }

    #[test]
    fn old_key_lines_are_replaced_and_others_kept() {
        let existing = "# mine\nGARMIN_BASE_URL=http://x\nGARMIN_API_KEY=old\n\
                        export GARMIN_API_KEY = older\nGARMIN_API_KEY_OLD=keep\n";
        assert_eq!(
            with_key(existing, "new"),
            "# mine\nGARMIN_BASE_URL=http://x\nGARMIN_API_KEY_OLD=keep\nGARMIN_API_KEY=new\n"
        );
    }

    #[test]
    fn has_key_ignores_empty_values() {
        assert!(has_key("GARMIN_API_KEY=abc\n"));
        assert!(!has_key("GARMIN_API_KEY=\nGARMIN_BASE_URL=http://x\n"));
        assert!(!has_key(""));
    }

    #[test]
    fn written_keys_read_back_unchanged() {
        // Every key must come back exactly as entered when the file is read.
        for key in [
            "abc123",
            "a/b+c=",
            "with#hash",
            "dollar$sign",
            "back\\slash",
        ] {
            let text = with_key("", key);
            let parsed: Vec<(String, String)> = dotenvy::from_read_iter(text.as_bytes())
                .map(Result::unwrap)
                .collect();
            assert_eq!(parsed, [(KEY_NAME.to_string(), key.to_string())], "{text}");
        }
    }

    #[test]
    fn format_check_rejects_pastes() {
        assert!(check_format("abc").is_ok());
        assert!(check_format("").is_err());
        assert!(check_format("two words").is_err());
        assert!(check_format("\"quoted\"").is_err());
    }

    #[test]
    fn only_key_errors_count_as_rejected() {
        assert!(is_bad_key(StatusCode::UNAUTHORIZED));
        assert!(is_bad_key(StatusCode::BAD_REQUEST));
        assert!(!is_bad_key(StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_bad_key(StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn save_writes_a_private_file_and_replaces_it() {
        let dir = env::temp_dir().join(format!("garmin-mcp-setup-test-{}", std::process::id()));
        let path = dir.join(".env");
        save(&dir, &path, "GARMIN_API_KEY=one\n").unwrap();
        save(&dir, &path, "GARMIN_API_KEY=two\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "GARMIN_API_KEY=two\n");
        assert!(!dir.join(".env.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_save_leaves_no_temp_file() {
        let dir = env::temp_dir().join(format!("garmin-mcp-setup-fail-{}", std::process::id()));
        // A non-empty folder named `.env` can't be replaced by a file, so
        // the rename fails on every system.
        let path = dir.join(".env");
        fs::create_dir_all(path.join("blocker")).unwrap();
        assert!(save(&dir, &path, "GARMIN_API_KEY=secret\n").is_err());
        // The key must not stay behind in the temp file.
        assert!(!dir.join(".env.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_are_quoted_for_shell_and_json() {
        assert_eq!(
            shell_quote("/usr/local/bin/garmin-mcp"),
            "/usr/local/bin/garmin-mcp"
        );
        if !cfg!(windows) {
            assert_eq!(shell_quote("/My Apps/garmin-mcp"), "'/My Apps/garmin-mcp'");
        }
        assert_eq!(
            json_string(r"C:\bin\garmin-mcp.exe"),
            r#""C:\\bin\\garmin-mcp.exe""#
        );
    }
}
