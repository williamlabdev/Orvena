//! OpenAI-compatible provider (OpenAI and OpenRouter) over /chat/completions.
//!
//! Rate-limit handling: capped keys (e.g. a Gemini free-tier key reached via the
//! OpenAI-compatible endpoint, limited to a few requests/minute) return HTTP 429.
//! Rather than surfacing that as a fatal provider error mid-run — which turns a
//! benchmark into garbage — this provider retries 429/503 responses, honoring the
//! server-supplied retry delay when present, and can optionally pace requests
//! proactively (`ORVENA_MIN_REQUEST_INTERVAL_MS`) to stay under a known limit.

use super::{ChatRequest, ChatResponse, Provider};
use crate::config::agent::ProviderSelection;
use crate::error::{Error, Result};
use async_trait::async_trait;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How many times to retry a rate-limited/transient response before giving up.
const DEFAULT_MAX_RETRIES: u32 = 6;
/// Upper bound on any single backoff sleep, so a bogus retry hint can't hang a run.
const MAX_BACKOFF: Duration = Duration::from_secs(90);

/// Process-global pacing clock: the reserved send time of the most recent request
/// across *all* provider instances. The benchmark harness rebuilds a provider per
/// task, so per-instance state would reset every run and never pace across runs —
/// exactly when a low-RPM endpoint gets hammered. A shared clock keeps the whole
/// process under `min_interval` regardless of how many providers are constructed.
fn pacing_clock() -> &'static Mutex<Option<Instant>> {
    static CLOCK: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    CLOCK.get_or_init(|| Mutex::new(None))
}

pub struct OpenAiCompat {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    id: &'static str,
    /// Max retries on 429/503/transport error before returning the error.
    max_retries: u32,
    /// Proactive minimum spacing between requests; `ZERO` disables throttling.
    min_interval: Duration,
}

impl OpenAiCompat {
    pub fn from_env(sel: &ProviderSelection) -> Result<Self> {
        let (env_key, default_base, id) = match sel.kind.as_str() {
            "openrouter" => ("OPENROUTER_API_KEY", "https://openrouter.ai/api/v1", "openrouter"),
            _ => ("OPENAI_API_KEY", "https://api.openai.com/v1", "openai"),
        };
        let api_key = std::env::var(env_key)
            .map_err(|_| Error::Provider(format!("{env_key} is not set — put it in .env")))?;
        let max_retries = std::env::var("ORVENA_MAX_RETRIES")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(DEFAULT_MAX_RETRIES);
        let min_interval = std::env::var("ORVENA_MIN_REQUEST_INTERVAL_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::ZERO);
        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: sel.model.clone(),
            base_url: sel.base_url.clone().unwrap_or_else(|| default_base.to_string()),
            id,
            max_retries,
            min_interval,
        })
    }

    /// Proactively pace requests to be at least `min_interval` apart, using the
    /// process-global clock so pacing holds across the per-task providers the
    /// benchmark builds. Reserves the next slot under a short lock (never held
    /// across the await), so callers self-space without hammering a low-RPM endpoint.
    async fn throttle(&self) {
        if self.min_interval.is_zero() {
            return;
        }
        let wait = {
            let mut last = pacing_clock().lock().unwrap();
            let now = Instant::now();
            let wait = match *last {
                Some(prev) if now < prev => prev - now,
                Some(prev) => {
                    let elapsed = now - prev;
                    self.min_interval.saturating_sub(elapsed)
                }
                None => Duration::ZERO,
            };
            // Reserve this request's effective send time so the next caller paces off it.
            *last = Some(now + wait);
            wait
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[async_trait]
impl Provider for OpenAiCompat {
    fn id(&self) -> &str {
        self.id
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
        });

        let mut attempt: u32 = 0;
        loop {
            self.throttle().await;

            // Transport errors (connection reset/timeout under load) are transient;
            // retry them with backoff rather than failing the run outright.
            let resp = match self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(_) if attempt < self.max_retries => {
                    attempt += 1;
                    tokio::time::sleep(
                        Duration::from_secs(2u64.saturating_pow(attempt).min(30)).min(MAX_BACKOFF),
                    )
                    .await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            let status = resp.status();
            if status.is_success() {
                let v: serde_json::Value = resp.json().await?;
                let content = v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let input_tokens = v["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens = v["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;
                return Ok(ChatResponse { content, input_tokens, output_tokens });
            }

            let text = resp.text().await.unwrap_or_default();
            let retryable = matches!(status.as_u16(), 429 | 503);
            if retryable && attempt < self.max_retries {
                attempt += 1;
                // Honor the server's retry hint (+1s margin); else exponential backoff.
                let wait = retry_delay_from_body(&text)
                    .map(|d| d + Duration::from_secs(1))
                    .unwrap_or_else(|| Duration::from_secs(2u64.saturating_pow(attempt).min(60)))
                    .min(MAX_BACKOFF);
                tokio::time::sleep(wait).await;
                continue;
            }

            return Err(Error::Provider(format!("{} returned {status}: {text}", self.id)));
        }
    }
}

/// Extract a retry delay from a rate-limit error body. Handles Google's structured
/// `error.details[].retryDelay: "12s"` and the plain-text "…retry in 11.99s" form.
fn retry_delay_from_body(text: &str) -> Option<Duration> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(details) = v["error"]["details"].as_array() {
            for d in details {
                if let Some(secs) = d["retryDelay"].as_str().and_then(parse_secs_suffix) {
                    return Some(Duration::from_secs_f64(secs));
                }
            }
        }
    }
    if let Some(idx) = text.find("retry in ") {
        let rest = &text[idx + "retry in ".len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(secs) = num.parse::<f64>() {
            if secs.is_finite() && secs >= 0.0 {
                return Some(Duration::from_secs_f64(secs));
            }
        }
    }
    None
}

/// Parse a `"<number>s"` duration like "12s" or "11.99s" into seconds.
fn parse_secs_suffix(s: &str) -> Option<f64> {
    let secs = s.strip_suffix('s')?.trim().parse::<f64>().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_google_structured_retry_delay() {
        let body = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED",
            "details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"12s"}]}}"#;
        assert_eq!(retry_delay_from_body(body), Some(Duration::from_secs(12)));
    }

    #[test]
    fn parses_fractional_and_plaintext_retry_delay() {
        let structured = r#"{"error":{"details":[{"retryDelay":"11.5s"}]}}"#;
        assert_eq!(retry_delay_from_body(structured), Some(Duration::from_secs_f64(11.5)));

        let plain = "You exceeded your quota. Please retry in 11.993681175s. status RESOURCE_EXHAUSTED";
        let got = retry_delay_from_body(plain).unwrap();
        assert!((got.as_secs_f64() - 11.993681175).abs() < 1e-6, "got {got:?}");
    }

    #[test]
    fn no_hint_returns_none() {
        assert_eq!(retry_delay_from_body("plain 500 error, no hint"), None);
        assert_eq!(retry_delay_from_body("{\"error\":{\"code\":500}}"), None);
    }
}
