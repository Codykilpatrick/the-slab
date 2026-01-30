use console::Style;

/// Available theme names
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeName {
    Default,
    Monokai,
    Nord,
    Solarized,
    Minimal,
    Dracula,
}

impl ThemeName {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "monokai" => ThemeName::Monokai,
            "nord" => ThemeName::Nord,
            "solarized" => ThemeName::Solarized,
            "minimal" => ThemeName::Minimal,
            "dracula" => ThemeName::Dracula,
            _ => ThemeName::Default,
        }
    }

    pub fn to_theme(self) -> Theme {
        match self {
            ThemeName::Default => Theme::default(),
            ThemeName::Monokai => Theme::monokai(),
            ThemeName::Nord => Theme::nord(),
            ThemeName::Solarized => Theme::solarized(),
            ThemeName::Minimal => Theme::minimal(),
            ThemeName::Dracula => Theme::dracula(),
        }
    }

    #[allow(dead_code)]
    pub fn all() -> &'static [ThemeName] {
        &[
            ThemeName::Default,
            ThemeName::Monokai,
            ThemeName::Nord,
            ThemeName::Solarized,
            ThemeName::Minimal,
            ThemeName::Dracula,
        ]
    }

    #[allow(dead_code)]
    pub fn name(&self) -> &'static str {
        match self {
            ThemeName::Default => "default",
            ThemeName::Monokai => "monokai",
            ThemeName::Nord => "nord",
            ThemeName::Solarized => "solarized",
            ThemeName::Minimal => "minimal",
            ThemeName::Dracula => "dracula",
        }
    }
}

/// Semantic color theme with styles for different UI elements
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub primary: Style,
    pub secondary: Style,
    pub success: Style,
    pub warning: Style,
    pub error: Style,
    pub muted: Style,
    pub accent: Style,
    pub border: Style,
}

impl Theme {
    /// Default theme - cyan/blue palette
    pub fn default() -> Self {
        Self {
            primary: Style::new().cyan().bold(),
            secondary: Style::new().blue(),
            success: Style::new().green(),
            warning: Style::new().yellow(),
            error: Style::new().red().bold(),
            muted: Style::new().dim(),
            accent: Style::new().magenta(),
            border: Style::new().dim(),
        }
    }

    /// Monokai-inspired warm palette
    pub fn monokai() -> Self {
        Self {
            primary: Style::new().color256(208).bold(), // Orange
            secondary: Style::new().color256(141),      // Purple
            success: Style::new().color256(148),        // Green
            warning: Style::new().color256(228),        // Yellow
            error: Style::new().color256(197).bold(),   // Pink/Red
            muted: Style::new().color256(242),          // Gray
            accent: Style::new().color256(81),          // Cyan
            border: Style::new().color256(239),         // Dark gray
        }
    }

    /// Nord theme - cool blue palette
    pub fn nord() -> Self {
        Self {
            primary: Style::new().color256(110).bold(), // Frost blue
            secondary: Style::new().color256(109),      // Frost teal
            success: Style::new().color256(108),        // Aurora green
            warning: Style::new().color256(179),        // Aurora yellow
            error: Style::new().color256(174).bold(),   // Aurora red
            muted: Style::new().color256(60),           // Polar night
            accent: Style::new().color256(139),         // Aurora purple
            border: Style::new().color256(60),          // Polar night
        }
    }

    /// Solarized theme
    pub fn solarized() -> Self {
        Self {
            primary: Style::new().color256(37).bold(), // Cyan
            secondary: Style::new().color256(33),      // Blue
            success: Style::new().color256(64),        // Green
            warning: Style::new().color256(136),       // Yellow
            error: Style::new().color256(160).bold(),  // Red
            muted: Style::new().color256(245),         // Base1
            accent: Style::new().color256(125),        // Magenta
            border: Style::new().color256(240),        // Base01
        }
    }

