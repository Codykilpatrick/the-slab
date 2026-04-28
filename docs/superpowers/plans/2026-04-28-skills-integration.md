# Skills Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a session-persistent skill system to slab that auto-loads Markdown skills from `.slab/skills/` using a hybrid keyword + LLM confirmation matcher, matching Claude Code's skill file format.

**Architecture:** Skills are Markdown files with YAML frontmatter loaded into a `SkillManager`. Before each user message, a keyword pre-filter screens inactive skills; if candidates exist, a short non-streaming LLM call confirms which to activate. Active skill content is injected into the system context alongside rules and persists for the session.

**Tech Stack:** Rust, `serde_yaml` (already dep), `serde_json` (already dep), `dirs-next` (already dep), existing `LlmBackend` trait.

---

### Task 1: Add `SkillsConfig` to `src/config.rs`

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/config.rs` at the bottom of the `#[cfg(test)]` block:

```rust
#[test]
fn test_skills_config_default() {
    let config = Config::default();
    assert!(config.skills.auto_load);
}
```

Run: `cargo test config::tests::test_skills_config_default 2>&1 | tail -5`
Expected: FAIL — `Config` has no `skills` field yet.

- [ ] **Step 2: Add `SkillsConfig` struct and `default_true` helper**

In `src/config.rs`, after the existing `UiConfig` struct (around line 90), add:

```rust
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_true")]
    pub auto_load: bool,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self { auto_load: true }
    }
}
```

> Note: check whether `default_true` already exists in config.rs. If it does, skip adding it again.

- [ ] **Step 3: Add `skills` field to `Config` struct**

In the `Config` struct, after the `ui` field, add:

```rust
#[serde(default)]
pub skills: SkillsConfig,
```

- [ ] **Step 4: Add `skills.auto_load` to `set_config_value` in `src/main.rs`**

In the `set_config_value` match block, add a new arm before the `_` catch-all:

```rust
"skills.auto_load" => {
    config.skills.auto_load = value
        .parse()
        .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
}
```

- [ ] **Step 5: Add `skills.auto_load` to `show_config` in `src/main.rs`**

In `show_config`, after the `diff_style` line, add:

```rust
println!(
    "  {} {}",
    style("Skills auto-load:").dim(),
    config.skills.auto_load
);
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test config::tests::test_skills_config_default 2>&1 | tail -5`
Expected: `test config::tests::test_skills_config_default ... ok`

- [ ] **Step 7: Verify full build**

Run: `cargo build 2>&1 | tail -10`
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add SkillsConfig with auto_load field"
```

---

### Task 2: Add `skills` field to `ContextManager` in `src/context.rs`

**Files:**
- Modify: `src/context.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block in `src/context.rs`:

```rust
#[test]
fn test_skills_injected_in_system_content() {
    let mut ctx = ContextManager::new(4096, PathBuf::from("."));
    ctx.set_skills("## Active Skills\n\n### test-skill\n\nDo the thing.".to_string());
    let msgs = ctx.build_messages();
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].content.contains("## Active Skills"));
    assert!(msgs[0].content.contains("Do the thing."));
}

#[test]
fn test_skills_injected_after_rules() {
    let mut ctx = ContextManager::new(4096, PathBuf::from("."));
    ctx.set_rules("## Rules\n\nFollow the rules.".to_string());
    ctx.set_skills("## Active Skills\n\n### test-skill\n\nDo the thing.".to_string());
    let msgs = ctx.build_messages();
    let content = &msgs[0].content;
    let rules_pos = content.find("Follow the rules.").unwrap();
    let skills_pos = content.find("Do the thing.").unwrap();
    assert!(rules_pos < skills_pos);
}
```

Run: `cargo test context::tests::test_skills_injected 2>&1 | tail -10`
Expected: FAIL — `set_skills` does not exist.

- [ ] **Step 2: Add `skills` field to `ContextManager` struct**

In `src/context.rs`, in the `ContextManager` struct, after the `rules` field:

```rust
/// Active skills content (injected after rules)
skills: Option<String>,
```

- [ ] **Step 3: Initialize `skills` to `None` in `ContextManager::new`**

In the `Self { ... }` block inside `ContextManager::new`, add:

```rust
skills: None,
```

- [ ] **Step 4: Add `set_skills` and `clear_skills` methods**

