mod cli;
mod completion;
mod config;
mod context;
mod error;
mod file_ops;
mod highlight;
mod ollama;
mod repl;
mod rules;
mod session;
mod templates;
mod testing;
mod theme;
mod ui;

use clap::Parser;
use console::style;
use std::process;

use cli::{Cli, Commands};
use config::Config;
use error::{Result, SlabError};
use ollama::OllamaClient;
use repl::Repl;
use theme::{BoxStyle, ThemeName};
use ui::BoxRenderer;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        print_error(&e);
        process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = Config::load(cli.config.as_ref())?;

    // Create Ollama client
    let client = OllamaClient::new(&config.ollama_host);

    // Determine streaming mode
    let streaming = !cli.no_stream && config.ui.streaming;

    // Handle commands
    match cli.command_or_default() {
        Commands::Chat {
            r#continue,
            session: session_name,
            files,
            template,
        } => {
            // Health check first
            client.health_check().await?;

            // Get model (CLI override > config default > first available)
            let model = get_model(&cli, &config, &client).await?;

            // Load session if requested
            let session = if r#continue {
                match session::Session::load_last() {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("{} {}", style("Warning:").yellow(), e);
                        None
                    }
                }
            } else if let Some(name) = &session_name {
                match session::Session::load(name) {
                    Ok(s) => Some(s),
                    Err(_) => {
                        // Create new session with this name
                        Some(session::Session::new(name, &model))
                    }
                }
            } else {
                None
            };

            // Run REPL
            let mut repl = Repl::new(client, config, model.clone(), streaming);
            if let Some(s) = session {
                repl.load_session(s);
            }
            if !files.is_empty() {
                repl.add_files(&files);
            }

            // If a template is provided, send it as the first message
            if let Some(tpl_name) = &template {
                repl.send_template(tpl_name).await?;
            }

            repl.run().await?;

            // Auto-save session
            if let Some(name) = session_name {
                if let Err(e) = repl.save_session(&name) {
                    eprintln!("{} {}", style("Warning:").yellow(), e);
                }
            }
        }

        Commands::Run {
            prompt,
            files,
            template,
        } => {
            // Health check
            client.health_check().await?;

            let model = get_model(&cli, &config, &client).await?;
            repl::run_single_prompt(
                &client,
                &config,
                &model,
                &prompt,
                streaming,
                &files,
                template.as_deref(),
            )
            .await?;
        }

        Commands::Config { show: _, init, set } => {
            if init {
                init_config()?;
            } else if let Some(key_value) = set {
                set_config_value(&key_value)?;
            } else {
                show_config(&config)?;
            }
        }

        Commands::Models { names_only } => {
            // Health check
            client.health_check().await?;

            list_models(&client, names_only).await?;
        }

        Commands::Sessions { names_only } => {
            list_sessions(names_only)?;
        }

        Commands::Test { filter, model } => {
            // Health check
            client.health_check().await?;

            run_tests(&client, &config, &cli, filter.as_deref(), model.as_deref()).await?;
        }

        Commands::Init => {
            init_project(&client).await?;
        }

        Commands::Completions { shell } => {
            generate_completions(shell);
        }
    }

    Ok(())
}

fn generate_completions(shell: cli::Shell) {
    use clap::CommandFactory;
    use clap_complete::{generate, Shell as ClapShell};

    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();

    let shell = match shell {
        cli::Shell::Bash => ClapShell::Bash,
        cli::Shell::Zsh => ClapShell::Zsh,
        cli::Shell::Fish => ClapShell::Fish,
        cli::Shell::PowerShell => ClapShell::PowerShell,
    };

    generate(shell, &mut cmd, name, &mut std::io::stdout());
}

async fn get_model(cli: &Cli, config: &Config, client: &OllamaClient) -> Result<String> {
    // CLI override
    if let Some(model) = &cli.model {
        return Ok(model.clone());
    }

    // Config default
    if let Some(model) = &config.default_model {
        return Ok(model.clone());
    }

    // First available model
    let models = client.list_models().await?;
    if let Some(first) = models.first() {
        Ok(first.name.clone())
    } else {
        Err(SlabError::NoModelsAvailable)
    }
}

