//! Bearer-token handling for the Garmin personal API.
//!
//! The flow is:
//! 1. `POST /personalapitoken-service/personal-api/personal-api-key/token`
//!    with the header `Authorization: ApiKey <key>`.
//! 2. Garmin answers with the bearer token as plain text (a JWT, not JSON).
//! 3. Every other API call sends `Authorization: Bearer <token>`.
//!
//! A token is valid for 24 hours, so we cache it and fetch a new one only
//! when it is about to expire.

// `Duration` is a span of time; `SystemTime` is a point on the wall clock.
use std::time::{Duration, SystemTime};

use anyhow::Context;
// Tokio's `Mutex` is an async-aware lock: waiting for it uses `.await`
// instead of blocking the whole thread.
use tokio::sync::Mutex;

/// Path of the endpoint that exchanges the API key for a bearer token.
const TOKEN_PATH: &str = "/personalapitoken-service/personal-api/personal-api-key/token";

/// How long Garmin says a token is valid.
// `Duration::from_secs` is a `const fn`, so it can be used in a constant.
const TOKEN_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// Fetch a new token this long before the old one expires, so a request that
/// starts just before expiry does not reach Garmin with a dead token.
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);

/// A bearer token together with the moment it stops being valid.
#[derive(Clone)]
struct CachedToken {
    /// The token itself (sent as `Authorization: Bearer <bearer>`).
    bearer: String,
    /// When the token expires.
    // `SystemTime` (wall clock) rather than `Instant` on purpose: on macOS,
    // `Instant` does not advance while the computer sleeps, so after an
    // overnight sleep an expired token would still look fresh.
    expires_at: SystemTime,
}

impl CachedToken {
    /// Returns `true` if the token is still safe to use at time `now`.
    ///
    /// `now` is a parameter (instead of reading the clock inside) so tests
    /// can check the logic with any time they like.
    fn is_fresh(&self, now: SystemTime) -> bool {
        // Fresh means: even after adding the safety margin, we are still
        // before the expiry time.
        now + REFRESH_MARGIN < self.expires_at
    }
}

/// What Garmin said about an API key (see [`TokenManager::check_key`]).
pub enum KeyCheck {
    /// Garmin issued a token: the key works.
    Valid,
    /// Garmin answered with this error status instead.
    Rejected(reqwest::StatusCode),
}

/// Gets bearer tokens from Garmin and keeps the current one in memory.
///
/// Safe to share between concurrent tool calls: the cache sits behind a
/// `Mutex`, so only one task can read or replace it at a time.
pub struct TokenManager {
    /// HTTP client, shared with `GarminClient`. `reqwest::Client` is cheap to
    /// clone because it is internally reference-counted.
    http: reqwest::Client,
    /// Full URL of the token endpoint.
    token_url: String,
    /// The personal API key. Secret: never log it.
    api_key: String,
    /// The current token, or `None` before the first fetch.
    // `Mutex<Option<...>>`: the lock protects a value that may be absent.
    cached: Mutex<Option<CachedToken>>,
}

impl TokenManager {
    /// Creates a manager. No network call happens until a token is needed.
    pub fn new(http: reqwest::Client, base_url: &str, api_key: String) -> Self {
        Self {
            http,
            // `format!` builds a `String`, like `println!` but returning the
            // text instead of printing it.
            token_url: format!("{base_url}{TOKEN_PATH}"),
            api_key,
            cached: Mutex::new(None),
        }
    }

    /// Returns a valid bearer token, fetching a new one if needed.
    pub async fn get_token(&self) -> anyhow::Result<String> {
        // Take the lock. We keep holding it during the fetch below, so if
        // several tool calls arrive at once, only the first fetches a token
        // and the others wait and then reuse it.
        // `mut` because we may replace the value inside.
        let mut cached = self.cached.lock().await;

        // `cached.as_ref()` turns `&Option<CachedToken>` into
        // `Option<&CachedToken>` (a borrow, no copy). `.filter(...)` keeps it
        // only if it is still fresh. `if let Some(token) = ...` runs the
        // block only when there is a value, binding it to `token`.
        if let Some(token) = cached.as_ref().filter(|t| t.is_fresh(SystemTime::now())) {
            // `.clone()` makes our own copy of the string to return, because
            // the original stays in the cache.
            return Ok(token.bearer.clone());
        }

        // No token yet, or it is about to expire: get a new one.
        let bearer = self.fetch_token().await?;
        // `*cached = ...` writes through the lock guard into the `Option`.
        *cached = Some(CachedToken {
            bearer: bearer.clone(),
            expires_at: SystemTime::now() + TOKEN_LIFETIME,
        });
        eprintln!("garmin: fetched new bearer token");
        Ok(bearer)
        // The lock guard `cached` goes out of scope here, which releases the
        // lock automatically. Rust calls this "drop".
    }

