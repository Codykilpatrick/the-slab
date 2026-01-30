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

use clap::Parser;
use console::style;
use std::process;

use cli::{Cli, Commands};
use config::Config;
use error::{Result, SlabError};
use ollama::OllamaClient;
use repl::Repl;

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
            repl.run().await?;

            // Auto-save session
            if let Some(name) = session_name {
                if let Err(e) = repl.save_session(&name) {
                    eprintln!("{} {}", style("Warning:").yellow(), e);
                }
            }
        }

        Commands::Run { prompt } => {
            // Health check
            client.health_check().await?;

            let model = get_model(&cli, &config, &client).await?;
            repl::run_single_prompt(&client, &config, &model, &prompt, streaming).await?;
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
    let sessions_dir = std::path::PathBuf::from(".slab/sessions");

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

    // Create example rule
    let rust_rules = r#"# Rust Coding Rules

- Use `thiserror` for custom error types
- No `unwrap()` or `expect()` in production code - use proper error handling
- Use `tracing` for logging instead of `println!`
- Prefer `&str` over `String` for function parameters when ownership isn't needed
"#;
    std::fs::write(".slab/rules/rust.md", rust_rules)?;

    // Create example template
    let review_template = r#"name: code_review
command: /review
description: Review code for issues and improvements
variables:
  - name: focus
    default: all
prompt: |
  Review the following code, focusing on {{focus}}:

  {{content}}

  Please identify:
  1. Potential bugs or issues
  2. Performance concerns
  3. Style/readability improvements
  4. Security considerations
"#;
    std::fs::write(".slab/templates/review.yaml", review_template)?;

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
    println!("    .slab/templates/review.yaml");
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
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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
    eprintln!("{} {}", style("Error:").red().bold(), error);

    // Provide helpful suggestions
    match error {
        SlabError::OllamaNotRunning(_) => {
            eprintln!();
            eprintln!("{}", style("Suggestions:").yellow());
            eprintln!("  1. Start Ollama: {}", style("ollama serve").cyan());
            eprintln!(
                "  2. Check if it's running: {}",
                style("curl http://localhost:11434/api/tags").cyan()
            );
        }
        SlabError::NoModelsAvailable => {
            eprintln!();
            eprintln!("{}", style("Suggestions:").yellow());
            eprintln!("  Pull a model: {}", style("ollama pull qwen2.5:7b").cyan());
        }
        SlabError::ModelNotFound(model) => {
            eprintln!();
            eprintln!("{}", style("Suggestions:").yellow());
            eprintln!(
                "  1. Pull the model: {}",
                style(format!("ollama pull {}", model)).cyan()
            );
            eprintln!(
                "  2. List available models: {}",
                style("slab models").cyan()
            );
        }
        _ => {}
    }
}
