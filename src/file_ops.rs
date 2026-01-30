use console::{style, Style, Term};
use similar::{ChangeTag, TextDiff};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::{Result, SlabError};

/// Represents a file operation detected from LLM output
#[derive(Debug, Clone)]
pub enum FileOperation {
    Create {
        path: PathBuf,
        content: String,
        language: Option<String>,
    },
    Edit {
        path: PathBuf,
        new_content: String,
        original_content: Option<String>,
        language: Option<String>,
    },
    Delete {
        path: PathBuf,
        original_content: Option<String>,
    },
    #[allow(dead_code)]
    Rename { from: PathBuf, to: PathBuf },
}

impl FileOperation {
    /// Get the primary path affected by this operation
    pub fn path(&self) -> &Path {
        match self {
            FileOperation::Create { path, .. } => path,
            FileOperation::Edit { path, .. } => path,
            FileOperation::Delete { path, .. } => path,
            FileOperation::Rename { from, .. } => from,
        }
    }

    /// Check if this operation is safe to execute
    pub fn safety_check(&self, project_root: &Path) -> Result<()> {
        let path = self.path();

        // Ensure path is within project root
        let canonical_root = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        };

        // Try to canonicalize, or use the joined path for new files
        let canonical_path = full_path
            .canonicalize()
            .unwrap_or_else(|_| full_path.clone());

        if !canonical_path.starts_with(&canonical_root) {
            return Err(SlabError::FileOperation(format!(
                "Path '{}' is outside project root",
                path.display()
            )));
        }

        // Protect .git directory
        if path.components().any(|c| c.as_os_str() == ".git") {
            return Err(SlabError::FileOperation(
                "Cannot modify files in .git directory".to_string(),
            ));
        }

        // For rename, also check destination
        if let FileOperation::Rename { to, .. } = self {
            let to_full = if to.is_absolute() {
                to.to_path_buf()
            } else {
                project_root.join(to)
            };
            let to_canonical = to_full.canonicalize().unwrap_or(to_full);

            if !to_canonical.starts_with(&canonical_root) {
                return Err(SlabError::FileOperation(format!(
                    "Destination path '{}' is outside project root",
                    to.display()
                )));
            }
        }

        Ok(())
    }

    /// Generate a preview of this operation
    pub fn preview(&self) -> String {
        match self {
            FileOperation::Create {
                path,
                content,
                language,
            } => {
                let lang = language.as_deref().unwrap_or("text");
                format!(
                    "{} {} ({})\n{}",
                    style("CREATE").green().bold(),
                    style(path.display()).cyan(),
                    lang,
                    preview_content(content, language.as_deref())
                )
            }
            FileOperation::Edit {
                path,
                new_content,
                original_content,
                language,
            } => {
                let original = original_content.as_deref().unwrap_or("");
                format!(
                    "{} {}\n{}",
                    style("EDIT").yellow().bold(),
                    style(path.display()).cyan(),
                    generate_diff(original, new_content, language.as_deref())
                )
            }
            FileOperation::Delete { path, .. } => {
                format!(
                    "{} {}",
                    style("DELETE").red().bold(),
                    style(path.display()).cyan()
                )
            }
            FileOperation::Rename { from, to } => {
                format!(
                    "{} {} → {}",
                    style("RENAME").magenta().bold(),
                    style(from.display()).cyan(),
                    style(to.display()).cyan()
                )
            }
        }
    }

    /// Execute the file operation
    pub fn execute(&self, project_root: &Path) -> Result<()> {
        self.safety_check(project_root)?;

        match self {
            FileOperation::Create { path, content, .. } => {
                let full_path = project_root.join(path);
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&full_path, content)?;
            }
            FileOperation::Edit {
                path, new_content, ..
            } => {
                let full_path = project_root.join(path);
                fs::write(&full_path, new_content)?;
            }
            FileOperation::Delete { path, .. } => {
                let full_path = project_root.join(path);
                if full_path.is_dir() {
                    fs::remove_dir_all(&full_path)?;
                } else {
                    fs::remove_file(&full_path)?;
                }
            }
            FileOperation::Rename { from, to } => {
                let from_full = project_root.join(from);
                let to_full = project_root.join(to);
                if let Some(parent) = to_full.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&from_full, &to_full)?;
            }
        }

        Ok(())
    }

    /// Create a rollback operation (inverse of this operation)
    #[allow(dead_code)]
    pub fn rollback(&self) -> Option<FileOperation> {
        match self {
            FileOperation::Create { path, .. } => Some(FileOperation::Delete {
                path: path.clone(),
                original_content: None,
            }),
            FileOperation::Edit {
                path,
                original_content,
                language,
                ..
            } => original_content
                .as_ref()
                .map(|content| FileOperation::Edit {
                    path: path.clone(),
                    new_content: content.clone(),
                    original_content: None,
                    language: language.clone(),
                }),
            FileOperation::Delete {
                path,
                original_content,
            } => original_content
                .as_ref()
                .map(|content| FileOperation::Create {
                    path: path.clone(),
                    content: content.clone(),
                    language: None,
                }),
            FileOperation::Rename { from, to } => Some(FileOperation::Rename {
                from: to.clone(),
                to: from.clone(),
            }),
        }
    }

    /// Load original content for edit/delete operations
    pub fn load_original(&mut self, project_root: &Path) {
        match self {
            FileOperation::Edit {
                path,
                original_content,
                ..
            } => {
                if original_content.is_none() {
                    let full_path = project_root.join(path);
                    *original_content = fs::read_to_string(&full_path).ok();
                }
            }
            FileOperation::Delete {
                path,
                original_content,
            } => {
                if original_content.is_none() {
                    let full_path = project_root.join(path);
                    *original_content = fs::read_to_string(&full_path).ok();
                }
            }
            _ => {}
        }
    }
}

