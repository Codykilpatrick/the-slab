use console::style;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

/// Syntax highlighter for code blocks
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Highlight a code block with the given language
    pub fn highlight(&self, code: &str, language: &str) -> String {
        let theme = &self.theme_set.themes["base16-ocean.dark"];

        // Try to find syntax for the language
        let syntax = self
            .syntax_set
            .find_syntax_by_token(language)
            .or_else(|| self.syntax_set.find_syntax_by_extension(language))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut result = String::new();

        for line in code.lines() {
            match highlighter.highlight_line(line, &self.syntax_set) {
                Ok(ranges) => {
                    let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
                    result.push_str(&escaped);
                    result.push_str("\x1b[0m\n");
                }
                Err(_) => {
                    result.push_str(line);
                    result.push('\n');
                }
            }
        }

        // Remove trailing newline if original didn't have one
        if !code.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        result
    }

    /// Format a response with syntax-highlighted code blocks
    pub fn format_response(&self, response: &str) -> String {
        let mut result = String::new();
        let mut in_code_block = false;
        let mut code_buffer = String::new();
        let mut current_lang = String::new();

        for line in response.lines() {
            if line.starts_with("```") {
                if in_code_block {
                    // End of code block - highlight and append
                    result.push_str(&format!("{}\n", style("─".repeat(40)).dim()));
                    result.push_str(&self.highlight(&code_buffer, &current_lang));
                    result.push_str(&format!("\n{}\n", style("─".repeat(40)).dim()));
                    code_buffer.clear();
                    current_lang.clear();
                    in_code_block = false;
                } else {
                    // Start of code block
                    current_lang = line[3..].trim().to_string();
                    // Strip filename if present (e.g., "rust:src/main.rs" -> "rust")
                    if let Some(colon_pos) = current_lang.find(':') {
                        let (lang, path) = current_lang.split_at(colon_pos);
                        result.push_str(&format!(
                            "{} {}\n",
                            style(&current_lang[..colon_pos]).cyan(),
                            style(&current_lang[colon_pos + 1..]).dim()
                        ));
                        current_lang = lang.to_string();
                    } else if !current_lang.is_empty() {
                        result.push_str(&format!("{}\n", style(&current_lang).cyan()));
                    }
                    in_code_block = true;
                }
            } else if in_code_block {
                code_buffer.push_str(line);
                code_buffer.push('\n');
            } else {
                result.push_str(line);
                result.push('\n');
            }
        }

        // Handle unclosed code block
        if in_code_block && !code_buffer.is_empty() {
            result.push_str(&format!("{}\n", style("─".repeat(40)).dim()));
            result.push_str(&self.highlight(&code_buffer, &current_lang));
            result.push_str(&format!("\n{}\n", style("─".repeat(40)).dim()));
        }

        // Remove trailing newline if original didn't have one
        if !response.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        result
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust() {
        let highlighter = Highlighter::new();
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let result = highlighter.highlight(code, "rust");
        // Should contain ANSI escape codes
        assert!(result.contains("\x1b["));
    }

    #[test]
    fn test_format_response_with_code() {
        let highlighter = Highlighter::new();
        let response = "Here's some code:\n```rust\nfn main() {}\n```\nThat's it.";
        let result = highlighter.format_response(response);
        // Should contain the formatted output
        assert!(result.contains("That's it"));
    }
}
