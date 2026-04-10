use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{Result, SlabError};
use crate::ollama::{ChatRequest, Message, ModelInfo};

/// OpenAI-compatible client. Works with vllm, llama.cpp --server, LM Studio,
/// and any other server that implements the `/v1/chat/completions` API.
#[derive(Debug, Clone)]
pub struct OpenAiClient {
    client: Client,
    pub(crate) base_url: String,
    api_key: Option<String>,
}

// ── Request types ─────────────────────────────────────────────────────────────

/// The POST body for /v1/chat/completions.
/// Note: we do NOT map Ollama's `num_ctx` here — that controls the context
/// *window* size, while OpenAI's `max_tokens` controls the *output* limit.
/// They are semantically different; omitting max_tokens lets the server use
/// its own default output limit.
#[derive(Debug, Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    stream: bool,
}

impl<'a> OpenAiChatRequest<'a> {
    fn from_chat_request(req: &'a ChatRequest, stream: bool) -> Self {
        let (temperature, top_p) = req
            .options
            .as_ref()
            .map(|o| (o.temperature, o.top_p))
            .unwrap_or((None, None));
        Self {
            model: &req.model,
            messages: &req.messages,
            temperature,
            top_p,
            stream,
        }
    }
}

// ── Non-streaming response types ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

// ── Streaming response types ──────────────────────────────────────────────────

/// One SSE chunk: `data: <json>\n\n`
#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiDelta,
    finish_reason: Option<String>,
}

/// Content is absent in the first chunk (which only carries `role`) and in
/// the final chunk (which only carries `finish_reason`). We therefore make
/// it `Option<String>`.
#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    content: Option<String>,
}

// ── Model list response ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
}

// ── Client implementation ─────────────────────────────────────────────────────