    /// Minimal theme - grayscale with subtle color
    pub fn minimal() -> Self {
        Self {
            primary: Style::new().white().bold(),
            secondary: Style::new().color256(250),
            success: Style::new().color256(250),
            warning: Style::new().color256(250),
            error: Style::new().red(),
            muted: Style::new().color256(240),
            accent: Style::new().color256(250),
            border: Style::new().color256(236),
        }
    }

    /// Dracula theme
    pub fn dracula() -> Self {
        Self {
            primary: Style::new().color256(141).bold(), // Purple
            secondary: Style::new().color256(117),      // Cyan
            success: Style::new().color256(84),         // Green
            warning: Style::new().color256(228),        // Yellow
            error: Style::new().color256(210).bold(),   // Red/Pink
            muted: Style::new().color256(61),           // Comment gray
            accent: Style::new().color256(212),         // Pink
            border: Style::new().color256(60),          // Selection
        }
    }
}

/// Box drawing style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxStyle {
    Rounded,
    Sharp,
    Double,
    Ascii,
    Heavy,
}

impl BoxStyle {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sharp" => BoxStyle::Sharp,
            "double" => BoxStyle::Double,
            "ascii" => BoxStyle::Ascii,
            "heavy" => BoxStyle::Heavy,
            _ => BoxStyle::Rounded,
        }
    }

    /// Get the box drawing characters for this style
    pub fn chars(&self) -> BoxChars {
        match self {
            BoxStyle::Rounded => BoxChars {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
                left_tee: '├',
                right_tee: '┤',
                top_tee: '┬',
                bottom_tee: '┴',
                cross: '┼',
            },
            BoxStyle::Sharp => BoxChars {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                horizontal: '─',
                vertical: '│',
                left_tee: '├',
                right_tee: '┤',
                top_tee: '┬',
                bottom_tee: '┴',
                cross: '┼',
            },
            BoxStyle::Double => BoxChars {
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
                horizontal: '═',
                vertical: '║',
                left_tee: '╠',
                right_tee: '╣',
                top_tee: '╦',
                bottom_tee: '╩',
                cross: '╬',
            },
            BoxStyle::Ascii => BoxChars {
                top_left: '+',
                top_right: '+',
                bottom_left: '+',
                bottom_right: '+',
                horizontal: '-',
                vertical: '|',
                left_tee: '+',
                right_tee: '+',
                top_tee: '+',
                bottom_tee: '+',
                cross: '+',
            },
            BoxStyle::Heavy => BoxChars {
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
                horizontal: '━',
                vertical: '┃',
                left_tee: '┣',
                right_tee: '┫',
                top_tee: '┳',
                bottom_tee: '┻',
                cross: '╋',
            },
        }
    }
}

/// Box drawing characters
#[derive(Debug, Clone, Copy)]
pub struct BoxChars {
    pub top_left: char,
    pub top_right: char,
    pub bottom_left: char,
    pub bottom_right: char,
    pub horizontal: char,
    pub vertical: char,
    #[allow(dead_code)]
    pub left_tee: char,
    #[allow(dead_code)]
    pub right_tee: char,
    #[allow(dead_code)]
    pub top_tee: char,
    #[allow(dead_code)]
    pub bottom_tee: char,
    #[allow(dead_code)]
    pub cross: char,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_name_from_str() {
        assert_eq!(ThemeName::from_str("default"), ThemeName::Default);
        assert_eq!(ThemeName::from_str("monokai"), ThemeName::Monokai);
        assert_eq!(ThemeName::from_str("NORD"), ThemeName::Nord);
        assert_eq!(ThemeName::from_str("unknown"), ThemeName::Default);
    }

    #[test]
    fn test_box_style_from_str() {
        assert_eq!(BoxStyle::from_str("rounded"), BoxStyle::Rounded);
        assert_eq!(BoxStyle::from_str("sharp"), BoxStyle::Sharp);
        assert_eq!(BoxStyle::from_str("DOUBLE"), BoxStyle::Double);
        assert_eq!(BoxStyle::from_str("unknown"), BoxStyle::Rounded);
    }
}
