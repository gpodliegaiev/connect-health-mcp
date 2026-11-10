//! Entry point of the `connect-health-mcp` binary.
//!
//! This program is an MCP (Model Context Protocol) server. An MCP client
//! (for example Claude Code or Claude Desktop) launches it as a child process
//! and talks to it by writing JSON messages to its stdin and reading replies
//! from its stdout. This file only wires things up; the actual tools live in
//! `server.rs`.

// `#![...]` (with `!`) is an attribute for the whole crate. Tests build
// large expected JSON values with `serde_json::json!`, a macro that calls
// itself once per value and needs a higher limit than the default 128.
// `cfg_attr(test, ...)` applies it only to test builds.
#![cfg_attr(test, recursion_limit = "256")]

// `mod server;` tells the compiler that a module named `server` exists and
// that its code is in `src/server.rs`. Without this line that file would be
// ignored entirely.
mod config;
mod garmin;
mod server;
mod setup;
mod update;

// `IsTerminal` adds `.is_terminal()` to stdin/stdout/stderr.
use std::{io::IsTerminal, sync::Arc};

// `use` brings names into scope so we can write `stdio()` instead of
// `rmcp::transport::stdio()`.
// - `ServiceExt` is a *trait*: a set of methods that can be added to other
//   types. Importing it is what makes the `.serve(...)` method available on
//   our `GarminServer` below. In Rust, trait methods can only be called when
//   the trait is in scope.
// - `stdio` builds a "transport": the channel the MCP messages travel over
//   (here, the process's standard input and output).
use rmcp::{ServiceExt, transport::stdio};

// `crate::` means "starting from the root of this project". This imports the
// `GarminServer` struct defined in `src/server.rs`.
use crate::{config::Config, garmin::GarminClient, server::GarminServer, update::UpdateNotice};

/// Help text for `connect-health-mcp --help`.
const USAGE: &str = "\
connect-health-mcp: an MCP server that gives AI apps access to your Garmin Connect data.

Usage:
  connect-health-mcp            run the MCP server on stdin/stdout (your AI app starts it)
  connect-health-mcp setup      save your Garmin API key, then show how to add the server
  connect-health-mcp --version  print the version
  connect-health-mcp --help     print this help";

/// Picks what to do from the command-line arguments: run the MCP server
/// (no arguments, which is how MCP clients start it) or one of the commands
/// a person runs in a terminal.
///
/// `#[tokio::main]` is an *attribute macro*. Rust's `main` function cannot be
/// `async` on its own, because something has to drive async code. The macro
/// rewrites this function into a normal `fn main()` that creates a Tokio
/// runtime (a scheduler for async tasks) and runs the async body on it.
///
/// The return type `anyhow::Result<()>` means: "either success with no value
/// (`()`, the empty type) or an error". `anyhow::Result` can hold any kind of
/// error, which is convenient for application code. If `main` returns an
/// error, Rust prints it and exits with a non-zero status code.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `args()` yields the program name first, then each argument;
    // `.nth(1)` is the first argument, if any. `.as_deref()` turns
    // `Option<String>` into `Option<&str>` so it can be matched against
    // string literals. `"a" | "b"` matches either one.
    // Output goes to stderr even here, so stdout is only ever used by MCP.
    match std::env::args().nth(1).as_deref() {
        None => serve().await,
        Some("setup") => setup::run().await,
        Some("--version" | "-V") => {
            // `env!` reads a value at compile time; Cargo sets
            // `CARGO_PKG_VERSION` from `version` in `Cargo.toml`.
            eprintln!("connect-health-mcp {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h" | "help") => {
            eprintln!("{USAGE}");
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown command `{other}`\n\n{USAGE}"),
    }
}

/// Starts the MCP server and keeps it running until the client disconnects.
async fn serve() -> anyhow::Result<()> {
    // stdout carries the MCP protocol; log to stderr only.
    // `eprintln!` (note the `e`) prints to stderr. Using `println!` here would
    // write garbage into the protocol stream and break the client.
    // The `!` means `eprintln!` is a macro, not a regular function.
    eprintln!("connect-health-mcp starting on stdio");

    // Someone ran `connect-health-mcp` by hand in a terminal (an MCP client would
    // connect stdin to a pipe instead). The server would just sit there
    // silently, so say what it is. `is_terminal` comes from the
    // `IsTerminal` trait.
    if std::io::stdin().is_terminal() {
        eprintln!(
            "This is an MCP server: your AI app starts it. To save your Garmin API key, \
             run `connect-health-mcp setup`. Press Ctrl+C to quit."
        );
    }

    // Read settings (API key, base URL) before serving anything. If the key
    // is missing, `?` stops the program here with a clear error message
    // instead of failing later on the first tool call.
    let config = Config::from_env()?;
    // `{:?}` uses our custom `Debug` impl, which hides the API key.
    eprintln!("config loaded: {config:?}");

    // Build the Garmin API client. No network call happens yet; the first
    // bearer token is fetched on the first tool call.
    // `&config` lends the config to `new` without giving it away.
    let garmin = GarminClient::new(&config)?;

    // Look for a newer release in the background (see `update.rs`). The
    // server and the check share the same `UpdateNotice` through an `Arc`
    // (a shared pointer); `Arc::clone` makes a second handle to it, not a
    // copy of the data.
    let update = Arc::new(UpdateNotice::default());
    if config.update_check {
        update::spawn_check(Arc::clone(&update));
    }

    // Create our server and start serving it over stdin/stdout.
    // - `GarminServer::new(garmin, update)` builds the server value (see
    //   `server.rs`), moving the Garmin client and the notice into it.
    // - `.serve(stdio())` performs the MCP handshake with the client and
    //   returns a handle to the running service.
    // - `.await` pauses here until that async work finishes, without blocking
    //   the thread.
    // - `?` is error propagation: if the result is an error, return it from
    //   `main` immediately; if it is `Ok(value)`, unwrap it to `value`.
    let service = GarminServer::new(garmin, update).serve(stdio()).await?;

    // Wait until the client closes the connection (or an error occurs).
    // While we wait, the service handles incoming requests in the background.
    service.waiting().await?;

    // Everything went fine. `Ok(())` is the success value with no data.
    // The last expression in a function (without a trailing `;`) is its
    // return value, so no `return` keyword is needed.
    Ok(())
}
