use console::Style;

use crate::theme::{BoxStyle, Theme};

/// Get the current terminal width
pub fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

/// Truncate a string to fit within a given width, adding ellipsis if needed
#[allow(dead_code)]
pub fn truncate_str(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        format!("{}...", &s[..max_width - 3])
    }
}

/// Wrap text to fit within a given width
#[allow(dead_code)]
pub fn wrap_text(s: &str, width: usize) -> Vec<String> {
    textwrap::wrap(s, width)
        .into_iter()
        .map(|c| c.to_string())
        .collect()
}

/// Calculate the display width of a string (handling Unicode properly)
#[allow(dead_code)]
pub fn display_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// Box renderer for creating bordered panels
pub struct BoxRenderer {
    style: BoxStyle,
    theme: Theme,
    width: usize,
}

impl BoxRenderer {
    pub fn new(style: BoxStyle, theme: Theme) -> Self {
        Self {
            style,
            theme,
            width: 60,
        }
    }

    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// Render a box with an optional title
    pub fn render_titled_box(&self, title: Option<&str>, content: &str) -> String {
        let chars = self.style.chars();
        let inner_width = self.width.saturating_sub(2);
        let mut output = String::new();

        // Top border with title
        output.push_str(&format!("{}", self.theme.border.apply_to(chars.top_left)));

        if let Some(t) = title {
            let title_str = format!(" {} ", t);
            let title_len = title_str.len();
            let remaining = inner_width.saturating_sub(title_len);
            output.push_str(&format!(
                "{}{}",
                self.theme.primary.apply_to(&title_str),
                self.theme.border.apply_to(
                    std::iter::repeat_n(chars.horizontal, remaining)
                        .collect::<String>()
                )
            ));
        } else {
            output.push_str(&format!(
                "{}",
                self.theme.border.apply_to(
                    std::iter::repeat_n(chars.horizontal, inner_width)
                        .collect::<String>()
                )
            ));
        }

        output.push_str(&format!(
            "{}\n",
            self.theme.border.apply_to(chars.top_right)
        ));

        // Content lines
        for line in content.lines() {
            output.push_str(&format!("{} ", self.theme.border.apply_to(chars.vertical)));
            output.push_str(line);
            output.push('\n');
        }

        // Bottom border
        output.push_str(&format!(
            "{}{}{}",
            self.theme.border.apply_to(chars.bottom_left),
            self.theme.border.apply_to(
                std::iter::repeat_n(chars.horizontal, inner_width)
                    .collect::<String>()
            ),
            self.theme.border.apply_to(chars.bottom_right)
        ));
        output.push('\n');

        output
    }

    /// Render a box with a styled title (for errors, warnings, etc.)
    pub fn render_styled_box(&self, title: &str, content: &str, title_style: &Style) -> String {
        let chars = self.style.chars();
        let inner_width = self.width.saturating_sub(2);
        let mut output = String::new();

        // Top border with styled title
        output.push_str(&format!("{}", self.theme.border.apply_to(chars.top_left)));

        let title_str = format!(" {} ", title);
        let title_len = title_str.len();
        let remaining = inner_width.saturating_sub(title_len);
        output.push_str(&format!(
            "{}{}",
            title_style.apply_to(&title_str),
            self.theme.border.apply_to(
                std::iter::repeat_n(chars.horizontal, remaining)
                    .collect::<String>()
            )
        ));

        output.push_str(&format!(
            "{}\n",
            self.theme.border.apply_to(chars.top_right)
        ));

        // Content lines
        for line in content.lines() {
            output.push_str(&format!(
                "{} {}\n",
                self.theme.border.apply_to(chars.vertical),
                line
            ));
        }

        // Bottom border
        output.push_str(&format!(
            "{}{}{}",
            self.theme.border.apply_to(chars.bottom_left),
            self.theme.border.apply_to(
                std::iter::repeat_n(chars.horizontal, inner_width)
                    .collect::<String>()
            ),
            self.theme.border.apply_to(chars.bottom_right)
        ));
        output.push('\n');

        output
    }
}

/// Quick helper to create an info box
#[allow(dead_code)]
pub fn info_box(content: &str) -> String {
    let theme = Theme::default();
    let renderer = BoxRenderer::new(BoxStyle::Rounded, theme.clone()).with_width(60);
    renderer.render_styled_box("Info", content, &theme.primary)
}

/// Quick helper to create an error box
#[allow(dead_code)]
pub fn error_box(content: &str) -> String {
    let theme = Theme::default();
    let renderer = BoxRenderer::new(BoxStyle::Rounded, theme.clone()).with_width(60);
    renderer.render_styled_box("Error", content, &theme.error)
}

/// Quick helper to create a success box
#[allow(dead_code)]
pub fn success_box(content: &str) -> String {
    let theme = Theme::default();
    let renderer = BoxRenderer::new(BoxStyle::Rounded, theme.clone()).with_width(60);
    renderer.render_styled_box("Success", content, &theme.success)
}

/// Quick helper to create a warning box
#[allow(dead_code)]
pub fn warning_box(content: &str) -> String {
    let theme = Theme::default();
    let renderer = BoxRenderer::new(BoxStyle::Rounded, theme.clone()).with_width(60);
    renderer.render_styled_box("Warning", content, &theme.warning)
}

/// Create a simple horizontal divider
#[allow(dead_code)]
pub fn divider(width: usize) -> String {
    "─".repeat(width)
}

/// Create a divider with a title
#[allow(dead_code)]
pub fn titled_divider(title: &str, width: usize) -> String {
    let title_part = format!("─ {} ", title);
    let remaining = width.saturating_sub(title_part.chars().count());
    format!("{}{}", title_part, "─".repeat(remaining))
}

/// Unicode symbols for various UI elements
#[allow(dead_code)]
pub mod symbols {
    pub const CHECK: &str = "✓";
    pub const CROSS: &str = "✗";
    pub const WARNING: &str = "⚠";
    pub const ARROW_RIGHT: &str = "→";
    pub const ARROW_LEFT: &str = "←";
    pub const BULLET: &str = "•";
    pub const PROMPT: &str = "❯";
    pub const ELLIPSIS: &str = "…";
    pub const VERTICAL_LINE: &str = "│";
    pub const HORIZONTAL_LINE: &str = "─";
}

/// Spinner character sets
#[allow(dead_code)]
pub mod spinners {
    pub const DOTS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    pub const CIRCLES: &[char] = &['◐', '◓', '◑', '◒'];
    pub const BLOCKS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
    pub const LINE: &[char] = &['|', '/', '-', '\\'];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 8), "hello...");
        assert_eq!(truncate_str("hi", 2), "hi");
    }

    #[test]
    fn test_divider() {
        assert_eq!(divider(5), "─────");
    }

    #[test]
    fn test_titled_divider() {
        let result = titled_divider("Title", 20);
        assert!(result.starts_with("─ Title "));
        assert_eq!(result.chars().count(), 20);
    }
}
