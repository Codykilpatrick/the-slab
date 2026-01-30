use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "slab")]
#[command(
    author,
    version,
    about = "The Slab - Offline CLI for local LLMs via Ollama"
)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Model to use (overrides config default)
    #[arg(short, long, global = true)]
    pub model: Option<String>,

    /// Path to config file
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Disable streaming (wait for complete response)
    #[arg(long, global = true)]
    pub no_stream: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start interactive chat REPL
    Chat {
        /// Continue from the last session
        #[arg(long, short = 'C')]
        r#continue: bool,

        /// Use a named session
        #[arg(long, short = 's')]
        session: Option<String>,
    },

    /// Run a single prompt and exit
    Run {
        /// The prompt to send
        prompt: String,
    },

    /// Show or edit configuration
    Config {
        /// Show current configuration
        #[arg(long)]
        show: bool,

        /// Initialize a new config file
        #[arg(long)]
        init: bool,

        /// Set a config value (key=value)
        #[arg(long)]
        set: Option<String>,
    },

    /// List available Ollama models
    Models {
        /// Output only model names (for shell completion scripts)
        #[arg(long)]
        names_only: bool,
    },

    /// List saved sessions
    Sessions {
        /// Output only session names (for shell completion scripts)
        #[arg(long)]
        names_only: bool,
    },

    /// Run prompt tests
    Test {
        /// Filter tests by pattern
        #[arg(long)]
        filter: Option<String>,

        /// Run tests with a specific model
        #[arg(long)]
        model: Option<String>,
    },

    /// Initialize a new project with .slab directory
    Init,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

impl Cli {
    pub fn command_or_default(&self) -> Commands {
        self.command.clone().unwrap_or(Commands::Chat {
            r#continue: false,
            session: None,
        })
    }
}

impl Clone for Commands {
    fn clone(&self) -> Self {
        match self {
            Commands::Chat {
                r#continue,
                session,
            } => Commands::Chat {
                r#continue: *r#continue,
                session: session.clone(),
            },
            Commands::Run { prompt } => Commands::Run {
                prompt: prompt.clone(),
            },
            Commands::Config { show, init, set } => Commands::Config {
                show: *show,
                init: *init,
                set: set.clone(),
            },
            Commands::Models { names_only } => Commands::Models {
                names_only: *names_only,
            },
            Commands::Sessions { names_only } => Commands::Sessions {
                names_only: *names_only,
            },
            Commands::Test { filter, model } => Commands::Test {
                filter: filter.clone(),
                model: model.clone(),
            },
            Commands::Init => Commands::Init,
            Commands::Completions { shell } => Commands::Completions { shell: *shell },
        }
    }
}