fn show_config(config: &Config) -> Result<()> {
    println!("{}", style("Current Configuration:").cyan().bold());
    println!();
    println!("  {} {}", style("Ollama host:").dim(), config.ollama_host);
    println!(
        "  {} {}",
        style("Default model:").dim(),
        config.default_model.as_deref().unwrap_or("(auto)")
    );
    println!(
        "  {} {}",
        style("Context limit:").dim(),
        config.context_limit
    );
    println!("  {} {}", style("Streaming:").dim(), config.ui.streaming);
    println!(
        "  {} {}",
        style("Auto-apply file ops:").dim(),
        config.ui.auto_apply_file_ops
    );
    println!(
        "  {} {}",
        style("Inline completion:").dim(),
        config.ui.inline_completion_preview
    );
    println!(
        "  {} {}",
        style("Fuzzy completion:").dim(),
        config.ui.fuzzy_completion
    );
    println!(
        "  {} {}",
        style("Max completions:").dim(),
        config.ui.max_completion_items
    );
    println!("  {} {}", style("Theme:").dim(), config.ui.theme);
    println!("  {} {}", style("Box style:").dim(), config.ui.box_style);
    println!(
        "  {} {}",
        style("Show status bar:").dim(),
        config.ui.show_status_bar
    );
    println!(
        "  {} {}",
        style("Show banner:").dim(),
        config.ui.show_banner
    );
    println!(
        "  {} {}",
        style("Code block style:").dim(),
        config.ui.code_block_style
    );
    println!("  {} {}", style("Diff style:").dim(), config.ui.diff_style);

    if !config.models.is_empty() {
        println!();
        println!("{}", style("Model configs:").cyan().bold());
        for (key, model) in &config.models {
            println!("  {}", style(key).yellow());
            println!("    name: {}", model.name);
            println!("    temperature: {}", model.temperature);
            println!("    top_p: {}", model.top_p);
            if let Some(sp) = &model.system_prompt {
                let preview: String = sp.chars().take(40).collect();
                println!("    system_prompt: {}...", preview);
            }
        }
    }

    println!();
    println!("{}", style("Config file locations:").dim());
    println!("  Project: .slab/config.toml");
    if let Some(global) = Config::global_config_path() {
        println!("  Global:  {}", global.display());
    }

    Ok(())
}

fn init_config() -> Result<()> {
    let config = Config::default();
    config.save()?;
    println!("{} Created .slab/config.toml", style("✓").green());
    Ok(())
}

fn set_config_value(key_value: &str) -> Result<()> {
    let parts: Vec<&str> = key_value.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(SlabError::ConfigError(
            "Invalid format. Use: --set key=value".to_string(),
        ));
    }

    let mut config = Config::load(None)?;
    let key = parts[0];
    let value = parts[1];

    match key {
        "ollama_host" => config.ollama_host = value.to_string(),
        "default_model" => config.default_model = Some(value.to_string()),
        "context_limit" => {
            config.context_limit = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid context_limit value".to_string()))?;
        }
        "ui.streaming" => {
            config.ui.streaming = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
        }
        "ui.auto_apply_file_ops" => {
            config.ui.auto_apply_file_ops = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
        }
        "ui.inline_completion_preview" => {
            config.ui.inline_completion_preview = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
        }
        "ui.fuzzy_completion" => {
            config.ui.fuzzy_completion = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
        }
        "ui.max_completion_items" => {
            config.ui.max_completion_items = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid number value".to_string()))?;
        }
        "ui.theme" => {
            config.ui.theme = value.to_string();
        }
        "ui.box_style" => {
            config.ui.box_style = value.to_string();
        }
        "ui.show_status_bar" => {
            config.ui.show_status_bar = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
        }
        "ui.show_banner" => {
            config.ui.show_banner = value
                .parse()
                .map_err(|_| SlabError::ConfigError("Invalid boolean value".to_string()))?;
        }
        "ui.code_block_style" => {
            config.ui.code_block_style = value.to_string();
        }
        "ui.diff_style" => {
            config.ui.diff_style = value.to_string();
        }
        _ => {
            return Err(SlabError::ConfigError(format!(
                "Unknown config key: {}",
                key
            )));
        }
    }

    config.save()?;
    println!("{} Set {} = {}", style("✓").green(), key, value);
    Ok(())
}

