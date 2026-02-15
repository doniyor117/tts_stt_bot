use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

#[derive(Debug, Clone, Serialize)]
struct GroqMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct GroqResponse {
    choices: Vec<GroqChoice>,
    usage: Option<GroqUsage>,
}

#[derive(Debug, Deserialize)]
struct GroqChoice {
    message: GroqMessageContent,
}

#[derive(Debug, Deserialize)]
struct GroqMessageContent {
    content: String,
}

#[derive(Debug, Deserialize)]
pub struct GroqUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub struct LlmClient {
    client: Client,
    api_key: String,
    model: String,
}

/// A simple (role, content) pair for building the messages array.
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub struct LlmResponse {
    pub text: String,
    pub usage: Option<GroqUsage>,
}

impl LlmClient {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            client: Client::new(),
            api_key: config.groq_api_key.clone(),
            model: config.groq_model.clone(),
        }
    }

    /// Send a conversation to Groq and get the assistant's reply.
    pub async fn chat(&self, messages: &[ChatMessage]) -> anyhow::Result<LlmResponse> {
        let groq_messages: Vec<GroqMessage> = messages
            .iter()
            .map(|m| GroqMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let body = serde_json::json!({
            "model": self.model,
            "messages": groq_messages,
            "temperature": 0.7,
            "max_tokens": 2048,
        });

        let resp = self
            .client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Groq API error ({}): {}", status, err_body);
        }

        let groq_resp: GroqResponse = resp.json().await?;

        let text = groq_resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(LlmResponse {
            text,
            usage: groq_resp.usage,
        })
    }

    /// Estimate token count for a string (rough: ~4 chars per token).
    pub fn estimate_tokens(text: &str) -> i32 {
        (text.len() as f64 / 4.0).ceil() as i32
    }
}
