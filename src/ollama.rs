use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{Result, SlabError};

#[derive(Debug, Clone)]
pub struct OllamaClient {
    client: Client,
    base_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ModelOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerateRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ModelOptions>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatResponse {
    pub message: Option<Message>,
    pub done: bool,
    #[serde(default)]
    pub total_duration: Option<u64>,
    #[serde(default)]
    pub eval_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GenerateResponse {
    pub response: String,
    pub done: bool,
    #[serde(default)]
    pub total_duration: Option<u64>,
    #[serde(default)]
    pub eval_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TagsResponse {
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ModelInfo {
    pub name: String,
    pub modified_at: String,
    pub size: u64,
    #[serde(default)]
    pub details: Option<ModelDetails>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ModelDetails {
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Check if Ollama is running
    pub async fn health_check(&self) -> Result<()> {
        let url = format!("{}/api/tags", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(_) => Err(SlabError::OllamaNotRunning(self.base_url.clone())),
            Err(e) if e.is_connect() => Err(SlabError::OllamaNotRunning(self.base_url.clone())),
            Err(e) => Err(SlabError::ConnectionError(e)),
        }
    }

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(SlabError::OllamaNotRunning(self.base_url.clone()));
        }

        let tags: TagsResponse = resp.json().await?;
        Ok(tags.models)
    }

    /// Send a chat request with streaming response
    pub async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<Result<String>>> {
        let url = format!("{}/api/chat", self.base_url);
        let mut req = request;
        req.stream = Some(true);

        let resp = self.client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if body.contains("model") && body.contains("not found") {
                return Err(SlabError::ModelNotFound(req.model));
            }
            return Err(SlabError::StreamError(format!("HTTP {}: {}", status, body)));
        }

        let (tx, rx) = mpsc::channel(100);
        let mut stream = resp.bytes_stream();

        tokio::spawn(async move {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if line.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<ChatResponse>(line) {
                                Ok(resp) => {
                                    if let Some(msg) = resp.message {
                                        if !msg.content.is_empty()
                                            && tx.send(Ok(msg.content)).await.is_err()
                                        {
                                            return;
                                        }
                                    }
                                    if resp.done {
                                        return;
                                    }
                                }
                                Err(e) => {
                                    let _ = tx
                                        .send(Err(SlabError::StreamError(format!(
                                            "Parse error: {}",
                                            e
                                        ))))
                                        .await;
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(SlabError::ConnectionError(e))).await;
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Send a chat request without streaming (returns complete response)
    pub async fn chat(&self, request: ChatRequest) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        let mut req = request;
        req.stream = Some(false);

        let resp = self.client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if body.contains("model") && body.contains("not found") {
                return Err(SlabError::ModelNotFound(req.model));
            }
            return Err(SlabError::StreamError(format!("HTTP {}: {}", status, body)));
        }

        let chat_resp: ChatResponse = resp.json().await?;
        Ok(chat_resp.message.map(|m| m.content).unwrap_or_default())
    }

    /// Send a generate request (single prompt, not chat)
    #[allow(dead_code)]
    pub async fn generate(&self, request: GenerateRequest) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let mut req = request;
        req.stream = Some(false);

        let resp = self.client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if body.contains("model") && body.contains("not found") {
                return Err(SlabError::ModelNotFound(req.model));
            }
            return Err(SlabError::StreamError(format!("HTTP {}: {}", status, body)));
        }

        let gen_resp: GenerateResponse = resp.json().await?;
        Ok(gen_resp.response)
    }
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}