async fn list_models(client: &OllamaClient, names_only: bool) -> Result<()> {
    let models = client.list_models().await?;

    if models.is_empty() {
        if !names_only {
            println!(
                "{}",
                style("No models available. Pull a model with: ollama pull <model>").yellow()
            );
        }
        return Ok(());
    }

    // For shell completion scripts - just output names
    if names_only {
        for model in &models {
            println!("{}", model.name);
        }
        return Ok(());
    }

    println!("{}", style("Available models:").cyan().bold());
    println!();

    for model in &models {
        let size_mb = model.size / 1_000_000;
        let size_str = if size_mb > 1000 {
            format!("{:.1} GB", size_mb as f64 / 1000.0)
        } else {
            format!("{} MB", size_mb)
        };

        print!("  {}", style(&model.name).green());

        if let Some(details) = &model.details {
            if let Some(params) = &details.parameter_size {
                print!(" {}", style(format!("({})", params)).dim());
            }
        }

        println!(" {}", style(size_str).dim());
    }

    println!();
    Ok(())
}

fn list_sessions(names_only: bool) -> Result<()> {
    let sessions_dir = config::find_project_root()
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
        .join(".slab/sessions");

    if !sessions_dir.exists() {
        if !names_only {
            println!("{}", style("No sessions directory found.").yellow());
        }
        return Ok(());
    }

    let mut sessions: Vec<String> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    sessions.push(stem.to_string_lossy().to_string());
                }
            }
        }
    }

    sessions.sort();

    if sessions.is_empty() {
        if !names_only {
            println!("{}", style("No saved sessions found.").yellow());
            println!(
                "{}",
                style("Start a session with: slab chat --session <name>").dim()
            );
        }
        return Ok(());
    }

    // For shell completion scripts - just output names
    if names_only {
        for session in &sessions {
            println!("{}", session);
        }
        return Ok(());
    }

    println!("{}", style("Saved sessions:").cyan().bold());
    println!();

    for session in &sessions {
        println!("  {}", style(session).green());
    }

    println!();
    Ok(())
}

