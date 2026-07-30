//! Anthropic (Claude) provider over the Messages API.

use super::{ChatRequest, ChatResponse, Provider};
use crate::config::agent::ProviderSelection;
use crate::error::{Error, Result};
use async_trait::async_trait;

pub struct Anthropic {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl Anthropic {
    pub fn from_env(sel: &ProviderSelection) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| Error::Provider("ANTHROPIC_API_KEY is not set — put it in .env".into()))?;
        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: sel.model.clone(),
            base_url: sel
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
        })
    }
}

#[async_trait]
impl Provider for Anthropic {
    fn id(&self) -> &str {
        "anthropic"
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        // Anthropic takes `system` separately from the message turns.
        let system: String = req
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "system": system,
            "messages": messages,
        });

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("anthropic returned {status}: {text}")));
        }

        let v: serde_json::Value = resp.json().await?;
        let content = v["content"]
            .as_array()
            .and_then(|blocks| blocks.iter().find_map(|b| b["text"].as_str()))
            .unwrap_or_default()
            .to_string();
        let input_tokens = v["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = v["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

        Ok(ChatResponse { content, input_tokens, output_tokens })
    }
}
