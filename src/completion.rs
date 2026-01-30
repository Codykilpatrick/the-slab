use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Represents a single completion suggestion
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Completion {
    /// The text to insert
    pub text: String,
    /// Display text (may differ from insert text)
    pub display: String,
    /// Optional description
    pub description: Option<String>,
    /// Completion score (higher = better match)
    pub score: f32,
    /// Type of completion
    pub kind: CompletionKind,
}

#[allow(dead_code)]
impl Completion {
    pub fn new(text: impl Into<String>, kind: CompletionKind) -> Self {
        let text = text.into();
        Self {
            display: text.clone(),
            text,
            description: None,
            score: 1.0,
            kind,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_display(mut self, display: impl Into<String>) -> Self {
        self.display = display.into();
        self
    }

    pub fn with_score(mut self, score: f32) -> Self {
        self.score = score;
        self
    }
}

/// Type of completion item
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CompletionKind {
    Command,
    File,
    Directory,
    Model,
    Session,
    Template,
    History,
    Argument,
}

#[allow(dead_code)]
impl CompletionKind {
    /// Get an icon/prefix for display
    pub fn icon(&self) -> &'static str {
        match self {
            CompletionKind::Command => "/",
            CompletionKind::File => "📄",
            CompletionKind::Directory => "📁",
            CompletionKind::Model => "🤖",
            CompletionKind::Session => "💬",
            CompletionKind::Template => "📋",
            CompletionKind::History => "⏱",
            CompletionKind::Argument => "→",
        }
    }
}

/// Context passed to completers
#[allow(dead_code)]
pub struct CompletionContext<'a> {
    /// Files currently in context
    pub context_files: Vec<&'a PathBuf>,
    /// Current working directory
    pub cwd: &'a Path,
    /// Available models (if known)
    pub models: Option<Vec<String>>,
    /// Command history
    pub history: &'a [String],
}

/// Trait for completion providers
pub trait Completer: Send + Sync {
    /// Generate completions for the given input
    fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion>;

    /// Get the name of this completer (for debugging)
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
}

/// Main completion engine that coordinates completers
pub struct CompletionEngine {
    /// Completers for specific commands (command name -> completer)
    command_completers: HashMap<String, Box<dyn Completer>>,
    /// Default completer for commands themselves
    command_list_completer: CommandCompleter,
    /// Whether fuzzy matching is enabled
    fuzzy_enabled: bool,
}