async fn init_project(client: &OllamaClient) -> Result<()> {
    println!("{}", style("Initializing The Slab...").cyan().bold());
    println!();

    // Check if Ollama is running and detect models
    let detected_model = match client.health_check().await {
        Ok(()) => {
            println!("{} Ollama is running", style("✓").green());
            match client.list_models().await {
                Ok(models) if !models.is_empty() => {
                    println!("{} Found {} model(s):", style("✓").green(), models.len());
                    for model in &models {
                        let size_mb = model.size / 1_000_000;
                        let size_str = if size_mb > 1000 {
                            format!("{:.1} GB", size_mb as f64 / 1000.0)
                        } else {
                            format!("{} MB", size_mb)
                        };
                        println!(
                            "    {} {}",
                            style(&model.name).yellow(),
                            style(size_str).dim()
                        );
                    }
                    // Use the first model as default
                    Some(models[0].name.clone())
                }
                _ => {
                    println!(
                        "{} No models found. Pull one with: {}",
                        style("⚠").yellow(),
                        style("ollama pull qwen2.5:7b").cyan()
                    );
                    None
                }
            }
        }
        Err(_) => {
            println!(
                "{} Ollama not running. Start with: {}",
                style("⚠").yellow(),
                style("ollama serve").cyan()
            );
            None
        }
    };

    println!();

    // Create .slab directory structure
    std::fs::create_dir_all(".slab/templates")?;
    std::fs::create_dir_all(".slab/rules")?;
    std::fs::create_dir_all(".slab/tests")?;
    std::fs::create_dir_all(".slab/sessions")?;

    // Create config with detected model
    let mut config = Config::default();
    if let Some(model) = detected_model {
        config.default_model = Some(model);
    }
    config.save()?;

    // Create rules
    let rust_rules = r#"When translating C to Rust follow the following rules:

REQUIREMENTS:
1. NO `unsafe` blocks - the code must be 100% safe Rust
2. NO global/static mutable state - use structs with methods instead
3. Use `Result<T, E>` for all operations that can fail
4. Use `Option<T>` for nullable values
5. Use iterators instead of manual loops where possible
6. Follow Rust naming conventions (snake_case for functions/variables, CamelCase for types)
7. Add appropriate derive macros (Debug, Clone, etc.) where useful
8. The code must compile with `cargo build` without warnings
9.  NEVER use `OnceLock`, `Lazy`, `static mut`, `Rc`, `RefCell`, or any global/static storage. Encapsulate ALL state into owned structs with methods. If the C code has free-standing functions that operate on global state, convert them to methods on the struct - do NOT create wrapper functions with hidden global state. The caller is responsible for creating and owning the struct instance.
10. Do NOT include unused imports - only import what is actually used in the code.
11. Use `match` expressions for branching instead of if/else chains where possible.
12. Use iterators (`.iter()`, `.map()`, `.filter()`, `.collect()`) instead of manual loops where possible.
13. Keep code concise - avoid verbose getter/setter boilerplate when direct public field access or builder patterns would be more idiomatic.
14. NEVER negate unsigned integers directly. Cast to the signed type BEFORE negating: use `-(x as i32)` not `-(x)` where x is u32. For zigzag decoding use wrapping operations like `.wrapping_neg()` or explicit casts.
15. C `union` types must be translated to Rust `enum` variants with data, NOT Rust `union`. Avoid `#[repr(C, packed)]` - instead model the wire format with serialization/deserialization methods that read and write byte slices.
16. Never return `&mut T` references from methods that also require `&mut self` - this causes multiple mutable borrow errors. Instead return indices or owned handles, and provide separate methods to access elements by index.
17. C `#define` integer constants used as type tags/categories (e.g., `#define TYPE_FOO 1`) MUST become Rust enums with `Display` impl. Never use raw integers with magic numbers for classification.
18. Remove dead code from the C original. If a variable is computed but never read (like a running total that's never used), delete it — do NOT preserve it with an underscore prefix.
19. Choose Rust-native types. Don't mechanically map C `int` to `i32`. Use `usize` for counts, indices, and sizes. Use `f32`/`f64` for thresholds that are compared against floats. Use the type that makes casts unnecessary at usage sites.
20. Convert C output-parameter patterns (`void foo(float *out, int len)`) to Rust return values (`fn foo() -> Vec<f32>`). Functions should return owned data, not write to caller-provided buffers.
21. Methods that don't read or write `self` must not take `self`. Make them associated functions (`fn foo() -> T`, no self) or free functions. Never add `&mut self` just because a method lives in an `impl` block.
22. Do not carry over C `#define MAX_FOO` buffer sizes as hardcoded allocations. Size containers dynamically to actual input (e.g., `Vec::with_capacity(input.len())`). If a limit is needed, make it a parameter or config field.
23. Prefer struct literal initialization over `new()` + field-by-field mutation. Build the complete struct in one expression.
24. Implement the `Default` trait for any type that has a `new()` taking no arguments. Derive it when field defaults match Rust's defaults (0, false, None); implement manually otherwise.
25. Never use `.unwrap()` or `.expect()` in library code. Use `?` with proper error types, or validate inputs at function entry and return `Result`. Panicking conversions like `.try_into().unwrap()` indicate a wrong type choice (see rule 20).
26. Doc comments must accurately describe the Rust function signature. Do not copy C comments verbatim if the return type or parameters changed. If the C function returned `int` count and the Rust function returns `Vec<T>`, update the docs.
27. Write tests that exercise core logic, not just constructors. Include: a basic happy-path test with known input/output, at least one edge case (empty input, boundary values), and a test that verifies error/failure paths.
"#;
    std::fs::write(".slab/rules/rust.md", rust_rules)?;

    let citizen_rules = r##"# Citizen Style Guide

> "This code is what it is because of its citizens" — Plato

## Core Principle

Decrease net cognitive complexity for developers.

**Priority:** Readability > Writeability

---

## Writing Style

### Typography

- **Use contractions** – "can't" not "cannot"
- **Use curly quotes** – "hello" and 'world' not "hello" and 'world'
- **Use proper punctuation** – ellipsis (…), en dash (–), em dash (—)
- **Use glyphs** – ←, →, 45° instead of <-, ->, 45 degrees
- **Use canonical spelling** – Python, Node.js, PostgreSQL (not python, node, postgres)

### Comments & Documentation

- **Communicate intent** – explain why, not what
- **Show examples** when possible
- **Use sentence case** – capitalize first letter only
- **Punctuate block comments** – end with periods
- **Inline short comments** – `speed = …  # Meters per hour`

```ts
// Get a unit's globally unique identifier.
//
// ("Zumwalt-class destroyer", 3) → "Unit:ZumwaltClassDestroyer:3"
const getUnitId = (name: string, index: number) => `Unit:${pascalCase(name)}:${index}`;
```

### Documentation Patterns

- **Types:** Begin with indefinite article – "A unit."
- **Properties:** Begin with definite article – "The name of a unit."

---

## Naming Conventions

### General Rules

- **Follow language conventions** – camelCase (JS/TS), snake_case (Python), etc.
- **Favor readability over brevity** – `docker_image` not `image`
- **Group with prefixes** – `cnn_kernel`, `cnn_stride`
- **Space out affixes** – `hidden_layer_0` not `hidden_layer0`

### Abbreviations & Acronyms

- **DON'T abbreviate** – `directory` not `dir`, `response` not `res`
- **DO use well-known acronyms** – `nato_classification`, `radar_range`, `url`, `s3_bucket`
- **DON'T invent acronyms** – `aerial_unit_speed` not `au_speed`

### Type Clarity

- **Add type hints to names** – `created_date` not `created`, `was_published` not `published`
- **Differentiate types** – don't reuse variable names for different types
- **DON'T add type suffixes** – `age` not `age_int`, `confidence` not `confidence_float`

```py
# ✓ Good
url = "https://spear.ai"
parsed_url = urlparse("https://spear.ai")

# ✗ Bad
url = "https://spear.ai"
url = urlparse("https://spear.ai")
```

### Booleans

- **Use appropriate verbs** – `can_delete`, `has_feature`, `should_reset`
- **Be positive** – `is_enabled` not `is_disabled`
- **Use correct tense** – `was_suspended`, `is_suspended`

### Collections

- **DON'T pluralize** – specify collection type instead

```py
# ✓ Good
equipment_list = [{…}, {…}]
equipment = equipment_list[0]
id_set = {"a", "b", "c"}

# ✗ Bad
equipment = [{…}, {…}]
equipment = equipment[0]
ids = {"a", "b", "c"}
```

---

## Data Formats

### Angles

- **Prefer degrees to radians** – easier to reason about, serialize better
- **Display with symbol** – `f"heading {heading}°"`

```ts
// ✓ Good
const turnLeft = (angle: number) => (360 + (angle - 90)) % 360;
turnLeft(0); // ⇒ 270
```


```ts
// ✓ Good
const backgroundColor = "#edf6ff";
const foregroundColor = "#006adc";
```

---

## Quick Reference

| Do | Don't |
|---|---|
| `can_delete = True` | `is_deletable = True` |
| `equipment_list = […]` | `equipment = […]` |
| `docker_image = "…"` | `image = "…"` |
| `response = request()` | `res = req()` |
| `"It's a quote"` | `"It's a quote"` |
| `// 1, 2, 3, …, 10` | `// 1, 2, 3, ..., 10` |
| `// Duration: 2–3 weeks` | `// Duration: 2-3 weeks` |
| `created_date = "…"` | `created = "…"` |
| `is_enabled = True` | `is_disabled = False` |
"##;
    std::fs::write(".slab/rules/citizen.md", citizen_rules)?;

    // Create templates
    let review_template = r#"name: code_review
command: /review
description: Review translated Rust code against rust.md rules
prompt: |
  Review the Rust code in the current package against the rules in .slab/rules/rust.md.

  For each issue found:
  1. Reference the rule number violated (e.g., "Rule 18")
  2. Cite the file and line number
  3. Describe what's wrong and what the fix should be
  4. Rate severity: **build-breaker**, **idiom violation**, or **nit**

  After outputting the review to the terminal, save the full report by writing it as a fenced code block with the path, like: ```markdown:.slab/reviews/{{package}}.md

  Check specifically for:
  - Compilation errors (unsigned negation, type mismatches, unused imports)
  - C idioms that weren't converted (magic numbers, output params, dead code)
  - Type choices that cause unnecessary casts at usage sites
  - Missing trait impls (Default, Display)
  - .unwrap()/.expect() in non-test code
  - Tests that only cover constructors, not logic/edge cases/error paths

  Output a summary to the terminal in this structure:

  ```markdown
  # Review: {{package}}

  ## Summary
  - **Build**: pass/fail
  - **Rules violated**: [list rule numbers]
  - **Issues**: N build-breakers, N idiom violations, N nits

  ## Issues

  ### [severity] Rule N — short description
  **File:** `src/lib.rs:NN`
  **Problem:** what's wrong
  **Fix:** what to do

  (repeat for each issue)

  ## Checklist
  - [ ] All #define type constants are Rust enums
  - [ ] No dead code carried from C
  - [ ] usize for counts/indices/sizes
  - [ ] No output parameters
  - [ ] No unnecessary &mut self
  - [ ] No hardcoded buffer sizes
  - [ ] Structs built with literals
  - [ ] Default implemented where applicable
  - [ ] No .unwrap()/.expect() outside tests
  - [ ] Doc comments match Rust signatures
  - [ ] Tests cover logic, edge cases, and error paths
  - [ ] cargo clippy passes with zero warnings
  ```
"#;
    std::fs::write(".slab/templates/review.yaml", review_template)?;

    let c_to_rust_template = r#"name: c-to-rust
command: /c-to-rust
description: Translate C code to idiomatic Rust
prompt: |
  Translate the attached C code to idiomatic Rust.

  Requirements:
  - Create actual .rs file(s), not just code blocks
  - If bindings.rs is attached, import FFI definitions rather than redefining them
  - Minimize unsafe blocks; document why each is necessary
  - Use Result<T, E> for error handling (map errno/return codes appropriately)
  - Prefer standard library types over raw pointers where possible
  - Add // SAFETY: comments for all unsafe code
  - C #define type constants (TYPE_FOO = 1, etc.) must become Rust enums — never raw integers
  - Convert C output-parameter patterns to Rust return values
  - Remove dead code from the C original; do not preserve unused computations
  - Choose Rust-native types (usize for counts/indices, matching float types for thresholds)
  - Never negate unsigned types directly — cast to isize/i32 BEFORE negating
  - All code must pass `cargo clippy` with zero warnings
"#;
    std::fs::write(".slab/templates/c-to-rust.yaml", c_to_rust_template)?;

    let c_improve_template = r#"name: c-improve
command: /c-improve
description: Refactor C code to safer, more robust C following MISRA/CERT standards
prompt: |
  Refactor the attached C code into safer, more robust C.

  Requirements:
  1. Use fixed-width types (stdint.h) - uint32_t, int64_t, size_t, etc.
  2. Add explicit error handling with enum return codes
  3. Remove all static/global state - pass context explicitly
  4. Add const correctness for all read-only parameters
  5. Add restrict keywords for non-aliasing pointers
  6. Add NULL pointer checks at function entry
  7. Add bounds checking for array accesses
  8. Use bool from stdbool.h instead of int flags
  9. Add comprehensive function documentation
  10. Keep cyclomatic complexity < 10
  11. Maximum function length: 50 lines
  12. MAINTAIN PERFORMANCE - no heap allocations in hot paths

  Target standards: MISRA C:2012, CERT C

  IMPORTANT: Output the COMPLETE refactored file inside a fenced code block
  annotated with the target file path using this exact format:

  ```c:{{content}}
  // complete refactored code here
  ```

  The path after the colon is critical — it tells the system which file to
  create or overwrite. You MUST include every line of the file, not just
  changed sections. Do NOT omit any code or use "// ... rest unchanged"
  placeholders.

  After the code block, provide a brief explanation of the safety improvements made.

  {{files}}
"#;
    std::fs::write(".slab/templates/c-improve.yaml", c_improve_template)?;

    let analyze_template = r#"name: analyze
command: /analyze
description: Analyze a Rust translation for issues without compiling
prompt: |
  Analyze the attached Rust translation of C code for correctness issues.
  Do NOT output corrected code. Instead, report your findings as text.

  Check for:
  - Type mismatches (e.g. u32 vs i32 in bitwise ops, pointer mutability)
  - Borrow checker violations (returning &mut from &mut self, multiple mutable borrows)
  - Unsigned integer negation without casting to signed first
  - Arithmetic overflow (e.g. casting to u8 then shifting by 8+)
  - Unused imports
  - Missing or incorrect error handling vs the original C
  - Logic differences from the original C code

  For each issue found, report:
  1. The function or line where it occurs
  2. What the problem is
  3. How to fix it
"#;
    std::fs::write(".slab/templates/analyze.yaml", analyze_template)?;

    // Create example test
    let example_test = r#"name: basic_response
prompt: "Say hello"
assertions:
  - type: contains
    value: "hello"
  - type: length_between
    min: 1
    max: 1000
timeout_secs: 30
tags:
  - basic
  - smoke
"#;
    std::fs::write(".slab/tests/basic.yaml", example_test)?;

    println!("{}", style("✓ Initialized .slab/ directory").green());
    println!("  Created:");
    println!("    .slab/config.toml");
    println!("    .slab/rules/rust.md");
    println!("    .slab/rules/citizen.md");
    println!("    .slab/templates/review.yaml");
    println!("    .slab/templates/c-to-rust.yaml");
    println!("    .slab/templates/c-improve.yaml");
    println!("    .slab/templates/analyze.yaml");
    println!("    .slab/tests/basic.yaml");
    println!("    .slab/sessions/");
    println!();
    println!("{}", style("Run 'slab chat' to start chatting!").cyan());

    Ok(())
}

async fn run_tests(
    client: &OllamaClient,
    config: &Config,
    cli: &Cli,
    filter: Option<&str>,
    model_override: Option<&str>,
) -> Result<()> {
    use std::path::PathBuf;
    use testing::{load_tests_from_directory, TestRunner};

    // Get model
    let model = if let Some(m) = model_override {
        m.to_string()
    } else if let Some(m) = &cli.model {
        m.clone()
    } else if let Some(m) = &config.default_model {
        m.clone()
    } else {
        let models = client.list_models().await?;
        models
            .first()
            .map(|m| m.name.clone())
            .ok_or(SlabError::NoModelsAvailable)?
    };

    // Load tests from tests/prompt_tests/ or .slab/tests/
    let project_root = config::find_project_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let test_dirs = [
        project_root.join("tests/prompt_tests"),
        project_root.join(".slab/tests"),
    ];

    let mut all_tests = Vec::new();
    for dir in &test_dirs {
        let tests = load_tests_from_directory(dir);
        all_tests.extend(tests);
    }

    if all_tests.is_empty() {
        println!("{}", style("No test files found.").yellow());
        println!(
            "{}",
            style("Create tests in tests/prompt_tests/*.yaml or .slab/tests/*.yaml").dim()
        );
        return Ok(());
    }

    println!(
        "{} Loaded {} test(s) from {} directories",
        style("→").cyan(),
        all_tests.len(),
        test_dirs.iter().filter(|d| d.exists()).count()
    );

    // Run tests
    let runner = TestRunner::new(client.clone(), config.clone(), model, cli.verbose);
    let results = runner.run_tests(&all_tests, filter, model_override).await;

    // Print results
    runner.print_results(&results);

    // Exit with error code if any tests failed
    let failed = results.iter().filter(|r| !r.passed).count();
    if failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn print_error(error: &SlabError) {
    // Use default theme for error display
    let theme = ThemeName::Default.to_theme();
    let renderer = BoxRenderer::new(BoxStyle::Rounded, theme.clone()).with_width(60);

    // Build error content with suggestions
    let mut content = format!("{}", error);

    // Add helpful suggestions based on error type
    let suggestions = match error {
        SlabError::OllamaNotRunning(_) => Some(vec![
            format!("Start Ollama: {}", style("ollama serve").cyan()),
            format!(
                "Check status: {}",
                style("curl http://localhost:11434/api/tags").cyan()
            ),
        ]),
        SlabError::NoModelsAvailable => Some(vec![format!(
            "Pull a model: {}",
            style("ollama pull qwen2.5:7b").cyan()
        )]),
        SlabError::ModelNotFound(model) => Some(vec![
            format!(
                "Pull the model: {}",
                style(format!("ollama pull {}", model)).cyan()
            ),
            format!("List available: {}", style("slab models").cyan()),
        ]),
        _ => None,
    };

    if let Some(suggestions) = suggestions {
        content.push_str("\n\nSuggestions:");
        for (i, s) in suggestions.iter().enumerate() {
            content.push_str(&format!("\n  {}. {}", i + 1, s));
        }
    }

    eprint!(
        "{}",
        renderer.render_styled_box("Error", &content, &theme.error)
    );
}
