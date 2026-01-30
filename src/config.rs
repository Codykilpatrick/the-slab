use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Result, SlabError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_ollama_host")]
    pub ollama_host: String,

    #[serde(default)]
    pub default_model: Option<String>,

    #[serde(default = "default_context_limit")]
    pub context_limit: usize,

    /// Default system prompt used when no model-specific prompt is set
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,

    #[serde(default)]
    pub models: HashMap<String, ModelConfig>,

    #[serde(default)]
    pub paths: PathsConfig,

    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_top_p")]
    pub top_p: f32,

    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathsConfig {
    #[serde(default)]
    pub templates: Option<PathBuf>,

    #[serde(default)]
    pub rules: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub theme: String,

    #[serde(default = "default_streaming")]
    pub streaming: bool,

    /// Auto-apply file operations without prompting
    #[serde(default)]
    pub auto_apply_file_ops: bool,

    /// Show inline completion preview (fish-style ghost text)
    #[serde(default = "default_true")]
    pub inline_completion_preview: bool,

    /// Enable fuzzy matching for completions
    #[serde(default = "default_true")]
    pub fuzzy_completion: bool,

    /// Maximum number of items to show in completion menu
    #[serde(default = "default_max_completions")]
    pub max_completion_items: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            streaming: default_streaming(),
            auto_apply_file_ops: false,
            inline_completion_preview: true,
            fuzzy_completion: true,
            max_completion_items: 10,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_completions() -> usize {
    10
}

fn default_ollama_host() -> String {
    "http://localhost:11434".to_string()
}

fn default_context_limit() -> usize {
    32768
}

fn default_temperature() -> f32 {
    0.7
}

fn default_top_p() -> f32 {
    0.9
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_streaming() -> bool {
    true
}

fn default_system_prompt() -> String {
    r#"You are a helpful coding assistant running in The Slab CLI.

## Creating or Modifying Files

When you need to create or modify files, output them as fenced code blocks with the filename after a colon:

```language:path/to/file.ext
file contents here
```

Examples:
- ```rust:src/main.rs for Rust files
- ```python:script.py for Python files
- ```txt:notes.txt for text files
- ```toml:Cargo.toml for TOML files

## Deleting Files

When you need to delete a file, output a delete marker on its own line:

DELETE:path/to/file.ext

Examples:
- DELETE:src/old_module.rs
- DELETE:temp/cache.json
- DELETE:unused.txt

The user will be prompted to confirm all file operations before they are applied."#.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ollama_host: default_ollama_host(),
            default_model: None,
            context_limit: default_context_limit(),
            system_prompt: default_system_prompt(),
            models: HashMap::new(),
            paths: PathsConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Config {
    /// Load config from file, checking project-local first, then global
    pub fn load(custom_path: Option<&PathBuf>) -> Result<Self> {
        // If custom path specified, use that
        if let Some(path) = custom_path {
            return Self::load_from_path(path);
        }

        // Try project-local config first
        let local_config = PathBuf::from(".slab/config.toml");
        if local_config.exists() {
            return Self::load_from_path(&local_config);
        }

        // Try global config
        if let Some(global_config) = Self::global_config_path() {
            if global_config.exists() {
                return Self::load_from_path(&global_config);
            }
        }

        // Return default config if no config file found
        Ok(Self::default())
    }

    fn load_from_path(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn global_config_path() -> Option<PathBuf> {
        dirs_next::config_dir().map(|p| p.join("slab").join("config.toml"))
    }

    pub fn project_config_path() -> PathBuf {
        PathBuf::from(".slab/config.toml")
    }

    /// Get the model config for a given model name, or create a default one
    pub fn get_model_config(&self, model: &str) -> ModelConfig {
        let mut config = self
            .models
            .get(model)
            .cloned()
            .unwrap_or_else(|| ModelConfig {
                name: model.to_string(),
                temperature: default_temperature(),
                top_p: default_top_p(),
                system_prompt: None,
            });

        // Use the global system prompt if no model-specific one is set
        if config.system_prompt.is_none() {
            config.system_prompt = Some(self.system_prompt.clone());
        }

        config
    }

    /// Save config to the project-local config file
    pub fn save(&self) -> Result<()> {
        let path = Self::project_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| SlabError::ConfigError(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}
