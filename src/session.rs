use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config::find_project_root;
use crate::ollama::Message;

/// A saved chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session name
    pub name: String,

    /// Model used
    pub model: String,

    /// Conversation messages
    pub messages: Vec<Message>,

    /// Timestamp when session was last updated
    pub updated_at: String,
}

impl Session {
    pub fn new(name: &str, model: &str) -> Self {
        Self {
            name: name.to_string(),
            model: model.to_string(),
            messages: Vec::new(),
            updated_at: chrono::Local::now().to_rfc3339(),
        }
    }

    /// Get the session directory
    fn session_dir() -> Option<PathBuf> {
        // Try project-local first by walking up directory tree
        if let Some(root) = find_project_root() {
            let local_dir = root.join(".slab/sessions");
            if local_dir.exists() || std::fs::create_dir_all(&local_dir).is_ok() {
                return Some(local_dir);
            }
        }

        // Fall back to global
        dirs_next::data_dir().map(|d| d.join("slab/sessions"))
    }

    /// Get the path to a session file
    fn session_path(name: &str) -> Option<PathBuf> {
        Self::session_dir().map(|d| d.join(format!("{}.json", sanitize_filename(name))))
    }

    /// Get the path to the "last" session marker
    fn last_session_path() -> Option<PathBuf> {
        Self::session_dir().map(|d| d.join("_last"))
    }

    /// Save the session to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::session_path(&self.name)
            .ok_or_else(|| "Could not determine session path".to_string())?;

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create session directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize session: {}", e))?;

        fs::write(&path, json).map_err(|e| format!("Failed to write session file: {}", e))?;

        // Update the "last" marker
        if let Some(last_path) = Self::last_session_path() {
            let _ = fs::write(last_path, &self.name);
        }

        Ok(())
    }

    /// Load a session by name
    pub fn load(name: &str) -> Result<Self, String> {
        let path = Self::session_path(name)
            .ok_or_else(|| "Could not determine session path".to_string())?;

        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read session file: {}", e))?;

        serde_json::from_str(&content).map_err(|e| format!("Failed to parse session: {}", e))
    }

    /// Load the last used session
    pub fn load_last() -> Result<Self, String> {
        let last_path = Self::last_session_path()
            .ok_or_else(|| "Could not determine last session path".to_string())?;

        let name =
            fs::read_to_string(&last_path).map_err(|_| "No previous session found".to_string())?;

        Self::load(name.trim())
    }

    /// List all available sessions
    #[allow(dead_code)]
    pub fn list() -> Vec<String> {
        let mut sessions = Vec::new();

        if let Some(dir) = Self::session_dir() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            sessions.push(stem.to_string());
                        }
                    }
                }
            }
        }

        sessions.sort();
        sessions
    }

    /// Delete a session
    #[allow(dead_code)]
    pub fn delete(name: &str) -> Result<(), String> {
        let path = Self::session_path(name)
            .ok_or_else(|| "Could not determine session path".to_string())?;

        fs::remove_file(&path).map_err(|e| format!("Failed to delete session: {}", e))
    }

    /// Update the timestamp
    #[allow(dead_code)]
    pub fn touch(&mut self) {
        self.updated_at = chrono::Local::now().to_rfc3339();
    }
}

/// Sanitize a filename to remove problematic characters
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("my:session"), "my_session");
        assert_eq!(sanitize_filename("test/path"), "test_path");
        assert_eq!(sanitize_filename("normal"), "normal");
    }
}
