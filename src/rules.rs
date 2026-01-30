use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// A rule that provides context/guidelines to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Rule name (derived from filename if not specified)
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,

    /// Glob patterns for files this rule applies to (e.g., "*.rs", "src/**/*.py")
    /// If empty, rule applies to all contexts
    #[serde(default)]
    pub applies_to: Vec<String>,

    /// Priority (higher = applied first). Default: 0
    #[serde(default)]
    pub priority: i32,

    /// Whether the rule is enabled. Default: true
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// The rule content (instructions for the LLM)
    pub content: String,
}

fn default_enabled() -> bool {
    true
}

impl Rule {
    /// Check if this rule applies to a given file path
    pub fn applies_to_file(&self, path: &Path) -> bool {
        if self.applies_to.is_empty() {
            return true; // No patterns = applies to everything
        }

        let path_str = path.to_string_lossy();

        for pattern_str in &self.applies_to {
            if let Ok(pattern) = Pattern::new(pattern_str) {
                if pattern.matches(&path_str) {
                    return true;
                }
                // Also try matching just the filename
                if let Some(filename) = path.file_name() {
                    if pattern.matches(&filename.to_string_lossy()) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

/// Manages rules loading and filtering
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Load rules from a directory
    pub fn load_from_directory(&mut self, dir: &Path) {
        if !dir.exists() || !dir.is_dir() {
            return;
        }

        // Load YAML rules
        self.load_yaml_rules(dir);

        // Load Markdown rules
        self.load_markdown_rules(dir);

        // Load plain text rules
        self.load_text_rules(dir);

        // Sort by priority (descending)
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    fn load_yaml_rules(&mut self, dir: &Path) {
        for ext in &["yaml", "yml"] {
            let pattern = dir.join(format!("*.{}", ext));
            if let Ok(entries) = glob::glob(&pattern.to_string_lossy()) {
                for entry in entries.flatten() {
                    if let Err(e) = self.load_yaml_rule(&entry) {
                        eprintln!("Warning: Failed to load rule {:?}: {}", entry, e);
                    }
                }
            }
        }
    }

    fn load_yaml_rule(&mut self, path: &Path) -> Result<(), String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let rule: Rule = serde_yaml::from_str(&content)
            .map_err(|e| format!("Failed to parse YAML: {}", e))?;

        if rule.enabled {
            self.rules.push(rule);
        }

        Ok(())
    }

    fn load_markdown_rules(&mut self, dir: &Path) {
        let pattern = dir.join("*.md");
        if let Ok(entries) = glob::glob(&pattern.to_string_lossy()) {
            for entry in entries.flatten() {
                if let Ok(rule) = self.parse_markdown_rule(&entry) {
                    if rule.enabled {
                        self.rules.push(rule);
                    }
                }
            }
        }
    }

    fn parse_markdown_rule(&self, path: &Path) -> Result<Rule, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Extract name from filename
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();

        // Check for YAML frontmatter
        let (metadata, rule_content) = if content.starts_with("---") {
            if let Some(end_idx) = content[3..].find("---") {
                let frontmatter = &content[3..end_idx + 3];
                let rest = &content[end_idx + 6..];

                // Try to parse frontmatter
                if let Ok(meta) = serde_yaml::from_str::<RuleMetadata>(frontmatter) {
                    (Some(meta), rest.trim().to_string())
                } else {
                    (None, content)
                }
            } else {
                (None, content)
            }
        } else {
            (None, content)
        };

        Ok(Rule {
            name: metadata.as_ref().and_then(|m| m.name.clone()).unwrap_or(name),
            description: metadata.as_ref().and_then(|m| m.description.clone()),
            applies_to: metadata.as_ref().map(|m| m.applies_to.clone()).unwrap_or_default(),
            priority: metadata.as_ref().map(|m| m.priority).unwrap_or(0),
            enabled: metadata.as_ref().map(|m| m.enabled).unwrap_or(true),
            content: rule_content,
        })
    }

    fn load_text_rules(&mut self, dir: &Path) {
        let pattern = dir.join("*.txt");
        if let Ok(entries) = glob::glob(&pattern.to_string_lossy()) {
            for entry in entries.flatten() {
                if let Ok(content) = fs::read_to_string(&entry) {
                    let name = entry
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unnamed")
                        .to_string();

                    self.rules.push(Rule {
                        name,
                        description: None,
                        applies_to: Vec::new(),
                        priority: 0,
                        enabled: true,
                        content,
                    });
                }
            }
        }
    }

    /// Get all enabled rules
    pub fn all_rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Filter rules that apply to specific files
    pub fn rules_for_files(&self, files: &[PathBuf]) -> Vec<&Rule> {
        if files.is_empty() {
            // No files = return all rules with no applies_to restriction
            return self.rules
                .iter()
                .filter(|r| r.enabled && r.applies_to.is_empty())
                .collect();
        }

        self.rules
            .iter()
            .filter(|rule| {
                if !rule.enabled {
                    return false;
                }
                if rule.applies_to.is_empty() {
                    return true; // Applies to everything
                }
                // Check if rule applies to any of the files
                files.iter().any(|f| rule.applies_to_file(f))
            })
            .collect()
    }

    /// Build a combined rules prompt from applicable rules
    pub fn build_rules_prompt(&self, files: &[PathBuf]) -> Option<String> {
        let applicable_rules = self.rules_for_files(files);

        if applicable_rules.is_empty() {
            return None;
        }

        let mut sections = Vec::new();

        for rule in applicable_rules {
            let section = if let Some(desc) = &rule.description {
                format!("### {} ({})\n\n{}", rule.name, desc, rule.content)
            } else {
                format!("### {}\n\n{}", rule.name, rule.content)
            };
            sections.push(section);
        }

        Some(sections.join("\n\n"))
    }

    /// Get the number of loaded rules
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Optional metadata for markdown rules (in frontmatter)
#[derive(Debug, Deserialize)]
struct RuleMetadata {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    applies_to: Vec<String>,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_applies_to_file() {
        let rule = Rule {
            name: "test".to_string(),
            description: None,
            applies_to: vec!["*.rs".to_string(), "*.py".to_string()],
            priority: 0,
            enabled: true,
            content: "test content".to_string(),
        };

        assert!(rule.applies_to_file(Path::new("src/main.rs")));
        assert!(rule.applies_to_file(Path::new("test.py")));
        assert!(!rule.applies_to_file(Path::new("readme.md")));
    }

    #[test]
    fn test_empty_applies_to_matches_all() {
        let rule = Rule {
            name: "test".to_string(),
            description: None,
            applies_to: Vec::new(),
            priority: 0,
            enabled: true,
            content: "test content".to_string(),
        };

        assert!(rule.applies_to_file(Path::new("anything.txt")));
        assert!(rule.applies_to_file(Path::new("src/main.rs")));
    }
}