    /// Forgets the cached token if it is still `rejected`.
    ///
    /// Called when Garmin answers 401 Unauthorized, meaning the token stopped
    /// working early (e.g. it was revoked). The next `get_token` then fetches
    /// a fresh one. Comparing with `rejected` avoids throwing away a newer
    /// token that another task fetched in the meantime.
    pub async fn invalidate(&self, rejected: &str) {
        let mut cached = self.cached.lock().await;
        // `matches!` checks a value against a pattern and returns a `bool`.
        // The `if` after the pattern is an extra condition (a "guard").
        if matches!(cached.as_ref(), Some(t) if t.bearer == rejected) {
            *cached = None;
        }
    }

    /// Asks Garmin whether the API key works, without caching anything.
    /// Used by `connect-health-mcp setup` before it saves a key.
    ///
    /// Returns `Ok(Valid)` or `Ok(Rejected(status))` when Garmin answered,
    /// and an error when it couldn't be reached. Found by probing: an unknown
    /// key gets 401, an empty one 400.
    pub async fn check_key(&self) -> anyhow::Result<KeyCheck> {
        let status = self.send_token_request().await?.status();
        // `if` is an expression in Rust: it produces the value of the branch
        // that runs, which becomes the function's return value.
        Ok(if status.is_success() {
            KeyCheck::Valid
        } else {
            KeyCheck::Rejected(status)
        })
    }

    /// Sends the token request and returns Garmin's response, whatever its
    /// status. Errors only on network problems.
    async fn send_token_request(&self) -> anyhow::Result<reqwest::Response> {
        // Build and send the request. Each method returns a modified request
        // builder; `.send()` starts it and `.await` waits for the response.
        self.http
            .post(&self.token_url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("ApiKey {}", self.api_key),
            )
            .send()
            .await
            .context("token request failed (network error)")
    }

    /// Calls the token endpoint and returns the token text.
    async fn fetch_token(&self) -> anyhow::Result<String> {
        let response = self.send_token_request().await?;

        let status = response.status();
        if !status.is_success() {
            // `anyhow::bail!` returns an error from the function right away.
            // We include the status but not the API key.
            anyhow::bail!("token request rejected by Garmin: HTTP {status}");
        }

        // The body is the token as plain text. `.trim()` removes a possible
        // trailing newline; `.to_string()` turns the borrowed `&str` into an
        // owned `String` we can return.
        let body = response
            .text()
            .await
            .context("could not read token response")?;
        let token = body.trim().to_string();
        if token.is_empty() {
            anyhow::bail!("Garmin returned an empty token");
        }
        Ok(token)
    }
}

// `#[cfg(test)]` means this module is compiled only for `cargo test`, not in
// the real program.
#[cfg(test)]
mod tests {
    // `super::*` imports everything from the parent module (this file).
    use super::*;

    /// Builds a token that expires `secs_left` seconds after `now`.
    fn token_expiring_in(now: SystemTime, secs_left: u64) -> CachedToken {
        CachedToken {
            bearer: "test".to_string(),
            expires_at: now + Duration::from_secs(secs_left),
        }
    }

    // `#[test]` marks a function as a test; `cargo test` runs it.
    #[test]
    fn new_token_is_fresh() {
        let now = SystemTime::now();
        assert!(token_expiring_in(now, TOKEN_LIFETIME.as_secs()).is_fresh(now));
    }

    #[test]
    fn token_inside_refresh_margin_is_stale() {
        let now = SystemTime::now();
        // Expires in 1 minute, which is less than the 5-minute margin.
        assert!(!token_expiring_in(now, 60).is_fresh(now));
    }

    #[test]
    fn expired_token_is_stale() {
        let now = SystemTime::now();
        let token = token_expiring_in(now, 60);
        // Look at it one hour later.
        assert!(!token.is_fresh(now + Duration::from_secs(3600)));
    }
}
