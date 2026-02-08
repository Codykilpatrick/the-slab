use thiserror::Error;

#[derive(Error, Debug)]
pub enum SlabError {
    #[error("Ollama is not running at {0}. Start it with: ollama serve")]
    OllamaNotRunning(String),

    #[error("Failed to connect to Ollama: {0}")]
    ConnectionError(#[from] reqwest::Error),

    #[error("Model '{0}' not found. Run 'slab models' to see available models.")]
    ModelNotFound(String),

    #[error("No models available. Pull a model with: ollama pull <model>")]
    NoModelsAvailable,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Failed to read config file: {0}")]
    ConfigReadError(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    ConfigParseError(#[from] toml::de::Error),

    #[error("Streaming error: {0}")]
    StreamError(String),

    #[error("File operation error: {0}")]
    FileOperation(String),

    #[error("Template error: {0}")]
    TemplateError(String),

    #[allow(dead_code)]
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SlabError>;
