use handlebars::Handlebars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::context::ContextManager;

/// A prompt template with variables and Handlebars syntax
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// Template name (e.g., "code_review")
    pub name: String,

    /// Slash command to invoke (e.g., "/review")
    pub command: String,

    /// Human-readable description
    pub description: String,

    /// Variables with optional defaults
    #[serde(default)]
    pub variables: Vec<TemplateVariable>,

    /// The prompt template using Handlebars syntax
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVariable {
    pub name: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Manages prompt templates
pub struct TemplateManager {
    templates: HashMap<String, PromptTemplate>,
    handlebars: Handlebars<'static>,
}

impl TemplateManager {
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(false); // Allow missing variables

        Self {
            templates: HashMap::new(),
            handlebars,
        }
    }

    /// Load templates from multiple directories
    pub fn load_from_directories(&mut self, dirs: &[PathBuf]) {
        for dir in dirs {
            if dir.exists() && dir.is_dir() {
                self.load_from_directory(dir);
            }
        }
    }

    /// Load all YAML templates from a directory
    pub fn load_from_directory(&mut self, dir: &Path) {
        let pattern = dir.join("*.yaml");
        let pattern_str = pattern.to_string_lossy();

        if let Ok(entries) = glob::glob(&pattern_str) {
            for entry in entries.flatten() {
                if let Err(e) = self.load_template(&entry) {
                    eprintln!("Warning: Failed to load template {:?}: {}", entry, e);
                }
            }
        }

        // Also try .yml extension
        let pattern = dir.join("*.yml");
        let pattern_str = pattern.to_string_lossy();

        if let Ok(entries) = glob::glob(&pattern_str) {
            for entry in entries.flatten() {
                if let Err(e) = self.load_template(&entry) {
                    eprintln!("Warning: Failed to load template {:?}: {}", entry, e);
                }
            }
        }
    }

    /// Load a single template from a YAML file
    pub fn load_template(&mut self, path: &Path) -> Result<(), String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read template file: {}", e))?;

        let template: PromptTemplate = serde_yaml::from_str(&content)
            .map_err(|e| format!("Failed to parse template YAML: {}", e))?;

        // Register the template with Handlebars
        self.handlebars
            .register_template_string(&template.name, &template.prompt)
            .map_err(|e| format!("Failed to register template: {}", e))?;

        // Store by command (without leading slash)
        let command_key = template.command.trim_start_matches('/').to_string();
        self.templates.insert(command_key, template);

        Ok(())
    }

    /// Load built-in default templates
    pub fn load_defaults(&mut self) {
        let defaults = get_default_templates();
        for template in defaults {
            if let Err(e) = self.handlebars
                .register_template_string(&template.name, &template.prompt)
            {
                eprintln!("Warning: Failed to register default template {}: {}", template.name, e);
                continue;
            }
            let command_key = template.command.trim_start_matches('/').to_string();
            self.templates.insert(command_key, template);
        }
    }

    /// Get a template by command (without leading slash)
    pub fn get(&self, command: &str) -> Option<&PromptTemplate> {
        let key = command.trim_start_matches('/');
        self.templates.get(key)
    }

    /// List all available templates
    pub fn list(&self) -> Vec<&PromptTemplate> {
        self.templates.values().collect()
    }

    /// List template commands
    pub fn commands(&self) -> Vec<String> {
        self.templates
            .values()
            .map(|t| t.command.clone())
            .collect()
    }

    /// Render a template with the given variables and context
    pub fn render(
        &self,
        template_name: &str,
        variables: &HashMap<String, String>,
        context: &ContextManager,
    ) -> Result<String, String> {
        let template = self.templates
            .values()
            .find(|t| t.name == template_name || t.command.trim_start_matches('/') == template_name)
            .ok_or_else(|| format!("Template not found: {}", template_name))?;

        // Build the render context
        let mut render_data = HashMap::new();

        // Add user-provided variables
        for (key, value) in variables {
            render_data.insert(key.clone(), value.clone());
        }

        // Add default values for missing variables
        for var in &template.variables {
            if !render_data.contains_key(&var.name) {
                if let Some(default) = &var.default {
                    render_data.insert(var.name.clone(), default.clone());
                }
            }
        }

        // Add built-in variables
        render_data.insert("date".to_string(), chrono::Local::now().format("%Y-%m-%d").to_string());
        render_data.insert("datetime".to_string(), chrono::Local::now().format("%Y-%m-%d %H:%M").to_string());

        // Add project name (current directory name)
        if let Ok(cwd) = std::env::current_dir() {
            if let Some(name) = cwd.file_name().and_then(|n| n.to_str()) {
                render_data.insert("project".to_string(), name.to_string());
            }
        }

        // Add files from context
        let files = context.list_files();
        if !files.is_empty() {
            let mut files_content = String::new();
            for path in files {
                if let Some(content) = context.get_file_content(path) {
                    let ext = path.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("txt");
                    files_content.push_str(&format!(
                        "### {}\n```{}\n{}\n```\n\n",
                        path.display(),
                        ext,
                        content
                    ));
                }
            }
            render_data.insert("files".to_string(), files_content);
        }

        // Render the template
        self.handlebars
            .render(&template.name, &render_data)
            .map_err(|e| format!("Failed to render template: {}", e))
    }

    /// Check if a command matches a template
    pub fn is_template_command(&self, command: &str) -> bool {
        let key = command.trim_start_matches('/');
        self.templates.contains_key(key)
    }
}

