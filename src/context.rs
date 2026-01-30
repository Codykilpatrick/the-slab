use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ollama::Message;

/// Simple offline tokenizer using chars/4 approximation
pub fn estimate_tokens(text: &str) -> usize {
    // Simple approximation: ~4 characters per token on average
    // This is a reasonable estimate for English text and code
    text.len() / 4
}

/// Manages conversation context including files, messages, and token budget
#[derive(Debug, Clone)]
pub struct ContextManager {
    /// System prompt (always included first)
    system_prompt: Option<String>,

    /// Rules content (injected after system prompt)
    rules: Option<String>,

    /// Files added to context (path -> content)
    files: HashMap<PathBuf, String>,

    /// Conversation messages
    messages: Vec<Message>,

    /// Maximum tokens allowed in context
    token_budget: usize,

    /// Project root for resolving relative paths
    project_root: PathBuf,
}

impl ContextManager {
    pub fn new(token_budget: usize, project_root: PathBuf) -> Self {
        Self {
            system_prompt: None,
            rules: None,
            files: HashMap::new(),
            messages: Vec::new(),
            token_budget,
            project_root,
        }
    }

    /// Set the system prompt
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Set the rules content
    pub fn set_rules(&mut self, rules: impl Into<String>) {
        self.rules = Some(rules.into());
    }

    /// Add a file to the context
    pub fn add_file(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };

        if !full_path.exists() {
            return Err(format!("File not found: {}", path.display()));
        }

        if !full_path.is_file() {
            return Err(format!("Not a file: {}", path.display()));
        }

        let content =
            fs::read_to_string(&full_path).map_err(|e| format!("Failed to read file: {}", e))?;

        // Store with relative path for display
        let display_path = path.to_path_buf();
        self.files.insert(display_path, content);