impl CompletionEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            command_completers: HashMap::new(),
            command_list_completer: CommandCompleter::new(),
            fuzzy_enabled: true,
        };

        // Register default completers
        engine.register("model", Box::new(ModelCompleter));
        engine.register("add", Box::new(FileCompleter::new()));
        engine.register("remove", Box::new(ContextFileCompleter));
        engine.register("help", Box::new(HelpCompleter));

        engine
    }

    /// Register a completer for a specific command
    pub fn register(&mut self, command: &str, completer: Box<dyn Completer>) {
        self.command_completers
            .insert(command.to_string(), completer);
    }

    /// Add template commands for completion
    pub fn add_template_commands(&mut self, templates: Vec<(String, String)>) {
        self.command_list_completer.add_templates(templates);
    }

    /// Get completions for the current input
    pub fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion> {
        let input = input.trim_start();

        // If input doesn't start with '/', check history only
        if !input.starts_with('/') {
            // For non-command input, offer history suggestions
            let history_completer = HistoryCompleter;
            let mut completions = history_completer.complete(input, context);
            self.sort_completions(&mut completions);
            return completions;
        }

        let without_slash = &input[1..];

        // Check if we're completing a command argument or the command itself
        if let Some(space_idx) = without_slash.find(' ') {
            // Completing an argument
            let command = &without_slash[..space_idx];
            let arg_input = without_slash[space_idx..].trim_start();
            let command_prefix = format!("/{} ", command);

            if let Some(completer) = self.command_completers.get(command) {
                let mut completions = completer.complete(arg_input, context);
                // Prepend the command to each completion so the full text is returned
                for completion in &mut completions {
                    completion.text = format!("{}{}", command_prefix, completion.text);
                }
                self.apply_fuzzy_scoring(&mut completions, arg_input);
                self.sort_completions(&mut completions);
                return completions;
            }

            // No specific completer for this command
            Vec::new()
        } else {
            // Completing the command name - also include matching history
            let mut completions = self.command_list_completer.complete(without_slash, context);

            // Add history matches for commands
            let history_completer = HistoryCompleter;
            let history_matches = history_completer.complete(input, context);
            completions.extend(history_matches);

            self.apply_fuzzy_scoring(&mut completions, without_slash);
            self.sort_completions(&mut completions);
            completions
        }
    }

    /// Apply fuzzy scoring to completions
    fn apply_fuzzy_scoring(&self, completions: &mut Vec<Completion>, query: &str) {
        if !self.fuzzy_enabled || query.is_empty() {
            return;
        }

        let query_lower = query.to_lowercase();

        for completion in completions.iter_mut() {
            let text_lower = completion.text.to_lowercase();

            // Exact prefix match - highest score
            if text_lower.starts_with(&query_lower) {
                completion.score = 1.0;
            }
            // Contains match - medium score
            else if text_lower.contains(&query_lower) {
                completion.score = 0.7;
            }
            // Fuzzy match - calculate score
            else if self.fuzzy_enabled {
                completion.score = fuzzy_score(&query_lower, &text_lower);
            }
        }

        // Filter out zero-score completions
        completions.retain(|c| c.score > 0.0);
    }

    /// Sort completions by score (descending)
    #[allow(clippy::ptr_arg)]
    fn sort_completions(&self, completions: &mut Vec<Completion>) {
        completions.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple fuzzy matching score
fn fuzzy_score(query: &str, target: &str) -> f32 {
    if query.is_empty() {
        return 1.0;
    }
    if target.is_empty() {
        return 0.0;
    }

    let mut query_chars = query.chars().peekable();
    let mut matched = 0;
    let mut consecutive = 0;
    let mut max_consecutive = 0;

    for c in target.chars() {
        if query_chars.peek() == Some(&c) {
            query_chars.next();
            matched += 1;
            consecutive += 1;
            max_consecutive = max_consecutive.max(consecutive);
        } else {
            consecutive = 0;
        }
    }

    // If we didn't match all query characters, no match
    if query_chars.peek().is_some() {
        return 0.0;
    }

    // Score based on match ratio and consecutive bonus
    let match_ratio = matched as f32 / target.len() as f32;
    let consecutive_bonus = max_consecutive as f32 / query.len() as f32 * 0.3;

    (match_ratio * 0.5 + consecutive_bonus).min(0.6) // Cap fuzzy matches below exact/contains
}

// ============== Built-in Completers ==============

/// Completes REPL command names
pub struct CommandCompleter {
    commands: Vec<(String, String)>, // (command, description)
}

impl CommandCompleter {
    pub fn new() -> Self {
        Self {
            commands: vec![
                ("help".into(), "Show help".into()),
                ("exit".into(), "Exit the REPL".into()),
                ("quit".into(), "Exit the REPL".into()),
                ("clear".into(), "Clear conversation".into()),
                ("model".into(), "Show/set current model".into()),
                ("context".into(), "Show context summary".into()),
                ("tokens".into(), "Show token usage".into()),
                ("files".into(), "List files in context".into()),
                ("add".into(), "Add file or directory to context".into()),
                ("remove".into(), "Remove file from context".into()),
                ("fileops".into(), "Toggle file operations".into()),
                ("templates".into(), "List available templates".into()),
                ("rules".into(), "Show loaded rules".into()),
            ],
        }
    }

    pub fn add_templates(&mut self, templates: Vec<(String, String)>) {
        for (cmd, desc) in templates {
            let cmd = cmd.trim_start_matches('/').to_string();
            if !self.commands.iter().any(|(c, _)| c == &cmd) {
                self.commands.push((cmd, desc));
            }
        }
    }
}

impl Default for CommandCompleter {
    fn default() -> Self {
        Self::new()
    }
}

impl Completer for CommandCompleter {
    fn complete(&self, input: &str, _context: &CompletionContext) -> Vec<Completion> {
        let input_lower = input.to_lowercase();

        self.commands
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(&input_lower) || cmd.contains(&input_lower))
            .map(|(cmd, desc)| {
                Completion::new(format!("/{}", cmd), CompletionKind::Command)
                    .with_description(desc.clone())
            })
            .collect()
    }

    fn name(&self) -> &'static str {
        "CommandCompleter"
    }
}

/// Completes file and directory paths
pub struct FileCompleter {
    show_hidden: bool,
}

impl FileCompleter {
    pub fn new() -> Self {
        Self { show_hidden: false }
    }
}

impl Default for FileCompleter {
    fn default() -> Self {
        Self::new()
    }
}

impl Completer for FileCompleter {
    fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion> {
        let mut completions = Vec::new();

        // Determine the directory to search and the prefix to match
        let (search_dir, prefix) = if input.is_empty() {
            (context.cwd.to_path_buf(), String::new())
        } else {
            let path = Path::new(input);
            if input.ends_with('/') || input.ends_with(std::path::MAIN_SEPARATOR) {
                (context.cwd.join(path), String::new())
            } else if let Some(parent) = path.parent() {
                let parent_dir = if parent.as_os_str().is_empty() {
                    context.cwd.to_path_buf()
                } else {
                    context.cwd.join(parent)
                };
                let prefix = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                (parent_dir, prefix)
            } else {
                (context.cwd.to_path_buf(), input.to_string())
            }
        };

        // Read directory entries
        if let Ok(entries) = std::fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                // Skip hidden files unless enabled
                if !self.show_hidden && name_str.starts_with('.') {
                    continue;
                }

                // Skip if doesn't match prefix
                if !prefix.is_empty()
                    && !name_str.to_lowercase().starts_with(&prefix.to_lowercase())
                {
                    continue;
                }

                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

                // Build the completion text
                let completion_text =
                    if input.contains('/') || input.contains(std::path::MAIN_SEPARATOR) {
                        let parent = Path::new(input).parent().unwrap_or(Path::new(""));
                        let mut path = parent.join(&name);
                        if is_dir {
                            path.push(""); // Adds trailing separator
                        }
                        path.to_string_lossy().to_string()
                    } else {
                        let mut text = name_str.to_string();
                        if is_dir {
                            text.push('/');
                        }
                        text
                    };

                let kind = if is_dir {
                    CompletionKind::Directory
                } else {
                    CompletionKind::File
                };

                completions.push(Completion::new(completion_text, kind));
            }
        }

        // Sort: directories first, then alphabetically
        completions.sort_by(|a, b| match (a.kind, b.kind) {
            (CompletionKind::Directory, CompletionKind::File) => std::cmp::Ordering::Less,
            (CompletionKind::File, CompletionKind::Directory) => std::cmp::Ordering::Greater,
            _ => a.text.to_lowercase().cmp(&b.text.to_lowercase()),
        });

        completions
    }

    fn name(&self) -> &'static str {
        "FileCompleter"
    }
}

