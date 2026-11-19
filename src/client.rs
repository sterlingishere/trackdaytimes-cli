//! The HTTP side: one agent, the API key, friendly errors, and polite retries when the API says "slow down".

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::config::Config;

/// The site the CLI reads by default.
pub const DEFAULT_API: &str = "https://trackdaytimes.com";
/// The biggest answer we accept (a whole 24-hour race's laps is a few MB).
const MAX_BODY: u64 = 256 * 1024 * 1024;
/// How often a 429 or a 5xx is tried again before giving up.
const RETRIES: u32 = 4;

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    base: String,
    key: Option<String>,
}

impl Client {
    pub fn new(base: &str, key: Option<String>) -> Client {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(120)))
            .user_agent(concat!("trackdaytimes-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Client {
            agent,
            base: base.trim_end_matches('/').to_string(),
            key,
        }
    }

    /// The client for commands that need a key: the one given, or the saved one.
    pub fn signed_in(base: &str, flag_key: Option<String>) -> Result<Client> {
        let key = match flag_key.filter(|k| !k.trim().is_empty()) {
            Some(key) => key,
            None => Config::load()?.api_key.ok_or_else(|| {
                anyhow!(
                    "You're not signed in. Make an API key at {}/settings#api-keys, then run `tdt login`.",
                    DEFAULT_API
                )
            })?,
        };
        Ok(Client::new(base, Some(key.trim().to_string())))
    }

    /// The site's address, for links.
    pub fn site(&self) -> &str {
        &self.base
    }

    /// `GET /api/v1/<path>` as JSON. `query` pairs with empty values are left out.
    pub fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let path = path.trim_start_matches('/');
        let url = if path.starts_with("api/") {
            format!("{}/{}", self.base, path)
        } else {
            format!("{}/api/v1/{}", self.base, path)
        };
        let mut attempt = 0;
        loop {
            let mut request = self.agent.get(&url).header("Accept", "application/json");
            for (k, v) in query {
                if !v.is_empty() {
                    request = request.query(*k, v);
                }
            }
            if let Some(key) = &self.key {
                request = request.header("Authorization", format!("Bearer {key}"));
            }
            let mut response = request
                .call()
                .with_context(|| format!("Couldn't reach {}. Check your connection.", self.base))?;
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let body = response
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_string()
                .context("The answer was cut off. Try again.")?;
            if (status == 429 || status >= 500) && attempt < RETRIES {
                attempt += 1;
                let wait = retry_after.unwrap_or(2u64.pow(attempt)).clamp(1, 60);
                eprintln!("The API asked us to slow down; trying again in {wait}s…");
                std::thread::sleep(Duration::from_secs(wait));
                continue;
            }
            if !(200..300).contains(&status) {
                bail!("{}", explain(status, &body, &url));
            }
            return serde_json::from_str(&body)
                .with_context(|| format!("{url} didn't answer with JSON."));
        }
    }
}

/// What a failed request means, in words.
fn explain(status: u16, body: &str, url: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    // The API's messages are sentences; some leave off the full stop.
    let message = match message.trim() {
        "" => String::new(),
        m if m.ends_with(['.', '!', '?']) => m.to_string(),
        m => format!("{m}."),
    };
    match status {
        401 => format!(
            "{} Run `tdt login` with a key from {DEFAULT_API}/settings#api-keys.",
            if message.is_empty() {
                "Your API key wasn't accepted."
            } else {
                &message
            }
        ),
        404 => {
            if message.is_empty() {
                "Not found. Check the name or slug (try `tdt search`).".to_string()
            } else {
                format!("{message} Check the name or slug (try `tdt search`).")
            }
        }
        429 => "Too many requests. Wait a minute and try again.".to_string(),
        _ if !message.is_empty() => format!("{message} ({status})"),
        _ => format!("{url} answered {status}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_the_api_message() {
        let body = r#"{"error":{"code":"not_found","message":"Event not found."}}"#;
        assert!(explain(404, body, "u").starts_with("Event not found."));
        assert!(explain(401, "", "u").contains("tdt login"));
        let bare = r#"{"error":{"message":"Layout not found"}}"#;
        assert!(explain(404, bare, "u").starts_with("Layout not found. Check"));
        assert_eq!(explain(500, "oops", "https://x"), "https://x answered 500.");
    }
}
