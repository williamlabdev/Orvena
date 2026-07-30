//! Local Ollama provider. Selected **explicitly** (never a silent default);
//! once chosen, the conventional localhost endpoint is used and is overridable.

use super::{ChatRequest, ChatResponse, Provider};
use crate::config::agent::ProviderSelection;
use crate::error::{Error, Result};
use async_trait::async_trait;

pub struct Ollama {
    client: reqwest::Client,
    model: String,
    base_url: String,
}

impl Ollama {
    pub fn new(sel: &ProviderSelection) -> Self {
        Self {
            client: reqwest::Client::new(),
            model: sel.model.clone(),
            base_url: sel.base_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string()),
        }
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

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

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
}
