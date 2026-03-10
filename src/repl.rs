use console::{style, Term};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyModifiers,
};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::completion::{CompletionContext, CompletionEngine, CompletionKind};
use crate::config::{find_project_root, Config};
use crate::context::ContextManager;
use crate::error::Result;
use crate::file_ops::{
    execute_operations, parse_exec_operations, parse_file_operations, FileOperationUI,
};
use crate::highlight::Highlighter;
use crate::ollama::{ChatRequest, LlmBackend, Message, ModelOptions, OllamaClient};
use crate::rules::RuleEngine;
use crate::session::Session;
use crate::templates::TemplateManager;
use crate::theme::{BoxStyle, Theme, ThemeName};
use crate::ui::{terminal_width, BoxRenderer};

pub struct Repl<B: LlmBackend = OllamaClient> {
    client: B,
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
    theme: Theme,
    box_style: BoxStyle,
}

impl<B: LlmBackend> Repl<B> {
    pub fn new(client: B, config: Config, model: String, streaming: bool) -> Self {
        // Find project root by walking up to find .slab/, fall back to cwd
        let project_root = find_project_root()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

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

        // Load theme from config
        let theme = ThemeName::from_str(&config.ui.theme).to_theme();
        let box_style = BoxStyle::from_str(&config.ui.box_style);

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
            theme,
            box_style,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let term = Term::stdout();
        term.clear_screen().ok();

        // Cache available models for completion
        if let Ok(models) = self.client.llm_list_models().await {
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
        // Show ASCII banner if enabled
        if self.config.ui.show_banner {
            println!(
                "{}",
                self.theme.primary.apply_to(
                    r#"
 _____ _       ___ _       _
|_   _| |_  __|  _| |__ _ | |__
  | | | ' \/ -_)__ \ / _` || '_ \
  |_| |_||_\___|___/_\__,_||_.__/
"#
                )
            );
        }

        // Show status bar if enabled
        if self.config.ui.show_status_bar {
            self.print_status_bar();
        } else {
            // Simple welcome message
            println!(
                "{} {} {}",
                self.theme.primary.apply_to("The Slab"),
                self.theme.muted.apply_to("·"),
                self.theme.warning.apply_to(&self.model)
            );
        }

        println!(
            "{}",
            self.theme
                .muted
                .apply_to("Type /help for commands, /exit to quit")
        );
        println!();
    }

    fn print_status_bar(&self) {
        let width = terminal_width().min(80);
        let chars = self.box_style.chars();
        let summary = self.context.summary();

        // Build status content
        let model_str = format!(" Model: {} ", self.model);
        let context_str = format!(
            " Context: {}f | {}t ",
            summary.files_count, summary.tokens_used
        );

        let content_len = model_str.len() + context_str.len() + 3; // +3 for separators
        let padding = width.saturating_sub(content_len + 2);

        // Top border
        println!(
            "{}{}{}{}{}",
            self.theme.border.apply_to(chars.top_left),
            self.theme.border.apply_to(chars.horizontal),
            self.theme.primary.apply_to(" The Slab "),
            self.theme.border.apply_to(
                std::iter::repeat_n(chars.horizontal, width.saturating_sub(13)).collect::<String>()
            ),
            self.theme.border.apply_to(chars.top_right)
        );

        // Content line
        println!(
            "{} {}{}{}{}{}",
            self.theme.border.apply_to(chars.vertical),
            self.theme.warning.apply_to(&model_str),
            self.theme.border.apply_to(chars.vertical),
            self.theme.secondary.apply_to(&context_str),
            " ".repeat(padding),
            self.theme.border.apply_to(chars.vertical)
        );

        // Bottom border
        println!(
            "{}{}{}",
            self.theme.border.apply_to(chars.bottom_left),
            self.theme.border.apply_to(
                std::iter::repeat_n(chars.horizontal, width.saturating_sub(2)).collect::<String>()
            ),
            self.theme.border.apply_to(chars.bottom_right)
        );
    }

    fn format_context_bar(&self, used: usize, budget: usize) -> String {
        const BAR_WIDTH: usize = 8;
        let filled = if budget > 0 {
            (used * BAR_WIDTH / budget).min(BAR_WIDTH)
        } else {
            0
        };
        let empty = BAR_WIDTH - filled;
        let pct = if budget > 0 { used * 100 / budget } else { 0 };
        let bar: String = "█".repeat(filled) + &"░".repeat(empty);
        format!("{} {}% ({}/{}t)", bar, pct, used, budget)
    }

    fn print_prompt(&self) {
        let summary = self.context.summary();
        let bar = self.format_context_bar(summary.tokens_used, summary.token_budget);

        // [model]
        print!(
            "{}{}{} ",
            self.theme.muted.apply_to("["),
            self.theme.warning.apply_to(&self.model),
            self.theme.muted.apply_to("]")
        );

        // [████░░░░ 42% (1024/8192t)] — always shown, with optional file count prefix
        if summary.files_count > 0 {
            print!(
                "{}{}{}{}{}",
                self.theme.muted.apply_to("["),
                self.theme
                    .secondary
                    .apply_to(format!("{}f | ", summary.files_count)),
                self.theme.muted.apply_to(&bar),
                self.theme.muted.apply_to("]"),
                self.theme.muted.apply_to(" "),
            );
        } else {
            print!(
                "{}{}{} ",
                self.theme.muted.apply_to("["),
                self.theme.muted.apply_to(&bar),
                self.theme.muted.apply_to("]"),
            );
        }

        print!("{} ", self.theme.primary.apply_to("❯"));
        io::stdout().flush().ok();
    }

    fn read_input(&mut self) -> Result<Option<String>> {
        self.print_prompt();

        let mut input = String::new();
        let mut cursor_pos: usize = 0;
        let mut stdout = io::stdout();
        let mut history_index = self.history.len();
        let mut saved_input = String::new();
        let mut current_preview: Option<String> = None;

        // Enable raw mode for better input handling
        crossterm::terminal::enable_raw_mode().ok();
        crossterm::execute!(stdout, EnableBracketedPaste).ok();

        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Paste(pasted_text)) => {
                        // Clear any pending preview
                        if let Some(ref preview) = current_preview {
                            self.clear_preview(preview.len());
                        }
                        current_preview = None;

                        // Normalize line endings (\r\n and \r → \n)
                        let normalized = pasted_text.replace("\r\n", "\n").replace('\r', "\n");

                        // Capture screen cursor position (in visual chars) before mutating buffer
                        let screen_cursor_pos = cursor_pos;

                        // Insert full pasted string at cursor position (single O(n) operation)
                        input.insert_str(cursor_pos, &normalized);
                        cursor_pos += normalized.len();

                        // Redraw entire input line once
                        self.redraw_full_input(&input, cursor_pos, screen_cursor_pos, &mut stdout);
                    }
                    Ok(Event::Key(key_event)) => {
                        // Clear any existing preview before processing
                        if let Some(ref preview) = current_preview {
                            self.clear_preview(preview.len());
                        }
                        current_preview = None;

                        match (key_event.code, key_event.modifiers) {
                            // Ctrl+C - cancel current input
                            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                crossterm::execute!(stdout, DisableBracketedPaste).ok();
                                crossterm::terminal::disable_raw_mode().ok();
                                println!();
                                return Ok(Some(String::new()));
                            }
                            // Ctrl+D - exit
                            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                                crossterm::execute!(stdout, DisableBracketedPaste).ok();
                                crossterm::terminal::disable_raw_mode().ok();
                                return Ok(None);
                            }
                            // Ctrl+L - clear screen
                            (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                                crossterm::execute!(stdout, DisableBracketedPaste).ok();
                                crossterm::terminal::disable_raw_mode().ok();
                                Term::stdout().clear_screen().ok();
                                self.print_welcome();
                                return Ok(Some(String::new()));
                            }
                            // Enter - submit
                            (KeyCode::Enter, _) => {
                                crossterm::execute!(stdout, DisableBracketedPaste).ok();
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
                                        cursor_pos = input.len();
                                        print!("{}", first_char);
                                        stdout.flush().ok();
                                    }
                                } else if cursor_pos < input.len() {
                                    cursor_pos += 1;
                                    print!("\x1b[C");
                                    stdout.flush().ok();
                                }
                            }
                            // Left arrow - move cursor left
                            (KeyCode::Left, _) => {
                                if cursor_pos > 0 {
                                    cursor_pos -= 1;
                                    print!("\x1b[D");
                                    stdout.flush().ok();
                                }
                            }
                            // Home - move cursor to start
                            (KeyCode::Home, _) => {
                                if cursor_pos > 0 {
                                    print!("\x1b[{}D", cursor_pos);
                                    cursor_pos = 0;
                                    stdout.flush().ok();
                                }
                            }
                            // End - move cursor to end
                            (KeyCode::End, _) => {
                                if cursor_pos < input.len() {
                                    print!("\x1b[{}C", input.len() - cursor_pos);
                                    cursor_pos = input.len();
                                    stdout.flush().ok();
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
                                    self.clear_input(&input, cursor_pos);
                                    input = self.history[history_index].clone();
                                    cursor_pos = input.len();
                                    print!("{}", input);
                                    stdout.flush().ok();
                                }
                            }
                            // Down arrow - history forward
                            (KeyCode::Down, _) => {
                                if history_index < self.history.len() {
                                    // Clear current line
                                    self.clear_input(&input, cursor_pos);
                                    history_index += 1;
                                    if history_index == self.history.len() {
                                        input = saved_input.clone();
                                    } else {
                                        input = self.history[history_index].clone();
                                    }
                                    cursor_pos = input.len();
                                    print!("{}", input);
                                    stdout.flush().ok();
                                }
                            }
                            // Tab - command completion
                            (KeyCode::Tab, _) => {
                                let completions = self.get_completions(&input);
                                if completions.len() == 1 {
                                    // Single match - auto-complete
                                    self.clear_input(&input, cursor_pos);
                                    input = completions[0].0.clone();
                                    cursor_pos = input.len();
                                    print!("{}", input);
                                    stdout.flush().ok();
                                } else if !completions.is_empty() {
                                    // Multiple matches - show completion menu
                                    // Disable raw mode for proper menu display
                                    crossterm::terminal::disable_raw_mode().ok();
                                    println!();
                                    self.show_completion_menu(&completions);
                                    self.print_prompt();
                                    print!("{}", input);
                                    stdout.flush().ok();
                                    // Re-enable raw mode
                                    crossterm::terminal::enable_raw_mode().ok();
                                }
                            }
                            // Backspace
                            (KeyCode::Backspace, _) => {
                                if cursor_pos > 0 {
                                    input.remove(cursor_pos - 1);
                                    cursor_pos -= 1;
                                    // Move cursor back, reprint rest of line, clear trailing char
                                    print!("\x08");
                                    let tail = &input[cursor_pos..];
                                    print!("{} ", tail);
                                    // Move cursor back to position
                                    let move_back = tail.len() + 1;
                                    for _ in 0..move_back {
                                        print!("\x08");
                                    }
                                    stdout.flush().ok();
                                }
                            }
                            // Delete key
                            (KeyCode::Delete, _) => {
                                if cursor_pos < input.len() {
                                    input.remove(cursor_pos);
                                    // Reprint rest of line, clear trailing char
                                    let tail = &input[cursor_pos..];
                                    print!("{} ", tail);
                                    let move_back = tail.len() + 1;
                                    for _ in 0..move_back {
                                        print!("\x08");
                                    }
                                    stdout.flush().ok();
                                }
                            }
                            // Regular character
                            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                                input.insert(cursor_pos, c);
                                cursor_pos += 1;
                                // Print from cursor position onward
                                let tail = &input[cursor_pos - 1..];
                                print!("{}", tail);
                                // Move cursor back to correct position
                                let move_back = tail.len() - 1;
                                for _ in 0..move_back {
                                    print!("\x08");
                                }
                                stdout.flush().ok();
                            }
                            _ => {}
                        }

                        // Show inline preview after input changes (only when cursor is at end)
                        if cursor_pos == input.len()
                            && (input.starts_with('/') || input.contains('@'))
                            && self.config.ui.inline_completion_preview
                        {
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
                    } // end Ok(Event::Key(...))
                    _ => {}
                } // end match event::read()
            }
        }
    }

    /// Clear the input text from the terminal
    fn clear_input(&self, input: &str, cursor_pos: usize) {
        // Move cursor to start of input
        if cursor_pos > 0 {
            print!("\x1b[{}D", cursor_pos);
        }
        // Overwrite entire input with spaces
        for _ in 0..input.len() {
            print!(" ");
        }
        // Move back to start
        if !input.is_empty() {
            print!("\x1b[{}D", input.len());
        }
    }

    /// Clear the preview text (spaces over it)
    fn clear_preview(&self, len: usize) {
        use std::io::Write;
        // Overwrite preview with spaces, then move cursor back
        for _ in 0..len {
            print!(" ");
        }
        for _ in 0..len {
            print!("\x08");
        }
        std::io::stdout().flush().ok();
    }

    /// Redraw entire input line after bulk insert (paste).
    /// Replaces per-character rendering for O(n) single-pass redraw.
    ///
    /// `screen_cursor_pos` is the byte offset in `input` where the terminal
    /// cursor currently sits (i.e. the position *before* the paste was inserted).
    /// This differs from `cursor_pos` (the desired position after redraw) and
    /// must be used for the initial backward movement so we don't overshoot into
    /// the prompt / context-bar area.
    fn redraw_full_input(
        &self,
        input: &str,
        cursor_pos: usize,
        screen_cursor_pos: usize,
        stdout: &mut impl io::Write,
    ) {
        // Replace internal newlines with visible glyph for single-line display
        let visual: String = input.replace('\n', "↵");

        // Visual column count of text before the *desired* cursor position
        let before_visual_len = input[..cursor_pos].replace('\n', "↵").chars().count();

        // Visual column count of text before the *current screen* cursor —
        // this is how far back we need to move to reach the start of input.
        let screen_visual_len = input[..screen_cursor_pos]
            .replace('\n', "↵")
            .chars()
            .count();

        // Move cursor back to start of input (based on where it actually is on
        // screen, not where we want it to end up after the redraw).
        if screen_visual_len > 0 {
            print!("\x1b[{}D", screen_visual_len);
        }

        // Print full visual line
        print!("{}", visual);

        // Move cursor back to cursor_pos
        let tail_visual_len = visual.chars().count() - before_visual_len;
        if tail_visual_len > 0 {
            print!("\x1b[{}D", tail_visual_len);
        }

        stdout.flush().ok();
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
            cwd: self.context.initial_cwd(),
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
                    println!(
                        "{} /add <file|directory> [file2 ...]",
                        style("Usage:").dim()
                    );
                    return Ok(true);
                }
                let mut any_change = false;
                for path in &parts[1..] {
                    if self.context.is_directory(path) {
                        match self.context.add_directory(path) {
                            Ok((added, skipped)) => {
                                any_change = true;
                                println!(
                                    "{} Added {} file(s) from {} to context",
                                    style("✓").green(),
                                    style(added).cyan(),
                                    style(path).cyan()
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
                            }
                            Err(e) => {
                                println!("{} {}", style("Error:").red(), e);
                            }
                        }
                    } else {
                        match self.context.add_file(path) {
                            Ok(()) => {
                                any_change = true;
                                println!(
                                    "{} Added {} to context",
                                    style("✓").green(),
                                    style(path).cyan()
                                );
                            }
                            Err(e) => {
                                println!("{} {}", style("Error:").red(), e);
                            }
                        }
                    }
                }
                if any_change {
                    self.update_rules_for_context();
                }
                Ok(true)
            }
            "remove" | "rm" => {
                if parts.len() < 2 {
                    println!("{} /remove <file> [file2 ...]", style("Usage:").dim());
                    return Ok(true);
                }
                let mut any_change = false;
                for path in &parts[1..] {
                    if self.context.remove_file(path) {
                        any_change = true;
                        println!(
                            "{} Removed {} from context",
                            style("✓").green(),
                            style(path).cyan()
                        );
                    } else {
                        println!(
                            "{} File not in context: {}",
                            style("✗").red(),
                            style(path).cyan()
                        );
                    }
                }
                if any_change {
                    self.update_rules_for_context();
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
            "watch" => {
                let enabled = !self.context.watch_mode();
                self.context.set_watch_mode(enabled);
                if enabled {
                    println!(
                        "{}",
                        style("Watch mode ON — context files will be refreshed from disk before each LLM call.").green()
                    );
                } else {
                    println!("{}", style("Watch mode OFF.").dim());
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
                        let status = if rule.enabled {
                            style("✓").green()
                        } else {
                            style("✗").dim()
                        };
                        let name_styled = if rule.enabled {
                            style(&rule.name).green()
                        } else {
                            style(&rule.name).dim()
                        };
                        print!("  {} {}", status, name_styled);
                        if !rule.enabled {
                            print!(" {}", style("(disabled)").dim());
                        }
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
            "exec" => {
                let cmd_line = command
                    .trim()
                    .strip_prefix("/exec")
                    .unwrap_or("")
                    .trim_start();
                if cmd_line.is_empty() {
                    println!("{} /exec <shell command>", style("Usage:").dim());
                    println!(
                        "{}",
                        style("Example: /exec podman exec container echo hello world").dim()
                    );
                    return Ok(true);
                }
                #[cfg(unix)]
                let output = Command::new("sh").arg("-c").arg(cmd_line).output();
                #[cfg(windows)]
                let output = Command::new("cmd").args(["/C", cmd_line]).output();
                match output {
                    Ok(o) => {
                        if !o.stdout.is_empty() {
                            print!("{}", String::from_utf8_lossy(&o.stdout));
                        }
                        if !o.stderr.is_empty() {
                            eprint!("{}", String::from_utf8_lossy(&o.stderr));
                        }
                        io::stdout().flush().ok();
                        if !o.status.success() {
                            if let Some(code) = o.status.code() {
                                println!("{} {}", style("Exit code:").dim(), code);
                            }
                        }
                        // Add command and output to context so the model has it
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        let code = o
                            .status
                            .code()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "—".into());
                        let mut ctx_msg = format!("[Ran shell command]\n$ {cmd_line}\n\n");
                        if !stdout.is_empty() {
                            ctx_msg.push_str("stdout:\n");
                            ctx_msg.push_str(&stdout);
                            if !stdout.ends_with('\n') {
                                ctx_msg.push('\n');
                            }
                        }
                        if !stderr.is_empty() {
                            ctx_msg.push_str("stderr:\n");
                            ctx_msg.push_str(&stderr);
                            if !stderr.ends_with('\n') {
                                ctx_msg.push('\n');
                            }
                        }
                        ctx_msg.push_str(&format!("exit code: {code}\n"));
                        self.context.add_message(Message::user(&ctx_msg));
                    }
                    Err(e) => {
                        println!("{} {}", style("Exec failed:").red(), e);
                        let ctx_msg =
                            format!("[Shell command failed]\n$ {cmd_line}\n\nerror: {e}\n");
                        self.context.add_message(Message::user(&ctx_msg));
                    }
                }
                Ok(true)
            }
            "rule" => {
                if parts.len() < 3 {
                    println!("{} /rule enable|disable <name>", style("Usage:").dim());
                    return Ok(true);
                }
                let action = parts[1];
                let name = parts[2..].join(" ");
                match action {
                    "enable" => {
                        if self.rules.enable_rule(&name) {
                            println!(
                                "{} Enabled rule: {}",
                                style("✓").green(),
                                style(&name).cyan()
                            );
                            self.update_rules_for_context();
                        } else {
                            println!(
                                "{} Rule not found: {}",
                                style("✗").red(),
                                style(&name).cyan()
                            );
                        }
                    }
                    "disable" => {
                        if self.rules.disable_rule(&name) {
                            println!(
                                "{} Disabled rule: {}",
                                style("✓").green(),
                                style(&name).cyan()
                            );
                            self.update_rules_for_context();
                        } else {
                            println!(
                                "{} Rule not found: {}",
                                style("✗").red(),
                                style(&name).cyan()
                            );
                        }
                    }
                    _ => {
                        println!("{} /rule enable|disable <name>", style("Usage:").dim());
                    }
                }
                Ok(true)
            }
            "export" => {
                use chrono::Local;
                use std::fmt::Write as FmtWrite;

                let messages = self.context.messages();
                if messages.is_empty() {
                    println!(
                        "{}",
                        style("Nothing to export — conversation is empty.").dim()
                    );
                    return Ok(true);
                }

                // Determine output path
                let filename = if parts.len() > 1 {
                    parts[1..].join(" ")
                } else {
                    format!("slab-export-{}.txt", Local::now().format("%Y-%m-%d-%H%M%S"))
                };
                let output_path = std::path::Path::new(&filename);

                // Build plain-text export (no ANSI codes — safe for tmux/pipes)
                let mut out = String::new();
                let _ = writeln!(out, "=== Slab Chat Export ===");
                let _ = writeln!(
                    out,
                    "Date:     {}",
                    Local::now().format("%Y-%m-%d %H:%M:%S")
                );
                let _ = writeln!(out, "Model:    {}", self.model);
                let _ = writeln!(out, "Messages: {}", messages.len());
                let _ = writeln!(out, "{}", "=".repeat(40));

                for (i, msg) in messages.iter().enumerate() {
                    let _ = writeln!(out);
                    let role = msg.role.to_uppercase();
                    let _ = writeln!(out, "[{}] {}", i + 1, role);
                    let _ = writeln!(out, "{}", "-".repeat(40));
                    let _ = writeln!(out, "{}", msg.content);
                }

                let _ = writeln!(out);
                let _ = writeln!(out, "{}", "=".repeat(40));
                let _ = writeln!(out, "End of export");

                match std::fs::write(output_path, &out) {
                    Ok(()) => {
                        println!(
                            "{} Exported {} message(s) to {}",
                            style("✓").green(),
                            messages.len(),
                            style(output_path.display()).cyan()
                        );
                    }
                    Err(e) => {
                        println!("{} Failed to write export: {}", style("✗").red(), e);
                    }
                }
                Ok(true)
            }
            "pwd" => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| self.project_root.clone());
                println!("{}", style(cwd.display()).cyan());
                Ok(true)
            }
            _ => {
                println!("{} {}", style("Unknown command:").red(), command);
                println!("{}", style("Type /help for available commands.").dim());
                Ok(true)
            }
        }
    }

    async fn run_phase_loop(
        &mut self,
        phases: &[crate::templates::TemplatePhase],
        max_iterations: usize,
        template_follow_up: Option<&str>,
        mut confirm: impl FnMut(usize) -> bool,
    ) -> Result<()> {
        use crate::templates::{PhaseFeedback, PhaseOutcome};

        let mut pass = 1usize;
        loop {
            if pass > max_iterations {
                println!(
                    "{}",
                    style(format!(
                        "⚠ Phase loop reached maximum of {} iteration(s). Stopping.",
                        max_iterations
                    ))
                    .yellow()
                );
                return Ok(());
            }

            println!(
                "\n{} {}",
                style("→").cyan(),
                style(format!("Phase loop: pass {}", pass)).bold()
            );

            let mut any_continue = false;
            let mut feedback_parts: Vec<String> = Vec::new();
            let mut follow_up_parts: Vec<String> = Vec::new();

            for phase in phases {
                let label = phase.name.as_deref().unwrap_or("phase");

                // Bug 3: skip phases with {{file}}/{{files}} when context is empty
                let cmd_str = match interpolate_phase_cmd(&phase.run, &self.context) {
                    Some(s) => s,
                    None => {
                        println!(
                            "  {} [{}] skipped: no files in context",
                            style("⚠").yellow(),
                            label
                        );
                        continue;
                    }
                };

                println!(
                    "  {} {}",
                    style(format!("[{}]", label)).dim(),
                    style(&cmd_str).cyan()
                );

                let output = Command::new("sh").arg("-c").arg(&cmd_str).output();

                match output {
                    Err(e) => {
                        println!("  {} {}", style("Error running phase:").red(), e);
                        // Bug 2: only continue when on_failure == Continue
                        if phase.on_failure == PhaseOutcome::Continue {
                            any_continue = true;
                        }
                        let entry = format!("[{}]: error: {}\n", label, e);
                        match phase.feedback {
                            PhaseFeedback::Never => {}
                            _ => feedback_parts.push(entry),
                        }
                        if phase.on_failure == PhaseOutcome::Continue {
                            if let Some(ref fu) = phase.follow_up {
                                follow_up_parts.push(fu.clone());
                            }
                        }
                    }
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if !stdout.is_empty() {
                            print!("{}", stdout);
                        }
                        if !stderr.is_empty() {
                            eprint!("{}", stderr);
                        }

                        let combined = format!("{}{}", stdout, stderr);
                        let entry = format!(
                            "[{}] (exit {}):\n{}\n",
                            label,
                            out.status.code().unwrap_or(-1),
                            combined
                        );

                        let outcome = if out.status.success() {
                            &phase.on_success
                        } else {
                            &phase.on_failure
                        };

                        let triggers_continue = *outcome == PhaseOutcome::Continue;
                        if triggers_continue {
                            any_continue = true;
                        }

                        // Determine whether to inject into LLM context
                        let should_inject = match phase.feedback {
                            PhaseFeedback::Always => true,
                            PhaseFeedback::OnFailure => !out.status.success(),
                            PhaseFeedback::Never => false,
                        };
                        if should_inject {
                            feedback_parts.push(entry);
                        }

                        if triggers_continue {
                            if let Some(ref fu) = phase.follow_up {
                                follow_up_parts.push(fu.clone());
                            }
                        }
                    }
                }
            }

            if !any_continue {
                println!(
                    "\n{} {}",
                    style("✓").green(),
                    style("All checks passed.").bold()
                );
                return Ok(());
            }

            // Ask user whether to continue
            print!(
                "\n{} Issues found on pass {}. Run another improvement pass? [y/N]: ",
                style("→").cyan(),
                pass
            );
            io::stdout().flush().ok();

            if !confirm(pass) {
                println!(
                    "{}",
                    style(format!("Stopped after {} pass(es).", pass)).dim()
                );
                return Ok(());
            }

            // Resolve follow-up message (precedence: per-phase → template-level → default)
            let follow_up_text = if !follow_up_parts.is_empty() {
                follow_up_parts.join("\n\n")
            } else if let Some(tfu) = template_follow_up {
                tfu.to_string()
            } else {
                "The checks above found issues. Please fix them and output the complete corrected file.".to_string()
            };

            let feedback_body = feedback_parts.join("\n");

            // Bug 1: single send_message combines phase results + follow-up
            let combined = format!(
                "[Phase results - pass {}]\n{}\n\n{}",
                pass, feedback_body, follow_up_text
            );
            self.send_message(&combined).await?;

            pass += 1;
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

        // Parse --output/-o flag before processing other args
        let mut output_file: Option<String> = None;
        let mut filtered_args: Vec<&str> = Vec::new();
        let mut iter = args.iter().peekable();
        while let Some(arg) = iter.next() {
            if *arg == "--output" || *arg == "-o" {
                if let Some(next) = iter.next() {
                    output_file = Some(next.to_string());
                }
            } else if let Some(val) = arg
                .strip_prefix("--output=")
                .or_else(|| arg.strip_prefix("-o="))
            {
                output_file = Some(val.to_string());
            } else {
                filtered_args.push(arg);
            }
        }
        let args = filtered_args.as_slice();

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

        // Replace the verbose template prompt in history with a short summary
        // so follow-up messages aren't dominated by the template instructions
        let summary = format!("[Used /{} template]", cmd);
        if let Some(msg) = self.context.last_user_message_mut() {
            *msg = summary;
        }

        // Run phase loop if template defines phases
        if !template.phases.is_empty() {
            let max_iterations = template.max_phases.unwrap_or(10);
            self.run_phase_loop(
                &template.phases,
                max_iterations,
                template.phases_follow_up.as_deref(),
                |_pass| {
                    let mut line = String::new();
                    io::stdin().read_line(&mut line).ok();
                    let answer = line.trim().to_lowercase();
                    answer == "y" || answer == "yes"
                },
            )
            .await?;
        }

        // Determine target file: from flag or interactive prompt
        let target_file = if let Some(f) = output_file {
            Some(f)
        } else {
            print!(
                "\n{} Save response to file? (Enter filename or press Enter to skip): ",
                style("→").cyan()
            );
            use std::io::Write as _;
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let trimmed = input.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        };

        if let Some(path) = target_file {
            if let Some(content) = self.context.last_assistant_message() {
                std::fs::write(&path, content)?;
                println!("{} Saved to {}", style("✓").green(), style(&path).yellow());
            }
        }

        Ok(true)
    }

    fn print_help(&self) {
        let width = terminal_width().min(70);
        let renderer = BoxRenderer::new(self.box_style, self.theme.clone()).with_width(width);

        // Built-in commands section
        let commands = [
            ("/help", "Show this help"),
            ("/exit", "Exit the REPL"),
            ("/clear", "Clear conversation"),
            ("/model [name]", "Show/set current model"),
            ("/context", "Show context summary"),
            ("/tokens", "Show token usage"),
            ("/files", "List files in context"),
            ("/add <path> [...]", "Add file/directory to context"),
            ("/remove <file> [...]", "Remove file from context"),
            ("/pwd", "Print working directory"),
            ("/fileops [on|off]", "Toggle file operations"),
            ("/templates", "List available templates"),
            ("/rules", "Show loaded rules"),
            ("/rule enable|disable", "Enable/disable a rule"),
            ("/exec <command>", "Run a shell command"),
            ("/export [file]", "Export chat to a text file"),
        ];

        let mut content = String::new();
        for (cmd, desc) in commands {
            content.push_str(&format!(
                "  {} - {}\n",
                self.theme.success.apply_to(format!("{:<16}", cmd)),
                self.theme.muted.apply_to(desc)
            ));
        }
        // Remove trailing newline
        content.pop();

        print!("{}", renderer.render_titled_box(Some("Commands"), &content));

        // Show template commands
        let templates = self.templates.list();
        if !templates.is_empty() {
            println!();
            let mut template_content = String::new();
            for t in templates {
                template_content.push_str(&format!(
                    "  {} - {}\n",
                    self.theme.success.apply_to(format!("{:<16}", t.command)),
                    self.theme.muted.apply_to(&t.description)
                ));
            }
            template_content.pop();
            print!(
                "{}",
                renderer.render_titled_box(Some("Templates"), &template_content)
            );
        }

        // Keyboard shortcuts
        println!();
        let shortcuts = [
            ("Ctrl+C", "Cancel current input"),
            ("Ctrl+D", "Exit"),
            ("Ctrl+L", "Clear screen"),
            ("Up/Down", "Navigate command history"),
            ("Tab", "Autocomplete commands"),
            ("Right", "Accept inline preview"),
        ];

        let mut shortcut_content = String::new();
        for (key, desc) in shortcuts {
            shortcut_content.push_str(&format!(
                "  {} - {}\n",
                self.theme.warning.apply_to(format!("{:<10}", key)),
                self.theme.muted.apply_to(desc)
            ));
        }
        shortcut_content.pop();

        print!(
            "{}",
            renderer.render_titled_box(Some("Shortcuts"), &shortcut_content)
        );

        println!();
        println!(
            "{}",
            self.theme
                .muted
                .apply_to("Type /help <command> for detailed command help.")
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
                "/add <file|directory> [file2 ...]",
                "Add a file or directory to context",
                "Adds file contents to the conversation context. If a directory is specified, \
                 all files are added recursively. Multiple paths can be specified.\n\n\
                 Directories skip: hidden files, node_modules, target, .git, binary files\n\n\
                 Examples:\n  /add src/main.rs\n  /add src/main.rs src/lib.rs\n  /add src/\n  /add ../other/",
            ),
            "remove" | "rm" => (
                "/remove <file> [file2 ...], /rm <file> [file2 ...]",
                "Remove a file from context",
                "Removes one or more previously added files from the context.\n\n\
                 Examples:\n  /remove src/main.rs\n  /remove src/main.rs src/lib.rs",
            ),
            "pwd" => (
                "/pwd",
                "Print working directory",
                "Prints the current working directory.",
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
            "rule" => (
                "/rule enable|disable <name>",
                "Enable or disable a rule",
                "Enables or disables a loaded rule by name. Rule names are shown by /rules.",
            ),
            "exec" => (
                "/exec <command>",
                "Run a shell command",
                "Runs the given command in the system shell (sh -c on Unix, cmd /C on Windows). \
                 Use for one-off commands like podman/docker exec, running scripts, etc.\n\n\
                 Example:\n  /exec podman exec container echo hello world",
            ),
            "export" => (
                "/export [filename]",
                "Export chat to a plain-text file",
                "Writes the full conversation to a plain-text file with no ANSI escape codes, \
                 making it safe to open in any editor or share via email/ticket. \
                 Works correctly through tmux and other terminal multiplexers.\n\n\
                 If no filename is given, a timestamped file is created in the current directory.\n\n\
                 Examples:\n  /export                     - Save to slab-export-YYYY-MM-DD-HHMMSS.txt\n  /export chat.txt            - Save to chat.txt\n  /export /tmp/debug-chat.txt - Save to an absolute path",
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
        // If watch mode is on, refresh all context files from disk first
        if self.context.watch_mode() {
            let refreshed = self.context.refresh_files();
            if !refreshed.is_empty() {
                println!(
                    "{}",
                    style(format!(
                        "↺  Refreshed {} file(s) from disk.",
                        refreshed.len()
                    ))
                    .dim()
                );
            }
        }

        // Expand @file references before sending to the LLM
        let expanded = self.context.expand_file_references(content);
        // Add the expanded message to context
        self.context.add_message(Message::user(&expanded));

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

        // Process exec blocks (run commands, add output to context)
        if !response.is_empty() {
            self.process_exec_operations(&response)?;
        }

        Ok(())
    }

    async fn stream_response(&mut self, request: ChatRequest) -> Result<String> {
        let mut rx = self.client.llm_stream(request).await?;

        let mut full_response = String::new();
        print!("{} ", style("┃").blue());
        io::stdout().flush().ok();

        // Enable raw mode and spawn a task to listen for Ctrl+C / Ctrl+D
        crossterm::terminal::enable_raw_mode().ok();
        let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::task::spawn_blocking(move || loop {
            if cancel_tx.is_closed() {
                return;
            }
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key_event)) = event::read() {
                    match (key_event.code, key_event.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL)
                        | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            let _ = cancel_tx.blocking_send(());
                            return;
                        }
                        _ => {}
                    }
                }
            }
        });

        let mut interrupted = false;

        loop {
            tokio::select! {
                chunk = rx.recv() => {
                    match chunk {
                        Some(Ok(text)) => {
                            full_response.push_str(&text);
                            // In raw mode, \n doesn't reset to column 0; use \r\n instead
                            print!("{}", text.replace('\n', "\r\n"));
                            io::stdout().flush().ok();
                        }
                        Some(Err(e)) => {
                            crossterm::terminal::disable_raw_mode().ok();
                            println!("\n{} {}", style("Error:").red(), e);
                            return Ok(String::new());
                        }
                        None => break,
                    }
                }
                _ = cancel_rx.recv() => {
                    interrupted = true;
                    break;
                }
            }
        }

        crossterm::terminal::disable_raw_mode().ok();

        if interrupted {
            // Drop the receiver so the spawned stream task stops
            drop(rx);
            println!("\n{}", style("(interrupted)").dim());
            println!();
            return Ok(String::new());
        }

        println!();

        // Re-print with syntax highlighting if there are code blocks
        if full_response.contains("```") {
            println!();
            let highlighted = self.highlighter.format_response(&full_response);
            for line in highlighted.lines() {
                println!("{}", line);
            }
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

        // Enable raw mode and spawn a task to listen for Ctrl+C / Ctrl+D
        crossterm::terminal::enable_raw_mode().ok();
        let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::task::spawn_blocking(move || loop {
            if cancel_tx.is_closed() {
                return;
            }
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key_event)) = event::read() {
                    match (key_event.code, key_event.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL)
                        | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            let _ = cancel_tx.blocking_send(());
                            return;
                        }
                        _ => {}
                    }
                }
            }
        });

        let response = tokio::select! {
            resp = self.client.llm_chat(request) => resp?,
            _ = cancel_rx.recv() => {
                crossterm::terminal::disable_raw_mode().ok();
                spinner.finish_and_clear();
                println!("{}", style("(interrupted)").dim());
                println!();
                return Ok(String::new());
            }
        };

        crossterm::terminal::disable_raw_mode().ok();
        spinner.finish_and_clear();

        // Format with syntax highlighting if there are code blocks
        if response.contains("```") {
            let highlighted = self.highlighter.format_response(&response);
            for line in highlighted.lines() {
                println!("{}", line);
            }
        } else {
            println!("{}", response);
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
            } else {
                self.context.clear_rules();
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

    /// Add files/directories to context from CLI --file flags
    pub fn add_files(&mut self, files: &[PathBuf]) {
        for path in files {
            let path_str = path.to_string_lossy();
            if path.is_dir() {
                match self.context.add_directory(&*path_str) {
                    Ok((added, skipped)) => {
                        println!(
                            "{} Added {} file(s) from {}",
                            style("✓").green(),
                            style(added).cyan(),
                            style(&path_str).cyan()
                        );
                        if !skipped.is_empty() {
                            println!(
                                "{} Skipped {} file(s) (binary/unreadable)",
                                style("⚠").yellow(),
                                skipped.len()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("{} {}: {}", style("Error:").red(), path_str, e);
                    }
                }
            } else {
                match self.context.add_file(&*path_str) {
                    Ok(()) => {
                        println!(
                            "{} Added {} to context",
                            style("✓").green(),
                            style(&path_str).cyan()
                        );
                    }
                    Err(e) => {
                        eprintln!("{} {}: {}", style("Error:").red(), path_str, e);
                    }
                }
            }
        }
        if !files.is_empty() {
            self.update_rules_for_context();
        }
    }

    /// Send a template as an initial message (used with --template flag in chat mode)
    pub async fn send_template(&mut self, template_name: &str) -> Result<()> {
        let template = self.templates.get(template_name).ok_or_else(|| {
            crate::error::SlabError::TemplateError(format!("Template not found: {}", template_name))
        })?;
        let template_display_name = template.name.clone();

        // Render with defaults only (no user variables in chat mode)
        let variables = HashMap::new();
        let rendered = self
            .templates
            .render(&template_display_name, &variables, &self.context)
            .map_err(crate::error::SlabError::TemplateError)?;

        eprintln!(
            "{} {} {}",
            style("→").cyan(),
            style("Using template:").dim(),
            style(template_name).yellow()
        );

        self.send_message(&rendered).await?;
        Ok(())
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

        // Filter out edits that would truncate files (LLM output only a snippet)
        let mut truncated = Vec::new();
        let safe_indices: Vec<usize> = safe_indices
            .into_iter()
            .filter(|&i| {
                if let Some((orig, new)) = operations[i].truncation_check() {
                    truncated.push((i, orig, new));
                    false
                } else {
                    true
                }
            })
            .collect();

        if !truncated.is_empty() {
            println!();
            for &(i, orig, new) in &truncated {
                println!(
                    "{} {} — blocked edit to {} ({} → {} lines). The model likely output only a snippet instead of the complete file.",
                    style("⚠").red(),
                    style("TRUNCATION BLOCKED").red().bold(),
                    style(operations[i].path().display()).cyan(),
                    orig,
                    new,
                );
            }
        }

        if safe_indices.is_empty() && !truncated.is_empty() {
            println!(
                "{} All edits were blocked due to truncation. Ask the model to output the complete file.",
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

    fn process_exec_operations(&mut self, response: &str) -> Result<()> {
        let commands = parse_exec_operations(response);
        if commands.is_empty() {
            return Ok(());
        }

        println!();
        println!(
            "{} {} command(s) to run:",
            style("→").cyan(),
            commands.len()
        );
        println!();
        for (i, cmd) in commands.iter().enumerate() {
            let preview = cmd.lines().next().unwrap_or(cmd).trim();
            let display = if preview.len() > 60 {
                format!("{}...", &preview[..57])
            } else {
                preview.to_string()
            };
            println!(
                "  {} {}",
                style(format!("[{}]", i + 1)).dim(),
                style(display).cyan()
            );
        }
        println!();
        print!(
            "{} ",
            style("[r]un all, [s]kip all, or enter numbers to run (e.g. 1 3):").dim()
        );
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        let to_run: Vec<usize> = match input.as_str() {
            "r" | "run" | "a" | "all" | "y" | "yes" => (0..commands.len()).collect(),
            "s" | "skip" | "n" | "no" => {
                println!("{}", style("No commands run.").dim());
                println!();
                return Ok(());
            }
            _ => {
                let mut indices = Vec::new();
                for part in input.split(|c: char| c == ',' || c.is_whitespace()) {
                    if let Ok(num) = part.trim().parse::<usize>() {
                        if num > 0 && num <= commands.len() {
                            indices.push(num - 1);
                        }
                    }
                }
                indices
            }
        };

        if to_run.is_empty() {
            println!("{}", style("No commands run.").dim());
            println!();
            return Ok(());
        }

        for &idx in &to_run {
            let cmd_line = &commands[idx];
            #[cfg(unix)]
            let output = Command::new("sh").arg("-c").arg(cmd_line).output();
            #[cfg(windows)]
            let output = Command::new("cmd").args(["/C", cmd_line]).output();

            match output {
                Ok(o) => {
                    if !o.stdout.is_empty() {
                        print!("{}", String::from_utf8_lossy(&o.stdout));
                    }
                    if !o.stderr.is_empty() {
                        eprint!("{}", String::from_utf8_lossy(&o.stderr));
                    }
                    io::stdout().flush().ok();
                    if !o.status.success() {
                        if let Some(code) = o.status.code() {
                            println!("{} {}", style("Exit code:").dim(), code);
                        }
                    }
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let code = o
                        .status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "—".into());
                    let mut ctx_msg = format!("[Ran shell command]\n$ {cmd_line}\n\n");
                    if !stdout.is_empty() {
                        ctx_msg.push_str("stdout:\n");
                        ctx_msg.push_str(&stdout);
                        if !stdout.ends_with('\n') {
                            ctx_msg.push('\n');
                        }
                    }
                    if !stderr.is_empty() {
                        ctx_msg.push_str("stderr:\n");
                        ctx_msg.push_str(&stderr);
                        if !stderr.ends_with('\n') {
                            ctx_msg.push('\n');
                        }
                    }
                    ctx_msg.push_str(&format!("exit code: {code}\n"));
                    self.context.add_message(Message::user(&ctx_msg));
                }
                Err(e) => {
                    println!("{} {}", style("Exec failed:").red(), e);
                    let ctx_msg = format!("[Shell command failed]\n$ {cmd_line}\n\nerror: {e}\n");
                    self.context.add_message(Message::user(&ctx_msg));
                }
            }
        }
        println!();
        Ok(())
    }
}

/// Interpolate {{file}} and {{files}} placeholders in a phase command string.
/// Returns None when the command uses {{file}} or {{files}} but context has no files.
fn interpolate_phase_cmd(cmd: &str, context: &ContextManager) -> Option<String> {
    let files = context.list_files();
    let uses_file_placeholder = cmd.contains("{{file}}") || cmd.contains("{{files}}");
    if uses_file_placeholder && files.is_empty() {
        return None;
    }
    let first = files
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let all = files
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    Some(cmd.replace("{{file}}", &first).replace("{{files}}", &all))
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
    files: &[PathBuf],
    template_name: Option<&str>,
) -> Result<()> {
    let model_config = config.get_model_config(model);
    let project_root = find_project_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Create a ContextManager to handle files and @references
    let mut context = ContextManager::new(config.context_limit, project_root.clone());

    if let Some(system_prompt) = &model_config.system_prompt {
        context.set_system_prompt(system_prompt.clone());
    }

    // Add files from --file flags
    for path in files {
        let path_str = path.to_string_lossy();
        if path.is_dir() {
            match context.add_directory(&*path_str) {
                Ok((added, _skipped)) => {
                    eprintln!(
                        "{} Added {} file(s) from {}",
                        style("✓").green(),
                        style(added).cyan(),
                        style(&path_str).cyan()
                    );
                }
                Err(e) => {
                    eprintln!("{} {}: {}", style("Error:").red(), path_str, e);
                }
            }
        } else {
            match context.add_file(&*path_str) {
                Ok(()) => {
                    eprintln!(
                        "{} Added {} to context",
                        style("✓").green(),
                        style(&path_str).cyan()
                    );
                }
                Err(e) => {
                    eprintln!("{} {}: {}", style("Error:").red(), path_str, e);
                }
            }
        }
    }

    // Resolve the actual prompt: either render a template or use as-is
    let actual_prompt = if let Some(tpl_name) = template_name {
        // Load templates
        let mut templates = TemplateManager::new();
        templates.load_defaults();
        let template_dirs = get_template_directories(&project_root);
        templates.load_from_directories(&template_dirs);

        // Look up the template
        let template = templates.get(tpl_name).ok_or_else(|| {
            crate::error::SlabError::TemplateError(format!("Template not found: {}", tpl_name))
        })?;
        let template_display_name = template.name.clone();

        // Parse prompt string as key=value pairs for template variables
        let mut variables = HashMap::new();
        let mut content_parts = Vec::new();

        for part in prompt.split_whitespace() {
            if let Some((key, value)) = part.split_once('=') {
                variables.insert(key.to_string(), value.to_string());
            } else {
                content_parts.push(part);
            }
        }

        if !content_parts.is_empty() {
            variables.insert("content".to_string(), content_parts.join(" "));
        }

        // Render the template
        let rendered = templates
            .render(&template_display_name, &variables, &context)
            .map_err(crate::error::SlabError::TemplateError)?;

        eprintln!(
            "{} {} {}",
            style("→").cyan(),
            style("Using template:").dim(),
            style(tpl_name).yellow()
        );

        rendered
    } else {
        // Expand @file references in the prompt
        context.expand_file_references(prompt)
    };

    // Add user message and build the full message list
    context.add_message(Message::user(&actual_prompt));
    let messages = context.build_messages();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ollama::{ChatRequest, LlmBackend, ModelInfo};
    use crate::templates::{PhaseFeedback, PhaseOutcome, TemplatePhase};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    // ── Mock LLM backend ─────────────────────────────────────────────────────

    /// Records every request sent so tests can assert on the content.
    struct MockLlmBackend {
        /// Canned response to return for every request.
        response: String,
        /// The last-user-message content from each `llm_chat` call, in order.
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl MockLlmBackend {
        fn new(response: impl Into<String>) -> (Self, Arc<Mutex<Vec<String>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            let backend = Self {
                response: response.into(),
                sent: Arc::clone(&sent),
            };
            (backend, sent)
        }
    }

    impl LlmBackend for MockLlmBackend {
        async fn llm_chat(&self, request: ChatRequest) -> crate::error::Result<String> {
            // Record the last user message so tests can inspect what was sent.
            if let Some(last) = request.messages.iter().rev().find(|m| m.role == "user") {
                self.sent.lock().unwrap().push(last.content.clone());
            }
            Ok(self.response.clone())
        }

        async fn llm_stream(
            &self,
            request: ChatRequest,
        ) -> crate::error::Result<mpsc::Receiver<crate::error::Result<String>>> {
            let text = self.llm_chat(request).await?;
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let _ = tx.send(Ok(text)).await;
            });
            Ok(rx)
        }

        async fn llm_list_models(&self) -> crate::error::Result<Vec<ModelInfo>> {
            Ok(vec![])
        }
    }

    // ── Helper ────────────────────────────────────────────────────────────────

    fn make_repl(backend: MockLlmBackend) -> Repl<MockLlmBackend> {
        Repl::new(backend, Config::default(), "test-model".into(), false)
    }

    fn phase(run: &str, on_success: PhaseOutcome, on_failure: PhaseOutcome) -> TemplatePhase {
        TemplatePhase {
            name: Some("test".into()),
            run: run.to_string(),
            on_success,
            on_failure,
            feedback: PhaseFeedback::OnFailure,
            follow_up: None,
        }
    }

    // ── interpolate_phase_cmd tests ───────────────────────────────────────────

    #[test]
    fn test_interpolate_no_files_no_placeholder() {
        // Commands without {{file}} should always return Some, even with no context files.
        let ctx = crate::context::ContextManager::new(4096, PathBuf::from("."));
        let result = interpolate_phase_cmd("echo hello", &ctx);
        assert_eq!(result, Some("echo hello".to_string()));
    }

    #[test]
    fn test_interpolate_file_placeholder_empty_context() {
        // Bug 3: {{file}} with no context files → None (phase should be skipped).
        let ctx = crate::context::ContextManager::new(4096, PathBuf::from("."));
        let result = interpolate_phase_cmd("gcc {{file}}", &ctx);
        assert!(result.is_none(), "expected None when context has no files");
    }

    #[test]
    fn test_interpolate_files_placeholder_empty_context() {
        let ctx = crate::context::ContextManager::new(4096, PathBuf::from("."));
        let result = interpolate_phase_cmd("clang-tidy {{files}}", &ctx);
        assert!(result.is_none());
    }

    // ── Template YAML field serialization ─────────────────────────────────────

    #[test]
    fn test_phase_feedback_default_is_on_failure() {
        let yaml = r#"
name: test
command: /test
description: test
prompt: hello
phases:
  - name: check
    run: "true"
"#;
        let tpl: crate::templates::PromptTemplate =
            serde_yaml::from_str(yaml).expect("parse failed");
        assert_eq!(tpl.phases[0].feedback, PhaseFeedback::OnFailure);
        assert_eq!(tpl.phases[0].follow_up, None);
    }

    #[test]
    fn test_template_new_fields_deserialize() {
        let yaml = r#"
name: test
command: /test
description: test
prompt: hello
phases_follow_up: "Fix the issues."
max_phases: 3
phases:
  - name: compile
    run: "gcc {{file}}"
    feedback: always
    on_success: stop
    on_failure: continue
    follow_up: "Please fix compile errors."
"#;
        let tpl: crate::templates::PromptTemplate =
            serde_yaml::from_str(yaml).expect("parse failed");
        assert_eq!(tpl.phases_follow_up.as_deref(), Some("Fix the issues."));
        assert_eq!(tpl.max_phases, Some(3));
        assert_eq!(tpl.phases[0].feedback, PhaseFeedback::Always);
        assert_eq!(
            tpl.phases[0].follow_up.as_deref(),
            Some("Please fix compile errors.")
        );
    }

    // ── run_phase_loop: Bug 1 — single message per pass ───────────────────────

    #[tokio::test]
    async fn test_single_message_sent_per_pass() {
        // The loop must call send_message exactly once per pass (not once for
        // context injection + once for the follow-up — that was Bug 1).
        let (backend, sent) = MockLlmBackend::new("fixed");
        let mut repl = make_repl(backend);

        let phases = vec![phase("false", PhaseOutcome::Stop, PhaseOutcome::Continue)];
        // One pass: answer y; second pass: answer n.
        let mut answers = vec![true, false].into_iter();
        repl.run_phase_loop(&phases, 10, None, |_| answers.next().unwrap_or(false))
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        // Exactly one llm_chat call per pass that had issues.
        assert_eq!(calls.len(), 1, "expected exactly 1 LLM call for 1 pass");
    }

    #[tokio::test]
    async fn test_single_message_contains_results_and_follow_up() {
        // The one message per pass must contain both phase output AND follow-up text.
        // confirm=true + max_iterations=1 → message fires on pass 1, loop ends on pass 2.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![TemplatePhase {
            name: Some("mycheck".into()),
            run: "sh -c 'echo myoutput; exit 1'".to_string(),
            on_success: PhaseOutcome::Stop,
            on_failure: PhaseOutcome::Continue,
            feedback: PhaseFeedback::OnFailure,
            follow_up: Some("My phase follow-up.".into()),
        }];
        repl.run_phase_loop(&phases, 1, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let msg = &calls[0];
        assert!(
            msg.contains("myoutput"),
            "phase output missing from message"
        );
        assert!(
            msg.contains("My phase follow-up."),
            "follow-up missing from message"
        );
    }

    // ── run_phase_loop: Bug 2 — exec error respects on_failure ────────────────

    #[tokio::test]
    async fn test_exec_error_on_failure_stop_does_not_loop() {
        // Bug 2: when a command fails to exec and on_failure == Stop, the loop
        // should NOT call send_message (i.e. should not trigger a continuation).
        let (backend, sent) = MockLlmBackend::new("ok");
        let mut repl = make_repl(backend);

        // Use an invalid command so exec itself fails; on_failure = Stop.
        let phases = vec![TemplatePhase {
            name: Some("bad".into()),
            run: "/nonexistent_binary_xyz_abc".to_string(),
            on_success: PhaseOutcome::Stop,
            on_failure: PhaseOutcome::Stop, // <-- Stop, not Continue
            feedback: PhaseFeedback::OnFailure,
            follow_up: None,
        }];
        repl.run_phase_loop(&phases, 10, None, |_| false)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(
            calls.len(),
            0,
            "exec error with on_failure=Stop must not trigger LLM call"
        );
    }

    #[tokio::test]
    async fn test_exec_error_on_failure_continue_does_loop() {
        // Exec error with on_failure=Continue SHOULD loop.
        let (backend, sent) = MockLlmBackend::new("ok");
        let mut repl = make_repl(backend);

        let phases = vec![TemplatePhase {
            name: Some("bad".into()),
            run: "/nonexistent_binary_xyz_abc".to_string(),
            on_success: PhaseOutcome::Stop,
            on_failure: PhaseOutcome::Continue,
            feedback: PhaseFeedback::OnFailure,
            follow_up: None,
        }];
        // confirm=true + max_iterations=1 → message fires on pass 1, loop exits on pass 2.
        repl.run_phase_loop(&phases, 1, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(
            calls.len(),
            1,
            "exec error with on_failure=Continue must trigger LLM call"
        );
    }

    // ── run_phase_loop: Bug 3 — empty context skips {{file}} phases ───────────

    #[tokio::test]
    async fn test_empty_context_skips_file_phase() {
        // Bug 3: phase with {{file}} and no context files must be skipped entirely,
        // not run with an empty string substituted.
        let (backend, sent) = MockLlmBackend::new("ok");
        let mut repl = make_repl(backend);

        // This phase would fail for the wrong reason if {{file}} expanded to "".
        // With the fix it should simply be skipped (no continue triggered).
        let phases = vec![phase(
            "gcc -Wall {{file}}",
            PhaseOutcome::Stop,
            PhaseOutcome::Continue,
        )];
        repl.run_phase_loop(&phases, 10, None, |_| false)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(
            calls.len(),
            0,
            "phase should be skipped when context has no files"
        );
    }

    // ── PhaseFeedback: Always / Never ─────────────────────────────────────────

    #[tokio::test]
    async fn test_feedback_always_injects_on_success() {
        // feedback: Always must include phase output in LLM message even when
        // the command succeeds (exit 0).  on_success=Continue so the loop runs.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![TemplatePhase {
            name: Some("report".into()),
            run: "sh -c 'echo always_output; exit 0'".to_string(),
            on_success: PhaseOutcome::Continue,
            on_failure: PhaseOutcome::Continue,
            feedback: PhaseFeedback::Always,
            follow_up: None,
        }];
        repl.run_phase_loop(&phases, 1, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].contains("always_output"),
            "feedback:Always must inject output even on success"
        );
    }

    #[tokio::test]
    async fn test_feedback_on_failure_does_not_inject_on_success() {
        // feedback: OnFailure (default) must NOT include output when exit 0.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![TemplatePhase {
            name: Some("check".into()),
            run: "sh -c 'echo clean_output; exit 0'".to_string(),
            on_success: PhaseOutcome::Continue,
            on_failure: PhaseOutcome::Continue,
            feedback: PhaseFeedback::OnFailure,
            follow_up: None,
        }];
        repl.run_phase_loop(&phases, 1, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            !calls[0].contains("clean_output"),
            "feedback:OnFailure must not inject output on success"
        );
    }

    #[tokio::test]
    async fn test_feedback_never_never_injects() {
        // feedback: Never must not include phase output in the LLM message at all.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![TemplatePhase {
            name: Some("noisy".into()),
            run: "sh -c 'echo never_output; exit 1'".to_string(),
            on_success: PhaseOutcome::Stop,
            on_failure: PhaseOutcome::Continue,
            feedback: PhaseFeedback::Never,
            follow_up: None,
        }];
        repl.run_phase_loop(&phases, 1, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            !calls[0].contains("never_output"),
            "feedback:Never must not inject output"
        );
    }

    // ── Follow-up precedence ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_follow_up_per_phase_beats_template() {
        // Per-phase follow_up must take precedence over template-level phases_follow_up.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![TemplatePhase {
            name: Some("check".into()),
            run: "false".to_string(),
            on_success: PhaseOutcome::Stop,
            on_failure: PhaseOutcome::Continue,
            feedback: PhaseFeedback::OnFailure,
            follow_up: Some("PER_PHASE_FOLLOW_UP".into()),
        }];
        repl.run_phase_loop(&phases, 1, Some("TEMPLATE_FOLLOW_UP"), |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].contains("PER_PHASE_FOLLOW_UP"),
            "per-phase follow_up must appear in message"
        );
        assert!(
            !calls[0].contains("TEMPLATE_FOLLOW_UP"),
            "template follow_up must not appear when per-phase wins"
        );
    }

    #[tokio::test]
    async fn test_follow_up_template_beats_default() {
        // Template-level phases_follow_up must beat the hardcoded default.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![phase("false", PhaseOutcome::Stop, PhaseOutcome::Continue)];
        repl.run_phase_loop(&phases, 1, Some("TEMPLATE_FOLLOW_UP"), |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].contains("TEMPLATE_FOLLOW_UP"),
            "template follow_up must appear when no per-phase follow_up"
        );
        assert!(
            !calls[0].contains("The checks above found issues"),
            "default follow_up must not appear when template_follow_up is set"
        );
    }

    #[tokio::test]
    async fn test_follow_up_default_used_when_no_others() {
        // Hardcoded default must be used when neither per-phase nor template follow_up is set.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![phase("false", PhaseOutcome::Stop, PhaseOutcome::Continue)];
        repl.run_phase_loop(&phases, 1, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].contains("The checks above found issues"),
            "hardcoded default follow_up must be used when no others are set"
        );
    }

    // ── max_phases / loop limit ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_loop_stops_at_max_phases() {
        // Loop must stop after max_iterations even when user keeps answering y.
        let (backend, sent) = MockLlmBackend::new("done");
        let mut repl = make_repl(backend);

        let phases = vec![phase("false", PhaseOutcome::Stop, PhaseOutcome::Continue)];
        // Always say yes — the max_iterations cap must stop it at 3.
        repl.run_phase_loop(&phases, 3, None, |_| true)
            .await
            .unwrap();

        let calls = sent.lock().unwrap();
        assert_eq!(calls.len(), 3, "loop must stop after max_iterations passes");
    }
}