/// Generate a colored diff between two strings
fn generate_diff(old: &str, new: &str, _language: Option<&str>) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    for change in diff.iter_all_changes() {
        let (sign, s) = match change.tag() {
            ChangeTag::Delete => ("-", Style::new().red()),
            ChangeTag::Insert => ("+", Style::new().green()),
            ChangeTag::Equal => (" ", Style::new().dim()),
        };
        output.push_str(&format!("{}{}", s.apply_to(sign), s.apply_to(change)));
    }

    output
}

/// Preview content with optional syntax highlighting
fn preview_content(content: &str, _language: Option<&str>) -> String {
    // For now, just show the content with line numbers
    // Full syntax highlighting can be added later
    let lines: Vec<&str> = content.lines().collect();
    let max_lines = 20;
    let mut output = String::new();

    for (i, line) in lines.iter().take(max_lines).enumerate() {
        output.push_str(&format!(
            "{} {}\n",
            style(format!("{:4}", i + 1)).dim(),
            style(line).green()
        ));
    }

    if lines.len() > max_lines {
        output.push_str(&format!(
            "{}",
            style(format!("... and {} more lines", lines.len() - max_lines)).dim()
        ));
    }

    output
}

/// Parse LLM output to detect file operations
/// Looks for code blocks with filename annotations like:
/// ```rust:src/main.rs
/// ```python path=src/script.py
/// And delete markers like:
/// DELETE:src/old_file.rs
pub fn parse_file_operations(text: &str, project_root: &Path) -> Vec<FileOperation> {
    let mut operations = Vec::new();
    let mut in_code_block = false;
    let mut current_lang = None;
    let mut current_path: Option<PathBuf> = None;
    let mut current_content = String::new();

    for line in text.lines() {
        // Check for delete markers (outside of code blocks)
        if !in_code_block {
            if let Some(path) = parse_delete_marker(line) {
                let full_path = project_root.join(&path);
                if full_path.exists() {
                    let original = fs::read_to_string(&full_path).ok();
                    operations.push(FileOperation::Delete {
                        path,
                        original_content: original,
                    });
                }
                continue;
            }
        }

        if let Some(header) = line.strip_prefix("```") {
            if in_code_block {
                // End of code block
                if let Some(path) = current_path.take() {
                    let full_path = project_root.join(&path);
                    let op = if full_path.exists() {
                        let original = fs::read_to_string(&full_path).ok();
                        FileOperation::Edit {
                            path,
                            new_content: current_content.clone(),
                            original_content: original,
                            language: current_lang.take(),
                        }
                    } else {
                        FileOperation::Create {
                            path,
                            content: current_content.clone(),
                            language: current_lang.take(),
                        }
                    };
                    operations.push(op);
                }
                current_content.clear();
                in_code_block = false;
            } else {
                // Start of code block - parse the header
                let (lang, path) = parse_code_block_header(header);
                current_lang = lang;
                current_path = path;
                in_code_block = true;
            }
        } else if in_code_block && current_path.is_some() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    operations
}

/// Parse a delete marker line
/// Supports formats:
/// - DELETE:path/to/file
/// - DELETE: path/to/file
/// - [DELETE] path/to/file
fn parse_delete_marker(line: &str) -> Option<PathBuf> {
    let line = line.trim();

    // Format: DELETE:path or DELETE: path
    if let Some(rest) = line.strip_prefix("DELETE:") {
        let path = rest.trim();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    // Format: [DELETE] path
    if let Some(rest) = line.strip_prefix("[DELETE]") {
        let path = rest.trim();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    // Format: **DELETE:** path (markdown bold)
    if let Some(rest) = line.strip_prefix("**DELETE:**") {
        let path = rest.trim();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    None
}

/// Parse a code block header to extract language and file path
/// Supports formats:
/// - ```rust:src/main.rs
/// - ```python path=src/script.py
/// - ```js file:src/app.js
fn parse_code_block_header(header: &str) -> (Option<String>, Option<PathBuf>) {
    let header = header.trim();

    if header.is_empty() {
        return (None, None);
    }

    // Format: lang:path
    if let Some(colon_pos) = header.find(':') {
        let lang = &header[..colon_pos];
        let path = &header[colon_pos + 1..];
        if !path.is_empty() && !path.contains(' ') {
            return (Some(lang.to_string()), Some(PathBuf::from(path)));
        }
    }

    // Format: lang path=filepath or lang file:filepath
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() >= 2 {
        let lang = Some(parts[0].to_string());
        for part in &parts[1..] {
            if let Some(path) = part.strip_prefix("path=") {
                return (lang, Some(PathBuf::from(path)));
            }
            if let Some(path) = part.strip_prefix("file:") {
                return (lang, Some(PathBuf::from(path)));
            }
            if let Some(path) = part.strip_prefix("file=") {
                return (lang, Some(PathBuf::from(path)));
            }
        }
    }

    // Just language, no path
    let lang = parts.first().map(|s| s.to_string());
    (lang, None)
}

/// Interactive confirmation UI for file operations
pub struct FileOperationUI {
    #[allow(dead_code)]
    term: Term,
}

impl FileOperationUI {
    pub fn new() -> Self {
        Self {
            term: Term::stdout(),
        }
    }

    /// Show file operations and ask for confirmation
    /// Returns the list of operations that should be applied
    pub fn confirm_operations(
        &self,
        operations: &mut [FileOperation],
        project_root: &Path,
    ) -> Result<Vec<usize>> {
        if operations.is_empty() {
            return Ok(vec![]);
        }

        // Load original content for all operations
        for op in operations.iter_mut() {
            op.load_original(project_root);
        }

        println!();
        println!(
            "{} {} file operation(s) detected:",
            style("→").cyan(),
            operations.len()
        );
        println!();

        let mut approved = Vec::new();

        for (i, op) in operations.iter().enumerate() {
            // Safety check
            if let Err(e) = op.safety_check(project_root) {
                println!(
                    "{} {} - {}",
                    style("⚠").yellow(),
                    style(op.path().display()).cyan(),
                    style(e).red()
                );
                continue;
            }

            // Show operation type and path
            println!(
                "{} {}",
                style(format!("[{}]", i + 1)).dim(),
                short_preview(op)
            );
        }

        println!();
        print!(
            "{} ",
            style("[a]pply all, [v]iew details, [s]kip all, or enter numbers to apply:").dim()
        );
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "a" | "apply" | "y" | "yes" => {
                // Approve all safe operations
                for (i, op) in operations.iter().enumerate() {
                    if op.safety_check(project_root).is_ok() {
                        approved.push(i);
                    }
                }
            }
            "v" | "view" => {
                // Show detailed view and ask again
                for (i, op) in operations.iter().enumerate() {
                    println!();
                    println!("{}", style(format!("═══ Operation {} ═══", i + 1)).cyan());
                    println!("{}", op.preview());
                }
                println!();
                return self.confirm_individual(operations, project_root);
            }
            "s" | "skip" | "n" | "no" => {
                // Skip all
            }
            _ => {
                // Parse numbers
                for part in input.split(|c: char| c == ',' || c.is_whitespace()) {
                    if let Ok(num) = part.trim().parse::<usize>() {
                        if num > 0 && num <= operations.len() {
                            let idx = num - 1;
                            if operations[idx].safety_check(project_root).is_ok() {
                                approved.push(idx);
                            }
                        }
                    }
                }
            }
        }

        Ok(approved)
    }

    fn confirm_individual(
        &self,
        operations: &[FileOperation],
        project_root: &Path,
    ) -> Result<Vec<usize>> {
        let mut approved = Vec::new();

        for (i, op) in operations.iter().enumerate() {
            if op.safety_check(project_root).is_err() {
                continue;
            }

            print!(
                "{} Apply {} {}? [y/n]: ",
                style(format!("[{}]", i + 1)).dim(),
                operation_type_str(op),
                style(op.path().display()).cyan()
            );
            io::stdout().flush().ok();

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().to_lowercase().starts_with('y') {
                approved.push(i);
            }
        }

        Ok(approved)
    }
}

fn short_preview(op: &FileOperation) -> String {
    match op {
        FileOperation::Create { path, language, .. } => {
            let lang = language.as_deref().unwrap_or("file");
            format!(
                "{} {} ({})",
                style("CREATE").green(),
                style(path.display()).cyan(),
                lang
            )
        }
        FileOperation::Edit { path, .. } => {
            format!(
                "{} {}",
                style("EDIT").yellow(),
                style(path.display()).cyan()
            )
        }
        FileOperation::Delete { path, .. } => {
            format!("{} {}", style("DELETE").red(), style(path.display()).cyan())
        }
        FileOperation::Rename { from, to } => {
            format!(
                "{} {} → {}",
                style("RENAME").magenta(),
                style(from.display()).cyan(),
                style(to.display()).cyan()
            )
        }
    }
}

fn operation_type_str(op: &FileOperation) -> &'static str {
    match op {
        FileOperation::Create { .. } => "create",
        FileOperation::Edit { .. } => "edit",
        FileOperation::Delete { .. } => "delete",
        FileOperation::Rename { .. } => "rename",
    }
}

/// Execute approved file operations with progress reporting
pub fn execute_operations(
    operations: &[FileOperation],
    approved: &[usize],
    project_root: &Path,
) -> Result<(usize, usize)> {
    let mut success = 0;
    let mut failed = 0;

    for &idx in approved {
        let op = &operations[idx];
        match op.execute(project_root) {
            Ok(()) => {
                println!(
                    "  {} {} {}",
                    style("✓").green(),
                    operation_type_str(op),
                    op.path().display()
                );
                success += 1;
            }
            Err(e) => {
                println!(
                    "  {} {} {} - {}",
                    style("✗").red(),
                    operation_type_str(op),
                    op.path().display(),
                    e
                );
                failed += 1;
            }
        }
    }

    Ok((success, failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_code_block_header() {
        // Test lang:path format
        let (lang, path) = parse_code_block_header("rust:src/main.rs");
        assert_eq!(lang, Some("rust".to_string()));
        assert_eq!(path, Some(PathBuf::from("src/main.rs")));

        // Test lang path=filepath format
        let (lang, path) = parse_code_block_header("python path=src/script.py");
        assert_eq!(lang, Some("python".to_string()));
        assert_eq!(path, Some(PathBuf::from("src/script.py")));

        // Test just language
        let (lang, path) = parse_code_block_header("javascript");
        assert_eq!(lang, Some("javascript".to_string()));
        assert_eq!(path, None);
    }

    #[test]
    fn test_parse_file_operations() {
        let text = r#"
Here's the updated code:

```rust:src/main.rs
fn main() {
    println!("Hello, world!");
}
```

And another file:

```python path=scripts/test.py
print("test")
```
"#;
        let ops = parse_file_operations(text, Path::new("."));
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn test_parse_delete_marker() {
        // Test DELETE:path format
        let path = parse_delete_marker("DELETE:src/old_file.rs");
        assert_eq!(path, Some(PathBuf::from("src/old_file.rs")));

        // Test DELETE: path format (with space)
        let path = parse_delete_marker("DELETE: temp/cache.json");
        assert_eq!(path, Some(PathBuf::from("temp/cache.json")));

        // Test [DELETE] format
        let path = parse_delete_marker("[DELETE] unused.txt");
        assert_eq!(path, Some(PathBuf::from("unused.txt")));

        // Test **DELETE:** format (markdown bold)
        let path = parse_delete_marker("**DELETE:** old.md");
        assert_eq!(path, Some(PathBuf::from("old.md")));

        // Test non-delete lines
        let path = parse_delete_marker("This is just text");
        assert_eq!(path, None);

        let path = parse_delete_marker("delete this file");
        assert_eq!(path, None);
    }
}