After the existing `clear_rules` method, add:

```rust
/// Set the active skills content
pub fn set_skills(&mut self, skills: impl Into<String>) {
    self.skills = Some(skills.into());
}

/// Clear skills from context
pub fn clear_skills(&mut self) {
    self.skills = None;
}
```

- [ ] **Step 5: Inject skills in `build_system_content`**

In `build_system_content`, after the rules block and before the files block:

```rust
// Active skills
if let Some(skills) = &self.skills {
    parts.push(skills.clone());
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test context::tests::test_skills_injected 2>&1 | tail -10`
Expected: both tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/context.rs
git commit -m "feat: add skills injection layer to ContextManager"
```

---

### Task 3: Create `src/skills.rs` — data model and file loading

**Files:**
- Create: `src/skills.rs`
- Modify: `src/main.rs` (add `mod skills;`)

- [ ] **Step 1: Write failing tests in a new file**

Create `src/skills.rs` with only the test module first:

```rust
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

pub struct SkillManager {
    available: HashMap<String, Skill>,
    active: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir; // note: no tempfile dep — use std::env::temp_dir instead

    fn write_skill(dir: &Path, name: &str, description: &str, content: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_md = format!(
            "---\nname: {}\ndescription: \"{}\"\n---\n\n{}",
            name, description, content
        );
        fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();
    }