/// Completes model names
pub struct ModelCompleter;

impl Completer for ModelCompleter {
    fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion> {
        let Some(models) = &context.models else {
            return Vec::new();
        };

        let input_lower = input.to_lowercase();

        models
            .iter()
            .filter(|m| {
                m.to_lowercase().starts_with(&input_lower)
                    || m.to_lowercase().contains(&input_lower)
            })
            .map(|m| Completion::new(m.clone(), CompletionKind::Model))
            .collect()
    }

    fn name(&self) -> &'static str {
        "ModelCompleter"
    }
}

/// Completes files currently in context (for /remove)
pub struct ContextFileCompleter;

impl Completer for ContextFileCompleter {
    fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion> {
        let input_lower = input.to_lowercase();

        context
            .context_files
            .iter()
            .filter_map(|p| p.to_str())
            .filter(|p| {
                p.to_lowercase().starts_with(&input_lower)
                    || p.to_lowercase().contains(&input_lower)
            })
            .map(|p| Completion::new(p.to_string(), CompletionKind::File))
            .collect()
    }

    fn name(&self) -> &'static str {
        "ContextFileCompleter"
    }
}

/// Completes command names for /help
pub struct HelpCompleter;

impl Completer for HelpCompleter {
    fn complete(&self, input: &str, _context: &CompletionContext) -> Vec<Completion> {
        let commands = [
            ("help", "Show help information"),
            ("exit", "Exit the REPL"),
            ("clear", "Clear conversation history"),
            ("model", "Show or change model"),
            ("context", "Show context summary"),
            ("tokens", "Show token usage"),
            ("files", "List files in context"),
            ("add", "Add file to context"),
            ("remove", "Remove file from context"),
            ("fileops", "Toggle file operations"),
            ("templates", "List templates"),
            ("rules", "Show loaded rules"),
        ];

        let input_lower = input.to_lowercase();

        commands
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(&input_lower))
            .map(|(cmd, desc)| {
                Completion::new(cmd.to_string(), CompletionKind::Command)
                    .with_description(desc.to_string())
            })
            .collect()
    }

    fn name(&self) -> &'static str {
        "HelpCompleter"
    }
}

/// Completes from command history
pub struct HistoryCompleter;

impl Completer for HistoryCompleter {
    fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion> {
        let input_lower = input.to_lowercase();

        // Get unique history entries that match, most recent first
        let mut seen = std::collections::HashSet::new();
        context
            .history
            .iter()
            .rev() // Most recent first
            .filter(|h| h.to_lowercase().starts_with(&input_lower) && seen.insert(h.to_lowercase()))
            .take(5) // Limit history suggestions
            .map(|h| {
                Completion::new(h.clone(), CompletionKind::History)
                    .with_description("from history".to_string())
                    .with_score(0.8) // Slightly lower than exact matches
            })
            .collect()
    }

    fn name(&self) -> &'static str {
        "HistoryCompleter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_score() {
        // Exact prefix should score high
        assert!(fuzzy_score("hel", "help") > 0.0);

        // Complete mismatch should score 0
        assert_eq!(fuzzy_score("xyz", "help"), 0.0);

        // Empty query matches everything
        assert_eq!(fuzzy_score("", "anything"), 1.0);
    }

    #[test]
    fn test_command_completer() {
        let completer = CommandCompleter::new();
        let context = CompletionContext {
            context_files: vec![],
            cwd: Path::new("."),
            models: None,
            history: &[],
        };

        let completions = completer.complete("he", &context);
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.text.contains("help")));
    }

    #[test]
    fn test_completion_engine() {
        let engine = CompletionEngine::new();
        let context = CompletionContext {
            context_files: vec![],
            cwd: Path::new("."),
            models: Some(vec!["llama3".to_string(), "codellama".to_string()]),
            history: &[],
        };

        // Test command completion
        let completions = engine.complete("/he", &context);
        assert!(completions.iter().any(|c| c.text == "/help"));

        // Test model completion
        let completions = engine.complete("/model ll", &context);
        assert!(completions.iter().any(|c| c.text == "llama3"));
    }
}
