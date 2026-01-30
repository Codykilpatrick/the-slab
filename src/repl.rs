use console::{style, Term};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::completion::{CompletionContext, CompletionEngine, CompletionKind};
use crate::config::Config;
use crate::context::ContextManager;
use crate::error::Result;
use crate::file_ops::{execute_operations, parse_file_operations, FileOperationUI};
use crate::highlight::Highlighter;
use crate::ollama::{ChatRequest, Message, ModelOptions, OllamaClient};
use crate::rules::RuleEngine;
use crate::session::Session;
use crate::templates::TemplateManager;

pub struct Repl {
    client: OllamaClient,
    config: Config,
    model: String,
    context: ContextManager,
    templates: TemplateManager,
    rules: RuleEngine,
    highlighter: Highlighter,
    completion_engine: CompletionEngine,
    streaming: bool,
    project_root: PathBuf,
    file_ops_enabled: bool,
    history: Vec<String>,
    cached_models: Option<Vec<String>>,
    #[allow(dead_code)]
    history_index: usize,
}

impl Repl {
    pub fn new(client: OllamaClient, config: Config, model: String, streaming: bool) -> Self {
        // Get current directory as project root
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Create context manager
        let mut context = ContextManager::new(config.context_limit, project_root.clone());

        // Set system prompt if configured
        let model_config = config.get_model_config(&model);
        if let Some(system_prompt) = &model_config.system_prompt {
            context.set_system_prompt(system_prompt.clone());
        }

        // Load templates
        let mut templates = TemplateManager::new();
        templates.load_defaults();

        // Load templates from directories
        let template_dirs = get_template_directories(&project_root);
        templates.load_from_directories(&template_dirs);

        // Load rules
        let mut rules = RuleEngine::new();
        let rules_dir = project_root.join(".slab/rules");
        rules.load_from_directory(&rules_dir);

        // Inject rules into context if any were loaded
        if rules.rule_count() > 0 {
            let files: Vec<PathBuf> = context.list_files().iter().map(|p| (*p).clone()).collect();
            if let Some(rules_prompt) = rules.build_rules_prompt(&files) {
                context.set_rules(rules_prompt);
            }
        }

        // Create highlighter for syntax highlighting
        let highlighter = Highlighter::new();

        // Create completion engine with template commands
        let mut completion_engine = CompletionEngine::new();
        let template_cmds: Vec<(String, String)> = templates
            .list()
            .iter()
            .map(|t| (t.command.clone(), t.description.clone()))
            .collect();
        completion_engine.add_template_commands(template_cmds);

        Self {
            client,
            config,
            model,
            context,
            templates,
            rules,
            highlighter,
            completion_engine,
            streaming,
            project_root,
            file_ops_enabled: true,
            history: Vec::new(),
            cached_models: None,
            history_index: 0,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let term = Term::stdout();
        term.clear_screen().ok();

        // Cache available models for completion
        if let Ok(models) = self.client.list_models().await {
            self.cached_models = Some(models.into_iter().map(|m| m.name).collect());
        }

        self.print_welcome();

        loop {
            match self.read_input()? {
                Some(input) => {
                    let trimmed = input.trim();

                    // Handle commands
                    if trimmed.starts_with('/') {
                        if self.handle_command(trimmed).await? {
                            continue;
                        } else {
                            break;
                        }
                    }

                    if trimmed.is_empty() {
                        continue;
                    }

                    // Send message to Ollama
                    self.send_message(trimmed).await?;
                }
                None => {
                    // Ctrl+D pressed
                    println!("\n{}", style("Goodbye!").dim());
                    break;
                }
            }
        }

        Ok(())
    }

    fn print_welcome(&self) {
        println!(
            "{} {} {}",
            style("The Slab").cyan().bold(),
            style("·").dim(),
            style(&self.model).yellow()
        );
        println!("{}", style("Type /help for commands, /exit to quit").dim());
        println!();
    }

    fn print_prompt(&self) {
        let summary = self.context.summary();
        let token_info = format!("{}t", summary.tokens_used);

        print!(
            "{}{}{} ",
            style("[").dim(),
            style(&self.model).yellow(),
            style("]").dim()
        );

        // Show file count and tokens if there are files
        if summary.files_count > 0 {
            print!(
                "{}{} {}{}{}",
                style("[").dim(),
                style(format!("{}f", summary.files_count)).cyan(),
                style("|").dim(),
                style(&token_info).dim(),
                style("] ").dim()
            );
        }

        print!("{} ", style("➜").cyan());
        io::stdout().flush().ok();
    }

    fn read_input(&mut self) -> Result<Option<String>> {
        self.print_prompt();

        let mut input = String::new();
        let mut stdout = io::stdout();
        let mut history_index = self.history.len();
        let mut saved_input = String::new();
        let mut current_preview: Option<String> = None;

        // Enable raw mode for better input handling
        crossterm::terminal::enable_raw_mode().ok();

        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key_event)) = event::read() {
                    // Clear any existing preview before processing
                    if let Some(ref preview) = current_preview {
                        self.clear_preview(preview.len());
                    }
                    current_preview = None;

                    match (key_event.code, key_event.modifiers) {
                        // Ctrl+C - cancel current input
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            crossterm::terminal::disable_raw_mode().ok();
                            println!();
                            return Ok(Some(String::new()));
                        }
                        // Ctrl+D - exit
                        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            crossterm::terminal::disable_raw_mode().ok();
                            return Ok(None);
                        }
                        // Ctrl+L - clear screen
                        (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                            crossterm::terminal::disable_raw_mode().ok();
                            Term::stdout().clear_screen().ok();
                            self.print_welcome();
                            return Ok(Some(String::new()));
                        }
                        // Enter - submit
                        (KeyCode::Enter, _) => {
                            crossterm::terminal::disable_raw_mode().ok();
                            println!();
                            // Add to history if non-empty
                            if !input.trim().is_empty() {
                                self.history.push(input.clone());
                            }
                            return Ok(Some(input));
                        }
                        // Right arrow - accept one character of preview or move cursor
                        (KeyCode::Right, _) => {
                            if let Some(preview) = self.get_inline_preview(&input) {
                                if !preview.is_empty() {
                                    // Accept first character of preview
                                    let first_char = preview.chars().next().unwrap();
                                    input.push(first_char);
                                    print!("{}", first_char);
                                    stdout.flush().ok();
                                }
                            }
                        }
                        // Up arrow - history back
                        (KeyCode::Up, _) => {
                            if !self.history.is_empty() && history_index > 0 {
                                // Save current input on first up
                                if history_index == self.history.len() {
                                    saved_input = input.clone();
                                }
                                history_index -= 1;
                                // Clear current line
                                self.clear_input(&input);
                                input = self.history[history_index].clone();
                                print!("{}", input);
                                stdout.flush().ok();
                            }
                        }
                        // Down arrow - history forward
                        (KeyCode::Down, _) => {
                            if history_index < self.history.len() {
                                // Clear current line
                                self.clear_input(&input);
                                history_index += 1;
                                if history_index == self.history.len() {
                                    input = saved_input.clone();
                                } else {
                                    input = self.history[history_index].clone();
                                }
                                print!("{}", input);
                                stdout.flush().ok();
                            }
                        }
                        // Tab - command completion
                        (KeyCode::Tab, _) => {
                            let completions = self.get_completions(&input);
                            if completions.len() == 1 {
                                // Single match - auto-complete
                                self.clear_input(&input);
                                input = completions[0].0.clone();
                                print!("{}", input);
                                stdout.flush().ok();
                            } else if !completions.is_empty() {
                                // Multiple matches - show completion menu
                                // Temporarily disable raw mode for proper newlines
                                crossterm::terminal::disable_raw_mode().ok();
                                println!();
                                self.show_completion_menu(&completions);
                                self.print_prompt();
                                print!("{}", input);
                                stdout.flush().ok();
                                // Re-enable raw mode for input handling
                                crossterm::terminal::enable_raw_mode().ok();
                            }
                        }
                        // Backspace
                        (KeyCode::Backspace, _) => {
                            if !input.is_empty() {
                                input.pop();
                                print!("\x08 \x08");
                                stdout.flush().ok();
                            }
                        }
                        // Regular character
                        (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                            input.push(c);
                            print!("{}", c);
                            stdout.flush().ok();
                        }
                        _ => {}
                    }

                    // Show inline preview after input changes
                    if input.starts_with('/') && self.config.ui.inline_completion_preview {
                        if let Some(preview) = self.get_inline_preview(&input) {
                            if !preview.is_empty() {
                                // Show dimmed preview text
                                print!("{}", style(&preview).dim());
                                // Move cursor back to end of actual input
                                for _ in 0..preview.len() {
                                    print!("\x08");
                                }
                                stdout.flush().ok();
                                current_preview = Some(preview);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Clear the input text from the terminal
    fn clear_input(&self, input: &str) {
        for _ in 0..input.len() {
            print!("\x08 \x08");
        }
    }

    /// Clear the preview text (spaces over it)
    fn clear_preview(&self, len: usize) {
        // Move forward, overwrite with spaces, move back
        for _ in 0..len {
            print!(" ");
        }
        for _ in 0..len {
            print!("\x08");
        }
        for _ in 0..len {
            print!("\x08");
        }
        for _ in 0..len {
            print!(" ");
        }
        for _ in 0..len {
            print!("\x08");
        }
    }

    /// Get the inline preview text (the part to show dimmed after cursor)
    fn get_inline_preview(&self, input: &str) -> Option<String> {
        let completions = self.get_completions(input);
        if let Some((text, _, _)) = completions.first() {
            // Return the part of the completion that extends beyond current input
            if text.len() > input.len() && text.starts_with(input) {
                return Some(text[input.len()..].to_string());
            }
        }
        None
    }

    /// Get completions using the completion engine
    /// Returns Vec<(text, description, kind)>
    fn get_completions(&self, input: &str) -> Vec<(String, Option<String>, CompletionKind)> {
        let context_files: Vec<&PathBuf> = self.context.list_files();

        let ctx = CompletionContext {
            context_files,
            cwd: &self.project_root,
            models: self.cached_models.clone(),
            history: &self.history,
        };

        self.completion_engine
            .complete(input, &ctx)
            .into_iter()
            .map(|c| (c.text, c.description, c.kind))
            .collect()
    }

    /// Display a formatted completion menu (non-interactive, for display only)
    fn show_completion_menu(&self, completions: &[(String, Option<String>, CompletionKind)]) {
        let max_items = self.config.ui.max_completion_items;

        // Find the max width for alignment
        let max_text_len = completions
            .iter()
            .map(|(t, _, _)| t.len())
            .max()
            .unwrap_or(10);

        // Print top border
        let width = (max_text_len + 35).min(60);
        println!(
            "{}",
            style(format!("╭─ Completions {}╮", "─".repeat(width - 15))).dim()
        );

        // Print completions
        for (idx, (text, desc, kind)) in completions.iter().take(max_items).enumerate() {
            self.print_completion_item(idx, None, text, desc.as_deref(), *kind, max_text_len);
        }

        if completions.len() > max_items {
            println!(
                "│ {} {}│",
                style("...").dim(),
                style(format!("and {} more", completions.len() - max_items)).dim()
            );
        }

        // Print bottom border with hint
        println!(
            "{}",
            style(format!(
                "╰─ ↑↓ navigate, Enter select, Esc cancel {}╯",
                "─".repeat(width.saturating_sub(40))
            ))
            .dim()
        );
    }

    /// Print a single completion item
    fn print_completion_item(
        &self,
        _idx: usize,
        selected: Option<usize>,
        text: &str,
        desc: Option<&str>,
        kind: CompletionKind,
        max_text_len: usize,
    ) {
        let icon = match kind {
            CompletionKind::Command => style("/").cyan(),
            CompletionKind::File => style("📄").white(),
            CompletionKind::Directory => style("📁").yellow(),
            CompletionKind::Model => style("🤖").magenta(),
            CompletionKind::Template => style("📋").green(),
            CompletionKind::History => style("⏱").dim(),
            _ => style(" ").white(),
        };

        let is_selected = selected == Some(_idx);
        let prefix = if is_selected { ">" } else { " " };

        let padded_text = format!("{:width$}", text, width = max_text_len + 2);
        let text_styled = if is_selected {
            style(padded_text).bold().reverse()
        } else {
            match kind {
                CompletionKind::Command => style(padded_text).cyan(),
                CompletionKind::Directory => style(padded_text).yellow(),
                CompletionKind::Model => style(padded_text).magenta(),
                CompletionKind::History => style(padded_text).dim(),
                _ => style(padded_text).green(),
            }
        };

        if let Some(d) = desc {
            let truncated: String = d.chars().take(30).collect();
            println!(
                "│{} {} {} {}│",
                style(prefix).cyan(),
                icon,
                text_styled,
                style(truncated).dim()
            );
        } else {
            println!("│{} {} {}│", style(prefix).cyan(), icon, text_styled);
        }
    }

    /// Show interactive completion menu with arrow key navigation
    /// Returns the selected completion text or None if cancelled
    #[allow(dead_code)]
    fn show_interactive_completion_menu(
        &self,
        completions: &[(String, Option<String>, CompletionKind)],
    ) -> Option<String> {
        if completions.is_empty() {
            return None;
        }

        let max_items = self.config.ui.max_completion_items;
        let visible_items: Vec<_> = completions.iter().take(max_items).collect();
        let mut selected: usize = 0;
        let mut stdout = io::stdout();

        // Calculate dimensions
        let max_text_len = visible_items
            .iter()
            .map(|(t, _, _)| t.len())
            .max()
            .unwrap_or(10);
        let width = (max_text_len + 35).min(60);
        let menu_height = visible_items.len() + 3; // items + borders + hint

        // Print initial menu
        println!(
            "{}",
            style(format!("╭─ Completions {}╮", "─".repeat(width - 15))).dim()
        );
        for (idx, (text, desc, kind)) in visible_items.iter().enumerate() {
            self.print_completion_item(
                idx,
                Some(selected),
                text,
                desc.as_deref(),
                *kind,
                max_text_len,
            );
        }
        if completions.len() > max_items {
            println!(
                "│ {} {}│",
                style("...").dim(),
                style(format!("and {} more", completions.len() - max_items)).dim()
            );
        }
        println!(
            "{}",
            style(format!(
                "╰─ ↑↓ navigate, Enter select, Esc cancel {}╯",
                "─".repeat(width.saturating_sub(40))
            ))
            .dim()
        );
        stdout.flush().ok();

        // Interactive loop
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key_event)) = event::read() {
                    match key_event.code {
                        KeyCode::Up => {
                            if selected > 0 {
                                selected -= 1;
                                self.redraw_menu(
                                    &visible_items,
                                    selected,
                                    max_text_len,
                                    width,
                                    menu_height,
                                );
                            }
                        }
                        KeyCode::Down => {
                            if selected < visible_items.len() - 1 {
                                selected += 1;
                                self.redraw_menu(
                                    &visible_items,
                                    selected,
                                    max_text_len,
                                    width,
                                    menu_height,
                                );
                            }
                        }
                        KeyCode::Enter => {
                            // Clear menu and return selection
                            self.clear_menu_lines(menu_height);
                            return Some(visible_items[selected].0.clone());
                        }
                        KeyCode::Esc => {
                            // Clear menu and cancel
                            self.clear_menu_lines(menu_height);
                            return None;
                        }
                        KeyCode::Tab => {
                            // Tab also selects
                            self.clear_menu_lines(menu_height);
                            return Some(visible_items[selected].0.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Redraw the completion menu (moves cursor up, redraws, moves back)
    fn redraw_menu(
        &self,
        items: &[&(String, Option<String>, CompletionKind)],
        selected: usize,
        max_text_len: usize,
        width: usize,
        menu_height: usize,
    ) {
        let mut stdout = io::stdout();

        // Move cursor up to start of menu
        print!("\x1b[{}A", menu_height);

        // Redraw
        println!(
            "{}",
            style(format!("╭─ Completions {}╮", "─".repeat(width - 15))).dim()
        );
        for (idx, (text, desc, kind)) in items.iter().enumerate() {
            self.print_completion_item(
                idx,
                Some(selected),
                text,
                desc.as_deref(),
                *kind,
                max_text_len,
            );
        }
        println!(
            "{}",
            style(format!(
                "╰─ ↑↓ navigate, Enter select, Esc cancel {}╯",
                "─".repeat(width.saturating_sub(40))
            ))
            .dim()
        );

        stdout.flush().ok();
    }

    /// Clear menu lines from terminal
    fn clear_menu_lines(&self, lines: usize) {
        // Move up and clear each line
        for _ in 0..lines {
            print!("\x1b[1A\x1b[2K");
        }
        io::stdout().flush().ok();
    }

    async fn handle_command(&mut self, command: &str) -> Result<bool> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let cmd = parts.first().unwrap_or(&"").trim_start_matches('/');

        // Check if it's a template command first
        if self.templates.is_template_command(cmd) {
            return self.handle_template_command(cmd, &parts[1..]).await;
        }

        match cmd {
            "help" => {
                if parts.len() > 1 {
                    self.print_command_help(parts[1]);
                } else {
                    self.print_help();
                }
                Ok(true)
            }
            "exit" | "quit" | "q" => Ok(false),
            "clear" => {
                Term::stdout().clear_screen().ok();
                self.context.clear_messages();
                println!("{}", style("Conversation cleared.").dim());
                println!();
                Ok(true)
            }
            "model" => {
                if parts.len() > 1 {
                    self.model = parts[1].to_string();
                    println!(
                        "{} {}",
                        style("Switched to model:").dim(),
                        style(&self.model).yellow()
                    );
                } else {
                    println!(
                        "{} {}",
                        style("Current model:").dim(),
                        style(&self.model).yellow()
                    );
                }
                Ok(true)
            }
            "context" => {
                let summary = self.context.summary();
                println!("{}", style("Context:").cyan().bold());
                println!("  {} {}", style("Messages:").dim(), summary.messages_count);
                println!("  {} {}", style("Files:").dim(), summary.files_count);
                println!(
                    "  {} {} / {}",
                    style("Tokens:").dim(),
                    summary.tokens_used,
                    summary.token_budget
                );
                println!(
                    "  {} {}",
                    style("System prompt:").dim(),
                    if summary.has_system_prompt {
                        "yes"
                    } else {
                        "no"
                    }
                );
                println!(
                    "  {} {}",
                    style("Rules:").dim(),
                    if summary.has_rules { "yes" } else { "no" }
                );
                Ok(true)
            }
            "tokens" => {
                let summary = self.context.summary();
                println!(
                    "{} {} / {} tokens",
                    style("Context:").dim(),
                    summary.tokens_used,
                    summary.token_budget
                );
                let remaining = summary.token_budget.saturating_sub(summary.tokens_used);
                let pct =
                    (summary.tokens_used as f64 / summary.token_budget as f64 * 100.0) as usize;
                println!(
                    "{} {} remaining ({}% used)",
                    style("Budget:").dim(),
                    remaining,
                    pct
                );
                Ok(true)
            }
            "files" => {
                let files = self.context.list_files();
                if files.is_empty() {
                    println!("{}", style("No files in context.").dim());
                } else {
                    println!("{}", style("Files in context:").cyan().bold());
                    for path in files {
                        let content = self.context.get_file_content(path);
                        let tokens = content.map(|c| c.len() / 4).unwrap_or(0);
                        println!(
                            "  {} {}",
                            style(path.display()).green(),
                            style(format!("(~{} tokens)", tokens)).dim()
                        );
                    }
                }
                Ok(true)
            }
            "add" => {
                if parts.len() < 2 {
                    println!("{} /add <file|directory>", style("Usage:").dim());
                    return Ok(true);
                }
                let path = parts[1..].join(" ");

                // Check if it's a directory
                if self.context.is_directory(&path) {
                    match self.context.add_directory(&path) {
                        Ok((added, skipped)) => {
                            println!(
                                "{} Added {} file(s) from {} to context",
                                style("✓").green(),
                                style(added).cyan(),
                                style(&path).cyan()
                            );
                            if !skipped.is_empty() && skipped.len() <= 5 {
                                println!("{} Skipped:", style("⚠").yellow());
                                for s in &skipped {
                                    println!("    {}", style(s).dim());
                                }
                            } else if !skipped.is_empty() {
                                println!(
                                    "{} Skipped {} file(s) (binary/unreadable)",
                                    style("⚠").yellow(),
                                    skipped.len()
                                );
                            }
                            // Update rules based on new file list
                            self.update_rules_for_context();
                        }
                        Err(e) => {
                            println!("{} {}", style("Error:").red(), e);
                        }
                    }
                } else {
                    // Single file
                    match self.context.add_file(&path) {
                        Ok(()) => {
                            println!(
                                "{} Added {} to context",
                                style("✓").green(),
                                style(&path).cyan()
                            );
                            // Update rules based on new file list
                            self.update_rules_for_context();
                        }
                        Err(e) => {
                            println!("{} {}", style("Error:").red(), e);
                        }
                    }
                }
                Ok(true)
            }
            "remove" | "rm" => {
                if parts.len() < 2 {
                    println!("{} /remove <file>", style("Usage:").dim());
                    return Ok(true);
                }
                let path = parts[1..].join(" ");
                if self.context.remove_file(&path) {
                    println!(
                        "{} Removed {} from context",
                        style("✓").green(),
                        style(&path).cyan()
                    );
                    // Update rules based on new file list
                    self.update_rules_for_context();
                } else {
                    println!(
                        "{} File not in context: {}",
                        style("✗").red(),
                        style(&path).cyan()
                    );
                }
                Ok(true)
            }
            "fileops" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "on" | "enable" => {
                            self.file_ops_enabled = true;
                            println!("{}", style("File operations enabled").green());
                        }
                        "off" | "disable" => {
                            self.file_ops_enabled = false;
                            println!("{}", style("File operations disabled").yellow());
                        }
                        _ => {
                            println!("{} /fileops [on|off]", style("Usage:").dim());
                        }
                    }
                } else {
                    println!(
                        "{} {}",
                        style("File operations:").dim(),
                        if self.file_ops_enabled {
                            style("enabled").green()
                        } else {
                            style("disabled").yellow()
                        }
                    );
                }
                Ok(true)
            }
            "templates" => {
                let templates = self.templates.list();
                if templates.is_empty() {
                    println!("{}", style("No templates loaded.").dim());
                } else {
                    println!("{}", style("Available templates:").cyan().bold());
                    for t in templates {
                        println!(
                            "  {} - {}",
                            style(&t.command).green(),
                            style(&t.description).dim()
                        );
                    }
                }
                Ok(true)
            }
            "rules" => {
                let rules = self.rules.all_rules();
                if rules.is_empty() {
                    println!("{}", style("No rules loaded.").dim());
                    println!(
                        "{}",
                        style("Add rules to .slab/rules/ (.yaml, .md, or .txt)").dim()
                    );
                } else {
                    println!("{}", style("Loaded rules:").cyan().bold());
                    for rule in rules {
                        print!("  {}", style(&rule.name).green());
                        if let Some(desc) = &rule.description {
                            print!(" - {}", style(desc).dim());
                        }
                        println!();
                        if !rule.applies_to.is_empty() {
                            println!(
                                "    {}",
                                style(format!("applies to: {}", rule.applies_to.join(", "))).dim()
                            );
                        }
                    }
                }
                Ok(true)
            }
            _ => {
                println!("{} {}", style("Unknown command:").red(), command);
                println!("{}", style("Type /help for available commands.").dim());
                Ok(true)
            }
        }
    }

    async fn handle_template_command(&mut self, cmd: &str, args: &[&str]) -> Result<bool> {
        let template = match self.templates.get(cmd) {
            Some(t) => t.clone(),
            None => {
                println!("{} Template not found: {}", style("Error:").red(), cmd);
                return Ok(true);
            }
        };

        // Parse arguments as key=value pairs or positional content
        let mut variables = HashMap::new();
        let mut content_parts = Vec::new();

        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                variables.insert(key.to_string(), value.to_string());
            } else {
                content_parts.push(*arg);
            }
        }

        // Join remaining args as content
        if !content_parts.is_empty() {
            variables.insert("content".to_string(), content_parts.join(" "));
        }

        // Render the template
        let prompt = match self
            .templates
            .render(&template.name, &variables, &self.context)
        {
            Ok(p) => p,
            Err(e) => {
                println!("{} {}", style("Template error:").red(), e);
                return Ok(true);
            }
        };

        // Show what we're sending
        println!(
            "{} {} {}",
            style("→").cyan(),
            style("Using template:").dim(),
            style(&template.name).yellow()
        );

        // Send the rendered prompt
        self.send_message(&prompt).await?;

        Ok(true)
    }

    fn print_help(&self) {
        println!("{}", style("Built-in commands:").cyan().bold());
        println!("  {}  - Show this help", style("/help").green());
        println!("  {}  - Exit the REPL", style("/exit").green());
        println!("  {} - Clear conversation", style("/clear").green());
        println!(
            "  {} - Show/set current model",
            style("/model [name]").green()
        );
        println!("  {} - Show context summary", style("/context").green());
        println!("  {} - Show token usage", style("/tokens").green());
        println!("  {} - List files in context", style("/files").green());
        println!(
            "  {} - Add file or directory to context",
            style("/add <path>").green()
        );
        println!(
            "  {} - Remove file from context",
            style("/remove <file>").green()
        );
        println!(
            "  {} - Toggle file operations",
            style("/fileops [on|off]").green()
        );
        println!(
            "  {} - List available templates",
            style("/templates").green()
        );
        println!("  {} - Show loaded rules", style("/rules").green());

        // Show template commands
        let templates = self.templates.list();
        if !templates.is_empty() {
            println!();
            println!("{}", style("Template commands:").cyan().bold());
            for t in templates {
                println!(
                    "  {} - {}",
                    style(&t.command).green(),
                    style(&t.description).dim()
                );
            }
        }

        println!();
        println!("{}", style("Keyboard shortcuts:").cyan().bold());
        println!("  {} - Cancel current input", style("Ctrl+C").yellow());
        println!("  {} - Exit", style("Ctrl+D").yellow());
        println!("  {} - Clear screen", style("Ctrl+L").yellow());
        println!("  {} - Navigate command history", style("Up/Down").yellow());
        println!("  {} - Autocomplete commands", style("Tab").yellow());
        println!();
        println!(
            "{}",
            style("Type /help <command> for detailed command help.").dim()
        );
        println!();
    }

    fn print_command_help(&self, cmd: &str) {
        let cmd = cmd.trim_start_matches('/');

        let help = match cmd {
            "help" => (
                "/help [command]",
                "Display help information",
                "Shows a list of all available commands. If a command name is provided, \
                 shows detailed help for that specific command.\n\n\
                 Examples:\n  /help        - Show all commands\n  /help add    - Show help for /add",
            ),
            "exit" | "quit" | "q" => (
                "/exit, /quit, /q",
                "Exit the REPL",
                "Exits the chat session. You can also use Ctrl+D to exit.",
            ),
            "clear" => (
                "/clear",
                "Clear conversation history",
                "Clears all conversation messages from the current session. Files added \
                 to context are preserved. Use Ctrl+L to clear the screen without clearing history.",
            ),
            "model" => (
                "/model [name]",
                "Show or change the current model",
                "Without arguments, shows the current model. With a model name, switches \
                 to that model for subsequent messages.\n\n\
                 Examples:\n  /model           - Show current model\n  /model qwen2.5:7b - Switch to qwen2.5:7b",
            ),
            "context" => (
                "/context",
                "Show context summary",
                "Displays a summary of the current context including message count, \
                 files in context, token usage, and whether system prompt/rules are set.",
            ),
            "tokens" => (
                "/tokens",
                "Show token usage",
                "Shows the current token count and remaining budget. Token count is \
                 estimated using a chars/4 approximation.",
            ),
            "files" => (
                "/files",
                "List files in context",
                "Shows all files currently added to the context, along with their \
                 approximate token counts.",
            ),
            "add" => (
                "/add <file|directory>",
                "Add a file or directory to context",
                "Adds file contents to the conversation context. If a directory is specified, \
                 all files are added recursively.\n\n\
                 Directories skip: hidden files, node_modules, target, .git, binary files\n\n\
                 Examples:\n  /add src/main.rs\n  /add src/\n  /add ../other/",
            ),
            "remove" | "rm" => (
                "/remove <file>, /rm <file>",
                "Remove a file from context",
                "Removes a previously added file from the context.\n\n\
                 Example:\n  /remove src/main.rs",
            ),
            "fileops" => (
                "/fileops [on|off]",
                "Toggle file operations",
                "Controls whether the REPL automatically detects and offers to apply \
                 file operations from model responses. When enabled, code blocks with \
                 filenames (e.g., ```rust:src/main.rs) are parsed as file operations.\n\n\
                 Examples:\n  /fileops     - Show current status\n  /fileops on  - Enable\n  /fileops off - Disable",
            ),
            "templates" => (
                "/templates",
                "List available templates",
                "Shows all loaded prompt templates and their slash commands. Templates \
                 are loaded from .slab/templates/ and ~/.config/slab/templates/.",
            ),
            "rules" => (
                "/rules",
                "Show loaded rules",
                "Displays all rules loaded from .slab/rules/. Rules provide persistent \
                 guidelines that are injected into every conversation context.",
            ),
            _ => {
                // Check if it's a template command
                if let Some(template) = self.templates.get(cmd) {
                    println!("{}", style(format!("/{}", cmd)).green().bold());
                    println!("  {}", template.description);
                    println!();
                    if !template.variables.is_empty() {
                        println!("{}", style("Variables:").cyan());
                        for var in &template.variables {
                            let default = var.default.as_deref().unwrap_or("(required)");
                            println!("  {} - default: {}", style(&var.name).yellow(), style(default).dim());
                        }
                        println!();
                    }
                    println!("{}", style("Usage:").cyan());
                    println!("  /{} [content]", cmd);
                    println!("  /{} var=value [content]", cmd);
                    return;
                }

                println!(
                    "{} Unknown command: /{}",
                    style("Error:").red(),
                    cmd
                );
                println!(
                    "{}",
                    style("Type /help for available commands.").dim()
                );
                return;
            }
        };

        println!("{}", style(help.0).green().bold());
        println!("  {}", help.1);
        println!();
        println!("{}", style("Description:").cyan());
        for line in help.2.lines() {
            println!("  {}", line);
        }
        println!();
    }

    async fn send_message(&mut self, content: &str) -> Result<()> {
        // Add user message to context
        self.context.add_message(Message::user(content));

        // Build messages from context
        let messages = self.context.build_messages();

        let model_config = self.config.get_model_config(&self.model);

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            stream: Some(self.streaming),
            options: Some(ModelOptions {
                temperature: Some(model_config.temperature),
                top_p: Some(model_config.top_p),
                num_ctx: Some(self.config.context_limit),
            }),
        };

        let response = if self.streaming {
            self.stream_response(request).await?
        } else {
            self.wait_response(request).await?
        };

        // Add assistant message to context
        if !response.is_empty() {
            self.context
                .add_message(Message::assistant(response.clone()));
        }

        // Process file operations if enabled
        if self.file_ops_enabled && !response.is_empty() {
            self.process_file_operations(&response)?;
        }

        Ok(())
    }

    async fn stream_response(&mut self, request: ChatRequest) -> Result<String> {
        let mut rx = self.client.chat_stream(request).await?;

        let mut full_response = String::new();
        print!("{} ", style("┃").blue());
        io::stdout().flush().ok();

        while let Some(result) = rx.recv().await {
            match result {
                Ok(chunk) => {
                    full_response.push_str(&chunk);
                    print!("{}", chunk);
                    io::stdout().flush().ok();
                }
                Err(e) => {
                    println!("\n{} {}", style("Error:").red(), e);
                    return Ok(String::new());
                }
            }
        }

        println!();

        // Re-print with syntax highlighting if there are code blocks
        if full_response.contains("```") {
            println!();
            println!("{}", style("─".repeat(50)).dim());
            let highlighted = self.highlighter.format_response(&full_response);
            for line in highlighted.lines() {
                println!("{} {}", style("┃").blue(), line);
            }
            println!("{}", style("─".repeat(50)).dim());
        }

        println!();

        Ok(full_response)
    }

    async fn wait_response(&mut self, request: ChatRequest) -> Result<String> {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner} {msg}")
                .unwrap(),
        );
        spinner.set_message("Thinking...");
        spinner.enable_steady_tick(Duration::from_millis(80));

        let response = self.client.chat(request).await?;

        spinner.finish_and_clear();

        // Format with syntax highlighting if there are code blocks
        if response.contains("```") {
            let highlighted = self.highlighter.format_response(&response);
            for line in highlighted.lines() {
                println!("{} {}", style("┃").blue(), line);
            }
        } else {
            println!("{} {}", style("┃").blue(), response);
        }
        println!();

        Ok(response)
    }

    fn update_rules_for_context(&mut self) {
        if self.rules.rule_count() > 0 {
            let files: Vec<PathBuf> = self
                .context
                .list_files()
                .iter()
                .map(|p| (*p).clone())
                .collect();
            if let Some(rules_prompt) = self.rules.build_rules_prompt(&files) {
                self.context.set_rules(rules_prompt);
            }
        }
    }

    /// Load a session into the REPL
    pub fn load_session(&mut self, session: Session) {
        // Update model if different
        if !session.model.is_empty() {
            self.model = session.model.clone();
        }

        // Load messages into context
        for msg in &session.messages {
            self.context.add_message(msg.clone());
        }

        // Add to command history
        for msg in &session.messages {
            if msg.role == "user" {
                self.history.push(msg.content.clone());
            }
        }

        println!(
            "{} Loaded session '{}' with {} messages",
            style("✓").green(),
            style(&session.name).cyan(),
            session.messages.len()
        );
    }

    /// Save the current session
    pub fn save_session(&self, name: &str) -> std::result::Result<(), String> {
        let messages = self.context.messages().to_vec();
        let mut session = Session::new(name, &self.model);
        session.messages = messages;
        session.save()
    }

    fn process_file_operations(&self, response: &str) -> Result<()> {
        let mut operations = parse_file_operations(response, &self.project_root);

        if operations.is_empty() {
            return Ok(());
        }

        // Filter to only safe operations
        let safe_indices: Vec<usize> = operations
            .iter()
            .enumerate()
            .filter(|(_, op)| op.safety_check(&self.project_root).is_ok())
            .map(|(i, _)| i)
            .collect();

        if safe_indices.is_empty() {
            println!(
                "{} All file operations failed safety checks",
                style("⚠").yellow()
            );
            return Ok(());
        }

        let approved = if self.config.ui.auto_apply_file_ops {
            // Auto-apply mode: show what we're doing and apply all safe operations
            println!();
            println!(
                "{} Auto-applying {} file operation(s):",
                style("→").cyan(),
                safe_indices.len()
            );
            for &i in &safe_indices {
                let op = &operations[i];
                println!("  {} {}", style("•").dim(), op.path().display());
            }
            safe_indices
        } else {
            // Interactive mode: ask for confirmation
            let ui = FileOperationUI::new();
            ui.confirm_operations(&mut operations, &self.project_root)?
        };

        if !approved.is_empty() {
            let (success, failed) = execute_operations(&operations, &approved, &self.project_root)?;
            println!();
            if failed == 0 {
                println!(
                    "{} {} operation(s) applied successfully",
                    style("✓").green(),
                    success
                );
            } else {
                println!(
                    "{} {} succeeded, {} failed",
                    style("⚠").yellow(),
                    success,
                    failed
                );
            }
        } else if !operations.is_empty() {
            println!("{}", style("No operations applied.").dim());
        }

        println!();
        Ok(())
    }
}