    #[test]
    fn test_load_skill_from_directory() {
        let tmp = std::env::temp_dir().join(format!("slab-test-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        write_skill(&tmp, "debugging", "Use when fixing bugs", "# Debug\n\nFollow these steps.");

        let mut mgr = SkillManager::new();
        mgr.load_from_directory(&tmp);

        assert_eq!(mgr.available_count(), 1);
        let skill = mgr.get_available("debugging").unwrap();
        assert_eq!(skill.name, "debugging");
        assert_eq!(skill.description, "Use when fixing bugs");
        assert!(skill.content.contains("Follow these steps."));

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_missing_skill_md_skipped() {
        let tmp = std::env::temp_dir().join(format!("slab-test2-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        // Create directory without SKILL.md
        fs::create_dir_all(tmp.join("empty-skill")).unwrap();

        let mut mgr = SkillManager::new();
        mgr.load_from_directory(&tmp);
        assert_eq!(mgr.available_count(), 0);

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_project_skill_overrides_global() {
        let global = std::env::temp_dir().join(format!("slab-global-{}", std::process::id()));
        let project = std::env::temp_dir().join(format!("slab-project-{}", std::process::id()));
        fs::create_dir_all(&global).unwrap();
        fs::create_dir_all(&project).unwrap();
        write_skill(&global, "review", "global description", "global content");
        write_skill(&project, "review", "project description", "project content");

        let mut mgr = SkillManager::new();
        mgr.load_from_directories(&[global.clone(), project.clone()]);

        let skill = mgr.get_available("review").unwrap();
        assert_eq!(skill.description, "project description");

        fs::remove_dir_all(&global).ok();
        fs::remove_dir_all(&project).ok();
    }
}
```

Add `mod skills;` to `src/main.rs` after the other mod declarations.

Run: `cargo test skills::tests 2>&1 | tail -15`
Expected: FAIL — `SkillManager::new`, `load_from_directory`, `get_available`, `available_count` not implemented.

- [ ] **Step 2: Implement `SkillManager::new` and file loading**

Replace the stub `SkillManager` struct definition in `src/skills.rs` with the full implementation:

```rust
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

impl SkillManager {
    pub fn new() -> Self {
        Self {
            available: HashMap::new(),
            active: Vec::new(),
        }
    }

    pub fn load_from_directories(&mut self, dirs: &[PathBuf]) {
        for dir in dirs {
            if dir.exists() && dir.is_dir() {
                self.load_from_directory(dir);
            }
        }
    }

    pub fn load_from_directory(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let skill_path = entry.path().join("SKILL.md");
            if skill_path.exists() {
                match Self::parse_skill_file(&skill_path) {
                    Ok(skill) => { self.available.insert(skill.name.clone(), skill); }
                    Err(e) => eprintln!("Warning: Failed to load skill {:?}: {}", skill_path, e),
                }
            }
        }
    }

    fn parse_skill_file(path: &Path) -> Result<Skill, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read skill file: {}", e))?;

        let after_start = content.strip_prefix("---\n")
            .or_else(|| content.strip_prefix("---"))
            .ok_or("Missing opening --- in frontmatter")?;
        let end_idx = after_start.find("\n---")
            .ok_or("Missing closing --- in frontmatter")?;

        let frontmatter = &after_start[..end_idx];
        let body = after_start[end_idx + 4..].trim().to_string();

        let meta: SkillFrontmatter = serde_yaml::from_str(frontmatter)
            .map_err(|e| format!("Failed to parse skill frontmatter: {}", e))?;

        Ok(Skill {
            name: meta.name,
            description: meta.description,
            content: body,
        })
    }

    pub fn available_count(&self) -> usize {
        self.available.len()
    }

    pub fn get_available(&self, name: &str) -> Option<&Skill> {
        self.available.get(name)
    }

    pub fn list_available(&self) -> Vec<&Skill> {
        let mut skills: Vec<&Skill> = self.available.values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test skills::tests::test_load_skill_from_directory skills::tests::test_missing_skill_md_skipped skills::tests::test_project_skill_overrides_global 2>&1 | tail -10`
Expected: all 3 pass.

- [ ] **Step 4: Commit**

```bash
git add src/skills.rs src/main.rs
git commit -m "feat: add SkillManager with file loading"
```

---

### Task 4: Active skill management and context building in `src/skills.rs`

**Files:**
- Modify: `src/skills.rs`

- [ ] **Step 1: Write failing tests**

Add to the `tests` module in `src/skills.rs`:

```rust
#[test]
fn test_activate_skill() {
    let tmp = std::env::temp_dir().join(format!("slab-activate-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    write_skill(&tmp, "debugging", "Fix bugs", "Step 1: reproduce.");

    let mut mgr = SkillManager::new();
    mgr.load_from_directory(&tmp);

    assert!(!mgr.is_active("debugging"));
    assert!(mgr.activate("debugging"));
    assert!(mgr.is_active("debugging"));
    // Activating again returns false (already active)
    assert!(!mgr.activate("debugging"));
    // Unknown skill returns false
    assert!(!mgr.activate("nonexistent"));

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_deactivate_skill() {
    let tmp = std::env::temp_dir().join(format!("slab-deact-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    write_skill(&tmp, "debugging", "Fix bugs", "Step 1: reproduce.");

    let mut mgr = SkillManager::new();
    mgr.load_from_directory(&tmp);
    mgr.activate("debugging");
    assert!(mgr.deactivate("debugging"));
    assert!(!mgr.is_active("debugging"));
    // Deactivating unknown returns false
    assert!(!mgr.deactivate("nonexistent"));

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_build_skills_context_empty() {
    let mgr = SkillManager::new();
    assert!(mgr.build_skills_context().is_none());
}

#[test]
fn test_build_skills_context_with_active() {
    let tmp = std::env::temp_dir().join(format!("slab-ctx-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    write_skill(&tmp, "debugging", "Fix bugs", "Step 1: reproduce.");

    let mut mgr = SkillManager::new();
    mgr.load_from_directory(&tmp);
    mgr.activate("debugging");

    let ctx = mgr.build_skills_context().unwrap();
    assert!(ctx.contains("## Active Skills"));
    assert!(ctx.contains("### debugging"));
    assert!(ctx.contains("Step 1: reproduce."));

    fs::remove_dir_all(&tmp).ok();
}
```

Run: `cargo test skills::tests::test_activate skills::tests::test_deactivate skills::tests::test_build_skills 2>&1 | tail -15`
Expected: FAIL.

- [ ] **Step 2: Implement activate, deactivate, is_active, active_names, build_skills_context**

Add to the `impl SkillManager` block in `src/skills.rs`:

```rust
pub fn activate(&mut self, name: &str) -> bool {
    if self.available.contains_key(name) && !self.active.contains(&name.to_string()) {
        self.active.push(name.to_string());
        true
    } else {
        false
    }
}

pub fn deactivate(&mut self, name: &str) -> bool {
    if let Some(pos) = self.active.iter().position(|n| n == name) {
        self.active.remove(pos);
        true
    } else {
        false
    }
}

pub fn is_active(&self, name: &str) -> bool {
    self.active.contains(&name.to_string())
}

pub fn active_names(&self) -> &[String] {
    &self.active
}

pub fn build_skills_context(&self) -> Option<String> {
    if self.active.is_empty() {
        return None;
    }
    let mut sections = vec!["## Active Skills".to_string()];
    for name in &self.active {
        if let Some(skill) = self.available.get(name) {
            sections.push(format!("### {}\n\n{}", skill.name, skill.content));
        }
    }
    Some(sections.join("\n\n"))
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test skills::tests 2>&1 | tail -15`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/skills.rs
git commit -m "feat: add skill activation and context building"
```

---

### Task 5: Keyword pre-filter in `src/skills.rs`

**Files:**
- Modify: `src/skills.rs`

- [ ] **Step 1: Write failing tests**

Add to the `tests` module in `src/skills.rs`:

```rust
#[test]
fn test_keyword_candidates_match() {
    let tmp = std::env::temp_dir().join(format!("slab-kw-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    write_skill(&tmp, "systematic-debugging", "Use when encountering bugs or test failures", "debug content");
    write_skill(&tmp, "code-review", "Review code for correctness and style", "review content");

    let mut mgr = SkillManager::new();
    mgr.load_from_directory(&tmp);

    let candidates = mgr.keyword_candidates("I have a bug in my test");
    let names: Vec<&str> = candidates.iter().map(|(n, _)| *n).collect();
    assert!(names.contains(&"systematic-debugging"), "expected systematic-debugging in {:?}", names);

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_keyword_candidates_no_match() {
    let tmp = std::env::temp_dir().join(format!("slab-kw2-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    write_skill(&tmp, "code-review", "Review code for correctness and style", "review content");

    let mut mgr = SkillManager::new();
    mgr.load_from_directory(&tmp);

    let candidates = mgr.keyword_candidates("what is the capital of France?");
    assert!(candidates.is_empty());

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_keyword_candidates_skips_active() {
    let tmp = std::env::temp_dir().join(format!("slab-kw3-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    write_skill(&tmp, "systematic-debugging", "Use when encountering bugs or test failures", "debug content");

    let mut mgr = SkillManager::new();
    mgr.load_from_directory(&tmp);
    mgr.activate("systematic-debugging");

    // Already active — should not appear in candidates
    let candidates = mgr.keyword_candidates("I have a bug in my test");
    assert!(candidates.is_empty());

    fs::remove_dir_all(&tmp).ok();
}
```

Run: `cargo test skills::tests::test_keyword 2>&1 | tail -15`
Expected: FAIL.

- [ ] **Step 2: Implement `tokenize` and `keyword_candidates`**

Add to `src/skills.rs` (outside the `impl` block, as a free function):

```rust
fn tokenize(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}
```

Add to the `impl SkillManager` block:

```rust
/// Returns (name, description) pairs for inactive skills that share tokens with the prompt.
pub fn keyword_candidates<'a>(&'a self, prompt: &str) -> Vec<(&'a str, &'a str)> {
    let prompt_tokens = tokenize(prompt);
    self.available
        .iter()
        .filter(|(name, skill)| {
            if self.active.contains(name) {
                return false;
            }
            let skill_tokens = tokenize(&format!("{} {}", skill.name, skill.description));
            !prompt_tokens.is_disjoint(&skill_tokens)
        })
        .map(|(name, skill)| (name.as_str(), skill.description.as_str()))
        .collect()
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test skills::tests 2>&1 | tail -15`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/skills.rs
git commit -m "feat: add keyword pre-filter for skill candidates"
```

---

### Task 6: Wire `SkillManager` into `Repl` — startup, `/skills`, `/skill` commands

**Files:**
- Modify: `src/repl.rs`

- [ ] **Step 1: Add `skills` field to `Repl` struct**

In `src/repl.rs`, at the top of the file add the import:

```rust
use crate::skills::SkillManager;
```

In the `Repl<B>` struct definition, after the `rules` field, add:

```rust
skills: SkillManager,
```

- [ ] **Step 2: Add `get_skill_directories` helper function**

At the bottom of `src/repl.rs`, after `get_template_directories`, add:

```rust
fn get_skill_directories(project_root: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // Global user skills (loaded first — project overrides on name collision)
    if let Some(config_dir) = dirs_next::config_dir() {
        dirs.push(config_dir.join("slab/skills"));
    }
    // Project-level skills
    dirs.push(project_root.join(".slab/skills"));
    dirs
}
```

- [ ] **Step 3: Load skills in `Repl::new`**

In `Repl::new`, after the rules loading block, add:

```rust
// Load skills
let mut skills = SkillManager::new();
let skill_dirs = get_skill_directories(&project_root);
skills.load_from_directories(&skill_dirs);
```

Add `skills` to the `Self { ... }` struct initializer after `rules`.

- [ ] **Step 4: Build and verify it compiles**

Run: `cargo build 2>&1 | tail -10`
Expected: no errors.

- [ ] **Step 5: Add `/skills` command to `handle_command`**

In `handle_command`, in the big `match cmd` block, after the `"rules"` arm, add:

```rust
"skills" => {
    let available = self.skills.list_available();
    if available.is_empty() {
        println!("{}", style("No skills loaded.").dim());
        println!(
            "{}",
            style("Add skills to .slab/skills/<name>/SKILL.md").dim()
        );
    } else {
        println!("{}", style("Available skills:").cyan().bold());
        for skill in available {
            let active_marker = if self.skills.is_active(&skill.name) {
                style(" [active]").green().to_string()
            } else {
                String::new()
            };
            println!(
                "  {}{}  {}",
                style(&skill.name).green(),
                active_marker,
                style(&skill.description).dim()
            );
        }
    }
    Ok(true)
}
```

- [ ] **Step 6: Add `/skill add|remove` command to `handle_command`**

After the `"skills"` arm, add:

```rust
"skill" => {
    if parts.len() < 3 {
        println!("{} /skill add|remove <name>", style("Usage:").dim());
        return Ok(true);
    }
    let action = parts[1];
    let name = parts[2..].join(" ");
    match action {
        "add" => {
            if self.skills.activate(&name) {
                println!(
                    "{} Loaded skill: {}",
                    style("↑").cyan(),
                    style(&name).cyan()
                );
                self.update_skills_for_context();
            } else if self.skills.is_active(&name) {
                println!("{} Skill already active: {}", style("⚠").yellow(), style(&name).cyan());
            } else {
                println!("{} Skill not found: {}", style("✗").red(), style(&name).cyan());
            }
        }
        "remove" => {
            if self.skills.deactivate(&name) {
                println!(
                    "{} Removed skill: {}",
                    style("✓").green(),
                    style(&name).cyan()
                );
                self.update_skills_for_context();
            } else {
                println!("{} Skill not active: {}", style("✗").red(), style(&name).cyan());
            }
        }
        _ => {
            println!("{} /skill add|remove <name>", style("Usage:").dim());
        }
    }
    Ok(true)
}
```

- [ ] **Step 7: Add `update_skills_for_context` helper method**

After the existing `update_rules_for_context` method, add:

```rust
fn update_skills_for_context(&mut self) {
    if let Some(skills_content) = self.skills.build_skills_context() {
        self.context.set_skills(skills_content);
    } else {
        self.context.clear_skills();
    }
}
```

- [ ] **Step 8: Add skills commands to `/help` listing**

In `handle_help_command`, in the `print_help` section where REPL commands are listed (the static commands array around line 1685), add:

```rust
("/skills", "List available skills"),
("/skill add|remove <name>", "Activate or deactivate a skill"),
```

- [ ] **Step 9: Add help detail for `/help skills` and `/help skill`**

In the `help_for_command` match, add these arms:

```rust
"skills" => (
    "/skills",
    "List available skills",
    "Shows all discovered skills from .slab/skills/ and ~/.config/slab/skills/. \
     Skills marked [active] are currently injected into context.\n\n\
     Example:\n  /skills",
),
"skill" => (
    "/skill add|remove <name>",
    "Activate or deactivate a skill",
    "Manually activate a skill by name, or remove an active one.\n\n\
     Examples:\n  /skill add systematic-debugging\n  /skill remove systematic-debugging",
),
```

- [ ] **Step 10: Add `SkillCompleter` to `src/completion.rs`**

Add a `CompletionKind::Skill` variant to the `CompletionKind` enum:

```rust
Skill,
```

Add its icon to the `CompletionKind::icon` match:

```rust
CompletionKind::Skill => "🔧",
```

Add the completer struct at the bottom of `src/completion.rs`, before the `#[cfg(test)]` block:

```rust
/// Completes skill names for `/skill add <name>` and `/skill remove <name>`
pub struct SkillCompleter {
    skill_names: Vec<String>,
}

impl SkillCompleter {
    pub fn new(names: Vec<String>) -> Self {
        Self { skill_names: names }
    }
}

impl Completer for SkillCompleter {
    fn complete(&self, input: &str, _context: &CompletionContext) -> Vec<Completion> {
        // input is everything after "/skill " — e.g. "add deb" or "remove sys"
        let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
        let (subcommand, partial) = match parts.as_slice() {
            [sub, partial] => (*sub, *partial),
            [sub] if *sub == "add" || *sub == "remove" => (*sub, ""),
            _ => return Vec::new(),
        };
        if subcommand != "add" && subcommand != "remove" {
            return Vec::new();
        }
        self.skill_names
            .iter()
            .filter(|n| n.starts_with(partial))
            .map(|n| {
                Completion::new(format!("{} {}", subcommand, n), CompletionKind::Skill)
                    .with_description(format!("skill: {}", n))
            })
            .collect()
    }

    fn name(&self) -> &'static str {
        "skill"
    }
}
```

- [ ] **Step 11: Register `SkillCompleter` in `Repl::new`**

In `src/repl.rs`, in `Repl::new`, after `completion_engine.add_template_commands(template_cmds)`, add:

```rust
let skill_names: Vec<String> = skills
    .list_available()
    .iter()
    .map(|s| s.name.clone())
    .collect();
completion_engine.register("skill", Box::new(crate::completion::SkillCompleter::new(skill_names)));
```

- [ ] **Step 12: Build and verify**

Run: `cargo build 2>&1 | tail -10`
Expected: no errors.

- [ ] **Step 13: Commit**

```bash
git add src/repl.rs src/completion.rs
git commit -m "feat: wire SkillManager into Repl with /skills and /skill commands"
```

---

### Task 7: Hybrid matcher in `send_message`

**Files:**
- Modify: `src/repl.rs`

- [ ] **Step 1: Add `extract_json_array` helper function**

At the bottom of `src/repl.rs`, add:

```rust
/// Extract the first JSON array from a string (handles LLM responses with surrounding text).
fn extract_json_array(text: &str) -> String {
    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if start <= end {
            return text[start..=end].to_string();
        }
    }
    "[]".to_string()
}
```

- [ ] **Step 2: Add `auto_load_skills` async method to `Repl`**

Add this method to `impl<B: LlmBackend> Repl<B>`:

```rust
async fn auto_load_skills(&mut self, prompt: &str) -> Vec<String> {
    // Step 1: keyword pre-filter
    let candidates = self.skills.keyword_candidates(prompt);
    if candidates.is_empty() {
        return Vec::new();
    }

    // Step 2: LLM confirmation
    let candidates_text = candidates
        .iter()
        .map(|(name, desc)| format!("- \"{}\": \"{}\"", name, desc))
        .collect::<Vec<_>>()
        .join("\n");

    let confirmation_prompt = format!(
        "You are a skill router. Based on the user's message, decide which skills are relevant.\n\n\
         User message: \"{}\"\n\n\
         Available skills:\n{}\n\n\
         Reply with ONLY a JSON array of relevant skill names. Return [] if none apply.\n\
         Example output: [\"skill-name\"] or []",
        prompt, candidates_text
    );

    let request = ChatRequest {
        model: self.model.clone(),
        messages: vec![Message::user(&confirmation_prompt)],
        stream: Some(false),
        options: Some(ModelOptions {
            temperature: Some(0.0),
            top_p: None,
            num_ctx: Some(512),
        }),
    };

    let response = match self.client.llm_chat(request).await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let json_str = extract_json_array(&response);
    let names: Vec<String> = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut newly_loaded = Vec::new();
    for name in &names {
        if self.skills.activate(name) {
            newly_loaded.push(name.clone());
        }
    }
    newly_loaded
}
```

- [ ] **Step 3: Hook `auto_load_skills` into `send_message`**

In the `send_message` method, after the watch-mode refresh block and before the `expand_file_references` call, add:

```rust
// Hybrid skill auto-loader
if self.config.skills.auto_load {
    let newly_loaded = self.auto_load_skills(content).await;
    if !newly_loaded.is_empty() {
        println!(
            "{}",
            style(format!(
                "↑ Loaded skill{}: {}",
                if newly_loaded.len() > 1 { "s" } else { "" },
                newly_loaded.join(", ")
            ))
            .dim()
        );
        self.update_skills_for_context();
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1 | tail -10`
Expected: no errors.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/repl.rs
git commit -m "feat: add hybrid skill auto-loader in send_message"
```

---

### Task 8: Seed skills in `slab init` and add example skill

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write the seed skill content and `create_dir_all` call**

In `init_project` in `src/main.rs`, after the `std::fs::create_dir_all(".slab/sessions")?;` line, add:

```rust
std::fs::create_dir_all(".slab/skills/code-review")?;
```

- [ ] **Step 2: Write the example skill file**

After creating the directory, write the skill file:

```rust
let code_review_skill = r#"---
name: code-review
description: "Use when reviewing code for bugs, style issues, or correctness"
---

# Code Review

When asked to review code, apply these guidelines systematically.

## What to Check

1. **Correctness** — Does the logic match the stated intent? Are there off-by-one errors, unchecked return values, or incorrect assumptions?
2. **Safety** — For C code: NULL checks, bounds checks, integer overflow, uninitialized variables. For Rust: no `unwrap()` in production paths.
3. **Style** — Follow the rules in `.slab/rules/` for the relevant language.
4. **Dead code** — Flag variables computed but never read, unreachable branches, or unused imports.

## Output Format

For each issue found, report:
- **File and line** where the issue occurs
- **Severity**: build-breaker, correctness bug, or nit
- **What's wrong** in one sentence
- **What to do** in one sentence

End with a summary line: `N build-breakers, N correctness bugs, N nits`.
"#;
std::fs::write(".slab/skills/code-review/SKILL.md", code_review_skill)?;
```

- [ ] **Step 3: Add the skill to the printed output**

In the `println!` block at the end of `init_project`, add the new line after the templates section:

```rust
println!("    .slab/skills/code-review/SKILL.md");
```

- [ ] **Step 4: Add `[skills]` section to the default config**

In `init_project`, after `config.save()?;`, no change needed — `SkillsConfig::default()` produces `auto_load = true` and serde will write it when serialized. Verify the TOML output includes the skills section by checking that `Config::default()` includes `skills`.

- [ ] **Step 5: Build and verify**

Run: `cargo build 2>&1 | tail -10`
Expected: no errors.

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass.

- [ ] **Step 7: Smoke test `slab init` output**

Run: `cargo run -- init 2>&1 | grep -E "skill|SKILL"` from a temp directory.
Expected: output includes `.slab/skills/code-review/SKILL.md`.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs
git commit -m "feat: seed .slab/skills/code-review/SKILL.md in slab init"
```

---

### Task 9: Bump version and update CHANGELOG

**Files:**
- Modify: `Cargo.toml`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Bump version in `Cargo.toml`**

Change line 4 from `version = "1.4.0"` to:

```toml
version = "1.5.0"
```

- [ ] **Step 2: Add CHANGELOG entry**

In `CHANGELOG.md`, add a new entry under `## [Unreleased]`:

```markdown
## [1.5.0] - 2026-04-28

### Added

- **Skills system** — A session-persistent skill system matching Claude Code's skill file format. Skills are Markdown files with YAML frontmatter stored in `.slab/skills/<name>/SKILL.md`. Active skills are injected into LLM context alongside rules.
- **Hybrid skill auto-loader** — Before each message, a keyword pre-filter screens inactive skills against the prompt; if candidates exist, a short non-streaming LLM call confirms which to activate. Newly loaded skills are announced with `↑ Loaded skill: <name>`.
- **`/skills` command** — Lists all available skills with `[active]` markers on loaded ones.
- **`/skill add|remove <name>` commands** — Manually activate or deactivate skills.
- **`slab init` seeding** — Creates `.slab/skills/code-review/SKILL.md` as a format reference.
- **`skills.auto_load` config key** — Set to `false` to disable auto-loading; manual `/skill add` always works.
```

- [ ] **Step 3: Run final build and test**

Run: `cargo build && cargo test 2>&1 | tail -20`
Expected: build succeeds, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml CHANGELOG.md
git commit -m "chore: bump version to 1.5.0, update CHANGELOG"
```
