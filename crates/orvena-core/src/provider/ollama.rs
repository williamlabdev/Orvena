//! Local Ollama provider. Selected **explicitly** (never a silent default);
//! once chosen, the conventional localhost endpoint is used and is overridable.

use super::{ChatRequest, ChatResponse, Provider, ProviderProvenance};
use crate::config::agent::{ProviderSelection, Sampling};
use crate::error::{Error, Result};
use async_trait::async_trait;

pub struct Ollama {
    client: reqwest::Client,
    model: String,
    base_url: String,
    /// `None` = inherited from the model's Modelfile; nothing is sent. See
    /// [`crate::config::agent::Sampling`] for why that is a state worth naming
    /// rather than a set of default numbers.
    sampling: Option<Sampling>,
}

impl Ollama {
    pub fn new(sel: &ProviderSelection) -> Self {
        Self {
            client: reqwest::Client::new(),
            model: sel.model.clone(),
            base_url: sel.base_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string()),
            sampling: sel.sampling,
        }
    }

    /// GET a JSON endpoint, or `None`. Provenance is bookkeeping: an
    /// unreachable server must leave the field unrecorded, never fail a run.
    async fn get_json(&self, path: &str) -> Option<serde_json::Value> {
        let resp = self.client.get(format!("{}{path}", self.base_url)).send().await.ok()?;
        resp.error_for_status().ok()?.json().await.ok()
    }

    async fn post_json(&self, path: &str, body: serde_json::Value) -> Option<serde_json::Value> {
        let resp =
            self.client.post(format!("{}{path}", self.base_url)).json(&body).send().await.ok()?;
        resp.error_for_status().ok()?.json().await.ok()
    }

    /// The tag as ollama spells it. `qwen3:14b` and `qwen3:14b:latest` name the
    /// same model, so entries are matched on either spelling.
    fn tag_matches(&self, name: &str) -> bool {
        name == self.model
            || name.strip_suffix(":latest").is_some_and(|stem| stem == self.model)
            || self.model.strip_suffix(":latest").is_some_and(|stem| stem == name)
    }
}

#[async_trait]
impl Provider for Ollama {
    fn id(&self) -> &str {
        "ollama"
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });
        // Sent only when the repo has taken control. Omitting the key entirely
        // is not the same as sending the model's own values: it leaves the
        // Modelfile in charge, which is the state provenance reports as
        // `inherited`.
        if let Some(s) = self.sampling {
            let mut options = serde_json::json!({
                "temperature": s.temperature,
                "top_p": s.top_p,
                "top_k": s.top_k,
            });
            if let Some(seed) = s.seed {
                options["seed"] = serde_json::json!(seed);
            }
            body["options"] = options;
        }

        let resp =
            self.client.post(format!("{}/api/chat", self.base_url)).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("ollama returned {status}: {text}")));
        }

        let v: serde_json::Value = resp.json().await?;
        let content = v["message"]["content"].as_str().unwrap_or_default().to_string();
        // Ollama reports counts as prompt_eval_count / eval_count.
        let input_tokens = v["prompt_eval_count"].as_u64().unwrap_or(0) as u32;
        let output_tokens = v["eval_count"].as_u64().unwrap_or(0) as u32;

        Ok(ChatResponse { content, input_tokens, output_tokens })
    }

    async fn provenance(&self) -> Option<ProviderProvenance> {
        let mut p = ProviderProvenance::default();

        if let Some(v) = self.get_json("/api/version").await {
            p.server_version = v["version"].as_str().map(|ver| format!("ollama {ver}"));
        }

        // Digest and quantization come from the local library listing: the tag
        // is what we asked for, the digest is what is actually on disk.
        if let Some(v) = self.get_json("/api/tags").await {
            if let Some(m) = v["models"].as_array().and_then(|ms| {
                ms.iter().find(|m| m["name"].as_str().is_some_and(|n| self.tag_matches(n)))
            }) {
                p.model_digest = m["digest"].as_str().map(str::to_string);
                p.quantization = m["details"]["quantization_level"].as_str().map(str::to_string);
            }
        }

        // The model's declared ceiling. The key is family-scoped
        // (`qwen3.context_length`), so it is found by suffix rather than by a
        // family table that would silently miss every future architecture.
        if let Some(v) =
            self.post_json("/api/show", serde_json::json!({ "model": self.model })).await
        {
            if let Some(info) = v["model_info"].as_object() {
                p.context_length_declared = info
                    .iter()
                    .find(|(k, _)| k.ends_with(".context_length"))
                    .and_then(|(_, v)| v.as_u64())
                    .map(|n| n as u32);
            }
            if p.quantization.is_none() {
                p.quantization = v["details"]["quantization_level"].as_str().map(str::to_string);
            }
        }

        // What the runtime actually granted — only knowable while the model is
        // resident, which is why this is read after the runs, not before.
        if let Some(v) = self.get_json("/api/ps").await {
            if let Some(m) = v["models"].as_array().and_then(|ms| {
                ms.iter().find(|m| m["name"].as_str().is_some_and(|n| self.tag_matches(n)))
            }) {
                p.context_length_effective = m["context_length"].as_u64().map(|n| n as u32);
            }
        }

        // All-empty means nothing was reachable. Reporting an empty block would
        // read as "checked, found nothing"; `None` says "not recorded".
        (p != ProviderProvenance::default()).then_some(p)
    }
}