/// Get directories to search for templates
fn get_template_directories(project_root: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Project-local templates (highest priority)
    dirs.push(project_root.join(".slab/templates"));

    // Global templates
    if let Some(config_dir) = dirs_next::config_dir() {
        dirs.push(config_dir.join("slab/templates"));
    }

    dirs
}

/// Run a single prompt (non-interactive)
pub async fn run_single_prompt(
    client: &OllamaClient,
    config: &Config,
    model: &str,
    prompt: &str,
    streaming: bool,
) -> Result<()> {
    let model_config = config.get_model_config(model);

    let mut messages = Vec::new();
    if let Some(system_prompt) = &model_config.system_prompt {
        messages.push(Message::system(system_prompt.clone()));
    }
    messages.push(Message::user(prompt));

    let request = ChatRequest {
        model: model.to_string(),
        messages,
        stream: Some(streaming),
        options: Some(ModelOptions {
            temperature: Some(model_config.temperature),
            top_p: Some(model_config.top_p),
            num_ctx: Some(config.context_limit),
        }),
    };

    let response = if streaming {
        let mut rx = client.chat_stream(request).await?;
        let mut full_response = String::new();
        while let Some(result) = rx.recv().await {
            match result {
                Ok(chunk) => {
                    full_response.push_str(&chunk);
                    print!("{}", chunk);
                    io::stdout().flush().ok();
                }
                Err(e) => {
                    eprintln!("\n{} {}", style("Error:").red(), e);
                    return Ok(());
                }
            }
        }
        println!();
        full_response
    } else {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner} {msg}")
                .unwrap(),
        );
        spinner.set_message("Thinking...");
        spinner.enable_steady_tick(Duration::from_millis(80));

        let response = client.chat(request).await?;
        spinner.finish_and_clear();

        println!("{}", response);
        response
    };

    // Process file operations for single prompt mode too
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut operations = parse_file_operations(&response, &project_root);

    if !operations.is_empty() {
        let ui = FileOperationUI::new();
        let approved = ui.confirm_operations(&mut operations, &project_root)?;

        if !approved.is_empty() {
            let (success, failed) = execute_operations(&operations, &approved, &project_root)?;
            println!();
            if failed == 0 {
                println!(
                    "{} {} operation(s) applied successfully",
                    style("✓").green(),
                    success
                );
            } else {
                println!(
                    "{} {} succeeded, {} failed",
                    style("⚠").yellow(),
                    success,
                    failed
                );
            }
        }
    }

    Ok(())
}