impl OpenAiClient {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    /// Attach the Authorization header if an API key is configured.
    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => builder.header("Authorization", format!("Bearer {}", key)),
            None => builder,
        }
    }

    /// Check connectivity by listing models. Returns an error if the server is
    /// unreachable or responds with a non-success status.
    pub async fn health_check(&self) -> Result<()> {
        let url = format!("{}/v1/models", self.base_url);
        let req = self.auth(self.client.get(&url));
        match req.send().await {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(_) => Err(SlabError::BackendNotReachable(self.base_url.clone())),
            Err(e) if e.is_connect() => {
                Err(SlabError::BackendNotReachable(self.base_url.clone()))
            }
            Err(e) => Err(SlabError::ConnectionError(e)),
        }
    }

    /// List models available on the server via `GET /v1/models`.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/v1/models", self.base_url);
        let req = self.auth(self.client.get(&url));
        let resp = req.send().await?;

        if !resp.status().is_success() {
            return Err(SlabError::BackendNotReachable(self.base_url.clone()));
        }

        let body: OpenAiModelsResponse = resp.json().await?;
        Ok(body
            .data
            .into_iter()
            .map(|m| ModelInfo {
                name: m.id,
                modified_at: None,
                size: None,
                details: None,
            })
            .collect())
    }

    /// Send a non-streaming chat request to `POST /v1/chat/completions`.
    pub async fn chat(&self, request: ChatRequest) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = OpenAiChatRequest::from_chat_request(&request, false);
        let req = self.auth(self.client.post(&url).json(&body));
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.as_u16() == 404
                || text.to_lowercase().contains("model")
                    && text.to_lowercase().contains("not found")
            {
                return Err(SlabError::ModelNotFound(request.model));
            }
            return Err(SlabError::StreamError(format!("HTTP {}: {}", status, text)));
        }

        let chat_resp: OpenAiChatResponse = resp.json().await?;
        Ok(chat_resp
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default())
    }

    /// Send a streaming chat request. Returns a channel that yields content
    /// tokens as they arrive.
    ///
    /// OpenAI uses Server-Sent Events (SSE):
    ///   - Each event line is prefixed with `data: `
    ///   - The stream terminates with `data: [DONE]`
    ///   - Blank lines separate events and must be ignored
    ///   - The first delta often has `role` but empty `content`
    ///   - The final delta has no `content` key, only `finish_reason`
    pub async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<Result<String>>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = OpenAiChatRequest::from_chat_request(&request, true);
        let req = self.auth(self.client.post(&url).json(&body));
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.as_u16() == 404
                || text.to_lowercase().contains("model")
                    && text.to_lowercase().contains("not found")
            {
                return Err(SlabError::ModelNotFound(request.model));
            }
            return Err(SlabError::StreamError(format!("HTTP {}: {}", status, text)));
        }

        let (tx, rx) = mpsc::channel(100);
        let mut stream = resp.bytes_stream();

        tokio::spawn(async move {
            // Accumulate a partial line buffer to handle chunks that split mid-line.
            let mut buf = String::new();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));

                        // Process every complete line (terminated by \n).
                        // We leave any trailing partial line in `buf`.
                        while let Some(newline_pos) = buf.find('\n') {
                            let line = buf[..newline_pos].trim_end_matches('\r').to_string();
                            buf = buf[newline_pos + 1..].to_string();

                            // Blank lines are SSE event separators — skip them.
                            if line.is_empty() {
                                continue;
                            }

                            // Strip the required `data: ` prefix.
                            let json_str = match line.strip_prefix("data: ") {
                                Some(s) => s,
                                None => {
                                    // Non-data lines (e.g. `event:`, `id:`, comments)
                                    // are valid SSE but we don't use them.
                                    continue;
                                }
                            };

                            // The stream terminator — not JSON, do not parse.
                            if json_str == "[DONE]" {
                                return;
                            }

                            match serde_json::from_str::<OpenAiStreamChunk>(json_str) {
                                Ok(chunk) => {
                                    for choice in chunk.choices {
                                        // `finish_reason` being set means this is
                                        // the last choice; content will be absent.
                                        if choice.finish_reason.is_some() {
                                            continue;
                                        }
                                        if let Some(content) = choice.delta.content {
                                            // First chunk has empty content — skip.
                                            if !content.is_empty()
                                                && tx.send(Ok(content)).await.is_err()
                                            {
                                                return;
                                            }
                                        }
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
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::Message;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_client(base_url: &str) -> OpenAiClient {
        OpenAiClient::new(base_url, None)
    }

    fn make_client_with_key(base_url: &str, key: &str) -> OpenAiClient {
        OpenAiClient::new(base_url, Some(key.to_string()))
    }

    // ── health_check ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_check_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        assert!(client.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn health_check_server_error_returns_err() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        assert!(client.health_check().await.is_err());
    }

    #[tokio::test]
    async fn health_check_unreachable_returns_err() {
        // Port 1 is reserved and should refuse connections immediately.
        let client = make_client("http://127.0.0.1:1");
        assert!(client.health_check().await.is_err());
    }

    // ── list_models ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_models_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"id": "llama3"},
                    {"id": "mistral-7b"}
                ]
            })))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        let models = client.list_models().await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "llama3");
        assert_eq!(models[1].name, "mistral-7b");
    }

    #[tokio::test]
    async fn list_models_server_error_returns_err() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        assert!(client.list_models().await.is_err());
    }

    // ── chat (non-streaming) ──────────────────────────────────────────────────

    #[tokio::test]
    async fn chat_returns_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "Hello!"}}]
            })))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        let request = ChatRequest {
            model: "llama3".to_string(),
            messages: vec![Message::user("hi")],
            stream: None,
            options: None,
        };
        let response = client.chat(request).await.unwrap();
        assert_eq!(response, "Hello!");
    }

    #[tokio::test]
    async fn chat_model_not_found_returns_err() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_string(r#"{"error":"model not found"}"#),
            )
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        let request = ChatRequest {
            model: "ghost-model".to_string(),
            messages: vec![Message::user("hi")],
            stream: None,
            options: None,
        };
        let err = client.chat(request).await.unwrap_err();
        assert!(
            matches!(err, SlabError::ModelNotFound(_)),
            "expected ModelNotFound, got: {err}"
        );
    }

    #[tokio::test]
    async fn chat_propagates_temperature_and_top_p() {
        use crate::ollama::ModelOptions;
        use wiremock::Request;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}]
            })))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        let request = ChatRequest {
            model: "llama3".to_string(),
            messages: vec![Message::user("hi")],
            stream: None,
            options: Some(ModelOptions {
                temperature: Some(0.3),
                top_p: Some(0.8),
                num_ctx: None,
            }),
        };
        client.chat(request).await.unwrap();

        // Inspect the captured request body.
        let received: Request = server.received_requests().await.unwrap().remove(0);
        let body: serde_json::Value = serde_json::from_slice(&received.body).unwrap();
        // serde_json serializes f32 values using their shortest decimal, so
        // 0.3f32 round-trips through JSON as exactly 0.3f64.
        let temp = body["temperature"].as_f64().unwrap();
        let top_p = body["top_p"].as_f64().unwrap();
        assert!((temp - 0.3).abs() < 1e-5, "temperature mismatch: {temp}");
        assert!((top_p - 0.8).abs() < 1e-5, "top_p mismatch: {top_p}");
        // num_ctx must NOT be forwarded to the OpenAI endpoint.
        assert!(body.get("max_tokens").is_none());
    }

    // ── API key ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn api_key_sent_as_bearer_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer secret-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
            .mount(&server)
            .await;

        let client = make_client_with_key(&server.uri(), "secret-key");
        // If the header is missing or wrong wiremock returns 404, so an Ok means
        // the header was present and correct.
        assert!(client.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn no_api_key_omits_authorization_header() {
        let server = MockServer::start().await;
        // Only match requests that do NOT have an Authorization header.
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        assert!(client.health_check().await.is_ok());

        let requests = server.received_requests().await.unwrap();
        let auth_present = requests[0]
            .headers
            .iter()
            .any(|(name, _)| name.as_str() == "authorization");
        assert!(
            !auth_present,
            "Authorization header should not be set when no api_key is configured"
        );
    }

    // ── chat_stream ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn chat_stream_assembles_tokens() {
        let sse_body = [
            r#"data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":" world"},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ]
        .join("\n\n")
            + "\n\n";

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        let request = ChatRequest {
            model: "llama3".to_string(),
            messages: vec![Message::user("hi")],
            stream: None,
            options: None,
        };

        let mut rx = client.chat_stream(request).await.unwrap();
        let mut tokens = Vec::new();
        while let Some(chunk) = rx.recv().await {
            tokens.push(chunk.unwrap());
        }

        assert_eq!(tokens, vec!["Hello", " world"]);
    }

    #[tokio::test]
    async fn chat_stream_skips_empty_content() {
        // First delta has role but no content; second has empty string content.
        let sse_body = [
            r#"data: {"choices":[{"delta":{"role":"assistant","content":""},"finish_reason":null}]}"#,
            r#"data: {"choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#,
            "data: [DONE]",
        ]
        .join("\n\n")
            + "\n\n";

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .mount(&server)
            .await;

        let client = make_client(&server.uri());
        let request = ChatRequest {
            model: "llama3".to_string(),
            messages: vec![Message::user("hi")],
            stream: None,
            options: None,
        };

        let mut rx = client.chat_stream(request).await.unwrap();
        let mut tokens = Vec::new();
        while let Some(chunk) = rx.recv().await {
            tokens.push(chunk.unwrap());
        }

        // Empty-string content must not be forwarded.
        assert_eq!(tokens, vec!["Hi"]);
    }
}