impl Default for TemplateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the default built-in templates
fn get_default_templates() -> Vec<PromptTemplate> {
    vec![
        PromptTemplate {
            name: "code_review".to_string(),
            command: "/review".to_string(),
            description: "Review code for issues and improvements".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "focus".to_string(),
                    default: Some("all aspects".to_string()),
                    description: Some("What to focus on (security, performance, style, etc.)".to_string()),
                },
            ],
            prompt: r#"Please review the following code, focusing on {{focus}}.

{{#if content}}
{{content}}
{{/if}}

{{#if files}}
## Files to Review

{{files}}
{{/if}}

Please identify:
1. Potential bugs or issues
2. Security concerns
3. Performance improvements
4. Code style and readability improvements
5. Any other suggestions"#.to_string(),
        },
        PromptTemplate {
            name: "explain".to_string(),
            command: "/explain".to_string(),
            description: "Explain how code works".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "detail".to_string(),
                    default: Some("moderate".to_string()),
                    description: Some("Level of detail (brief, moderate, detailed)".to_string()),
                },
            ],
            prompt: r#"Please explain the following code with {{detail}} detail.

{{#if content}}
{{content}}
{{/if}}

{{#if files}}
## Code to Explain

{{files}}
{{/if}}

Explain:
1. What the code does at a high level
2. How the main components work
3. Any important patterns or techniques used
4. Potential edge cases or gotchas"#.to_string(),
        },
        PromptTemplate {
            name: "refactor".to_string(),
            command: "/refactor".to_string(),
            description: "Suggest refactoring improvements".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "goal".to_string(),
                    default: Some("improve readability and maintainability".to_string()),
                    description: Some("Refactoring goal".to_string()),
                },
            ],
            prompt: r#"Please suggest how to refactor the following code to {{goal}}.

{{#if content}}
{{content}}
{{/if}}

{{#if files}}
## Code to Refactor

{{files}}
{{/if}}

For each suggestion:
1. Explain what to change and why
2. Show the refactored code
3. Note any trade-offs or considerations"#.to_string(),
        },
        PromptTemplate {
            name: "test_gen".to_string(),
            command: "/test".to_string(),
            description: "Generate tests for code".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "framework".to_string(),
                    default: Some("the appropriate testing framework".to_string()),
                    description: Some("Testing framework to use".to_string()),
                },
            ],
            prompt: r#"Please generate comprehensive tests for the following code using {{framework}}.

{{#if content}}
{{content}}
{{/if}}

{{#if files}}
## Code to Test

{{files}}
{{/if}}

Include tests for:
1. Normal/happy path cases
2. Edge cases
3. Error conditions
4. Boundary conditions

Use descriptive test names that explain what is being tested."#.to_string(),
        },
        PromptTemplate {
            name: "fix".to_string(),
            command: "/fix".to_string(),
            description: "Fix a bug or issue".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "issue".to_string(),
                    default: None,
                    description: Some("Description of the bug or issue".to_string()),
                },
            ],
            prompt: r#"Please help fix the following issue: {{issue}}

{{#if files}}
## Relevant Code

{{files}}
{{/if}}

Please:
1. Identify the root cause
2. Explain the fix
3. Provide the corrected code
4. Suggest how to prevent similar issues"#.to_string(),
        },
        PromptTemplate {
            name: "document".to_string(),
            command: "/doc".to_string(),
            description: "Generate documentation for code".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "style".to_string(),
                    default: Some("the language's standard documentation style".to_string()),
                    description: Some("Documentation style".to_string()),
                },
            ],
            prompt: r#"Please generate documentation for the following code using {{style}}.

{{#if content}}
{{content}}
{{/if}}

{{#if files}}
## Code to Document

{{files}}
{{/if}}

Include:
1. Function/method descriptions
2. Parameter documentation
3. Return value documentation
4. Usage examples where appropriate"#.to_string(),
        },
    ]
}

/// Write default templates to a directory
pub fn write_default_templates(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create templates directory: {}", e))?;

    let defaults = get_default_templates();
    for template in defaults {
        let filename = format!("{}.yaml", template.name);
        let path = dir.join(&filename);

        // Don't overwrite existing templates
        if path.exists() {
            continue;
        }

        let yaml = serde_yaml::to_string(&template)
            .map_err(|e| format!("Failed to serialize template: {}", e))?;

        fs::write(&path, yaml)
            .map_err(|e| format!("Failed to write template file: {}", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_defaults() {
        let mut manager = TemplateManager::new();
        manager.load_defaults();

        assert!(manager.get("review").is_some());
        assert!(manager.get("explain").is_some());
        assert!(manager.get("refactor").is_some());
        assert!(manager.get("test").is_some());
    }

    #[test]
    fn test_render_template() {
        let mut manager = TemplateManager::new();
        manager.load_defaults();

        let context = ContextManager::new(4096, PathBuf::from("."));
        let mut vars = HashMap::new();
        vars.insert("content".to_string(), "fn main() {}".to_string());

        let result = manager.render("code_review", &vars, &context);
        assert!(result.is_ok());
        let rendered = result.unwrap();
        assert!(rendered.contains("fn main() {}"));
    }
}