        Ok(())
    }

    /// Add all files in a directory to the context (recursively)
    /// Returns the number of files added and a list of skipped files
    pub fn add_directory(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(usize, Vec<String>), String> {
        let path = path.as_ref();
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };

        if !full_path.exists() {
            return Err(format!("Directory not found: {}", path.display()));
        }

        if !full_path.is_dir() {
            return Err(format!("Not a directory: {}", path.display()));
        }

        let mut added = 0;
        let mut skipped = Vec::new();

        // Walk the directory recursively
        for entry in walkdir::WalkDir::new(&full_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_hidden(e) && !is_ignored_dir(e))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();

            // Skip binary files
            if is_likely_binary(file_path) {
                skipped.push(format!("{} (binary)", file_path.display()));
                continue;
            }

            // Try to read the file
            match fs::read_to_string(file_path) {
                Ok(content) => {
                    // Use relative path from the original path argument
                    let relative = file_path
                        .strip_prefix(&full_path)
                        .map(|p| path.join(p))
                        .unwrap_or_else(|_| file_path.to_path_buf());
                    self.files.insert(relative, content);
                    added += 1;
                }
                Err(_) => {
                    skipped.push(format!("{} (unreadable)", file_path.display()));
                }
            }
        }

        Ok((added, skipped))
    }

    /// Check if the path is a directory
    pub fn is_directory(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };
        full_path.is_dir()
    }

    /// Remove a file from the context
    pub fn remove_file(&mut self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref().to_path_buf();
        self.files.remove(&path).is_some()
    }

    /// Get list of files in context
    pub fn list_files(&self) -> Vec<&PathBuf> {
        self.files.keys().collect()
    }

    /// Check if a file is in context
    #[allow(dead_code)]
    pub fn has_file(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    /// Get file content from context
    pub fn get_file_content(&self, path: impl AsRef<Path>) -> Option<&String> {
        self.files.get(path.as_ref())
    }

    /// Add a message to the conversation
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Get all messages
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Clear conversation messages (but keep files)
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    /// Clear everything except system prompt
    #[allow(dead_code)]
    pub fn clear_all(&mut self) {
        self.files.clear();
        self.messages.clear();
        self.rules = None;
    }

    /// Build the full message list for sending to the LLM
    pub fn build_messages(&self) -> Vec<Message> {
        let mut result = Vec::new();

        // 1. System prompt with rules and files
        let system_content = self.build_system_content();
        if !system_content.is_empty() {
            result.push(Message::system(system_content));
        }

        // 2. Conversation messages
        result.extend(self.messages.clone());

        result
    }

    /// Build the system content including prompt, rules, and files
    fn build_system_content(&self) -> String {
        let mut parts = Vec::new();

        // System prompt
        if let Some(prompt) = &self.system_prompt {
            parts.push(prompt.clone());
        }

        // Rules
        if let Some(rules) = &self.rules {
            parts.push(format!("## Rules\n\n{}", rules));
        }

        // Files in context
        if !self.files.is_empty() {
            let mut files_section = String::from("## Files in Context\n\n");
            for (path, content) in &self.files {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");
                files_section.push_str(&format!(
                    "### {}\n```{}\n{}\n```\n\n",
                    path.display(),
                    ext,
                    content
                ));
            }
            parts.push(files_section);
        }

        parts.join("\n\n")
    }

    /// Calculate total token count for current context
    pub fn token_count(&self) -> usize {
        let messages = self.build_messages();
        messages.iter().map(|m| estimate_tokens(&m.content)).sum()
    }

    /// Get token budget
    #[allow(dead_code)]
    pub fn token_budget(&self) -> usize {
        self.token_budget
    }

    /// Get remaining token budget
    #[allow(dead_code)]
    pub fn tokens_remaining(&self) -> usize {
        let used = self.token_count();
        self.token_budget.saturating_sub(used)
    }

    /// Check if context is over budget
    #[allow(dead_code)]
    pub fn is_over_budget(&self) -> bool {
        self.token_count() > self.token_budget
    }

    /// Prune old messages to fit within token budget
    /// Keeps system prompt and recent messages, removes oldest user/assistant pairs
    #[allow(dead_code)]
    pub fn prune_to_fit(&mut self) {
        while self.is_over_budget() && self.messages.len() > 2 {
            // Remove the oldest user message and its response
            // Skip index 0 if it's a system message that was moved to messages
            let remove_idx = if self.messages.first().map(|m| m.role.as_str()) == Some("system") {
                1
            } else {
                0
            };

            if remove_idx < self.messages.len() {
                self.messages.remove(remove_idx);
                // Also remove the response if there is one
                if remove_idx < self.messages.len() {
                    self.messages.remove(remove_idx);
                }
            } else {
                break;
            }
        }
    }

    /// Get a summary of context state
    pub fn summary(&self) -> ContextSummary {
        ContextSummary {
            files_count: self.files.len(),
            messages_count: self.messages.len(),
            tokens_used: self.token_count(),
            token_budget: self.token_budget,
            has_system_prompt: self.system_prompt.is_some(),
            has_rules: self.rules.is_some(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextSummary {
    pub files_count: usize,
    pub messages_count: usize,
    pub tokens_used: usize,
    pub token_budget: usize,
    pub has_system_prompt: bool,
    pub has_rules: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("test"), 1);
        assert_eq!(estimate_tokens("hello world"), 2);
    }

    #[test]
    fn test_context_manager_messages() {
        let mut ctx = ContextManager::new(4096, PathBuf::from("."));
        ctx.set_system_prompt("You are a helpful assistant.");
        ctx.add_message(Message::user("Hello"));
        ctx.add_message(Message::assistant("Hi there!"));

        let messages = ctx.build_messages();
        assert_eq!(messages.len(), 3); // system + user + assistant
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[2].role, "assistant");
    }
}

/// Check if a directory entry is hidden (starts with .)
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

/// Check if a directory should be ignored (node_modules, target, etc.)
fn is_ignored_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }

    let ignored = [
        "node_modules",
        "target",
        "dist",
        "build",
        "__pycache__",
        ".git",
        ".svn",
        ".hg",
        "vendor",
        "venv",
        ".venv",
        "env",
        ".env",
    ];

    entry
        .file_name()
        .to_str()
        .map(|s| ignored.contains(&s))
        .unwrap_or(false)
}

/// Check if a file is likely binary based on extension
fn is_likely_binary(path: &Path) -> bool {
    let binary_extensions = [
        // Images
        "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", // Audio/Video
        "mp3", "mp4", "wav", "avi", "mov", "flv", "wmv", "webm", // Archives
        "zip", "tar", "gz", "bz2", "7z", "rar", "xz", // Executables
        "exe", "dll", "so", "dylib", "bin", "o", "a", // Documents
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", // Fonts
        "ttf", "otf", "woff", "woff2", "eot", // Other binary
        "pyc", "pyo", "class", "jar", "war", "sqlite", "db", "sqlite3",
        "lock", // Often large and not useful
    ];

    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| binary_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}
