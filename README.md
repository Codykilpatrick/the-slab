<p align="center">
  <img src="the-slab.png" alt="The Slab" width="400">
</p>

<h1 align="center">The Slab</h1>

<p align="center">
  <strong>An offline CLI tool for local LLMs via Ollama</strong><br>
  Designed for air-gapped environments with zero external dependencies at runtime.
</p>

---

## Table of Contents

- [Features](#features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Configuration](#configuration)
- [File Operations](#file-operations)
- [Templates](#templates)
- [Rules](#rules)
- [Testing](#testing)
- [Shell Completions](#shell-completions)
- [Project Structure](#project-structure)
- [Troubleshooting](#troubleshooting)
- [License](#license)

## Features

- **100% Offline** - Works completely offline, perfect for air-gapped environments
- **Streaming Responses** - Real-time streaming output from your local models
- **File Operations** - Create and edit files directly from LLM responses with diff preview
- **Context Management** - Add files to context, track token usage
- **Prompt Templates** - Reusable templates with Handlebars syntax
- **Rules Engine** - Persistent coding guidelines injected into every conversation
- **Testing Framework** - Validate LLM responses with assertions
- **Session Management** - Save and resume conversations
- **Syntax Highlighting** - Beautiful code blocks in responses

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/the-slab.git
cd the-slab

# Build release binary
cargo build --release

# Install to your PATH (optional)
cp target/release/slab ~/.local/bin/
```

### Prerequisites

- [Rust](https://rustup.rs/) 1.70+
- [Ollama](https://ollama.ai) running locally

## Quick Start

```bash
# Start Ollama (in another terminal)
ollama serve

# Pull a model
ollama pull qwen2.5:7b

# Initialize a project
slab init

# Start chatting
slab chat
```

## Usage

### Commands

```bash
slab chat                    # Start interactive REPL
slab chat --continue         # Resume last session
slab chat --session myproj   # Use named session
slab run "your prompt"       # Run single prompt
slab models                  # List available models
slab test                    # Run prompt tests
slab init                    # Initialize .slab/ directory
slab config --show           # Show configuration
slab completions bash        # Generate shell completions
```

### Global Options

```bash
-m, --model <MODEL>    # Override default model
-c, --config <FILE>    # Use custom config file
-v, --verbose          # Enable verbose output
    --no-stream        # Disable streaming
```

### REPL Commands

| Command | Description |
|---------|-------------|
| `/help` | Show all commands |
| `/help <cmd>` | Detailed help for a command |
| `/exit`, `/quit`, `/q` | Exit the REPL |
| `/clear` | Clear conversation history |
| `/model [name]` | Show or change model |
| `/context` | Show context summary |
| `/tokens` | Show token usage |
| `/files` | List files in context |
| `/add <path>` | Add file or directory to context |
| `/remove <file>` | Remove file from context |
| `/fileops [on\|off]` | Toggle file operations |
| `/templates` | List available templates |
| `/rules` | Show loaded rules |

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+C` | Cancel current input |
| `Ctrl+D` | Exit |
| `Ctrl+L` | Clear screen |
| `Up/Down` | Navigate command history |
| `Tab` | Autocomplete commands |

## Configuration

Configuration is loaded from (in order of priority):

1. `.slab/config.toml` (project-local)
2. `~/.config/slab/config.toml` (global)

### Example Config

```toml
ollama_host = "http://localhost:11434"
default_model = "qwen2.5:7b"
context_limit = 32768

[models.qwen]
name = "qwen2.5:7b"
temperature = 0.7
top_p = 0.9
system_prompt = "You are a helpful coding assistant."

[ui]
streaming = true
```

### Config Options

| Key | Description | Default |
|-----|-------------|---------|
| `ollama_host` | Ollama API URL | `http://localhost:11434` |
| `default_model` | Default model to use | First available |
| `context_limit` | Max context tokens | `32768` |
| `ui.streaming` | Enable streaming | `true` |
| `ui.auto_apply_file_ops` | Auto-apply file operations | `false` |

## File Operations

The Slab can detect file operations in LLM responses and apply them with your confirmation.

A default system prompt instructs the LLM to output files in a special format.

### Creating/Editing Files

````markdown
```rust:src/main.rs
fn main() {
    println!("Hello, world!");
}
```
````

### Deleting Files

```text
DELETE:src/old_file.rs
```

When the LLM outputs code blocks with filenames or delete markers, you'll be prompted to review and choose:

- `[a]pply` - Apply this change
- `[v]iew` - View full diff
- `[s]kip` - Skip this change
- `[A]pply all` - Apply all remaining changes

Toggle with `/fileops on` or `/fileops off`.

### Auto-Apply Mode

To automatically apply file operations without prompting:

```bash
# Enable via CLI
slab config --set ui.auto_apply_file_ops=true

# Or in config file
```

```toml
# .slab/config.toml
[ui]
auto_apply_file_ops = true
```

When enabled, all safe file operations are applied immediately. Safety checks still prevent operations outside the project root or in `.git/`.

### Customizing the System Prompt

You can customize the system prompt in your config:

```toml
# .slab/config.toml
system_prompt = """
You are a helpful coding assistant.

When creating files, use: ```language:path/to/file
When deleting files, use: DELETE:path/to/file
"""
```

## Templates

Create reusable prompt templates in `.slab/templates/`:

```yaml
# .slab/templates/review.yaml
name: code_review
command: /review
description: Review code for issues
variables:
  - name: focus
    default: all
prompt: |
  Review this code, focusing on {{focus}}:

  {{content}}

  Identify bugs, performance issues, and improvements.
```

Use in REPL:

```text
/review focus=security
```

### Built-in Variables

| Variable | Description |
|----------|-------------|
| `{{content}}` | User-provided content |
| `{{file:path}}` | Contents of a file |
| `{{files}}` | All files in context |
| `{{language}}` | Detected language |
| `{{date}}` | Current date |

## Rules

Add persistent coding guidelines in `.slab/rules/`:

```markdown
# .slab/rules/rust.md
---
name: rust_guidelines
applies_to:
  - "*.rs"
priority: 10
---

# Rust Coding Rules

- Use `thiserror` for custom error types
- No `unwrap()` in production code
- Prefer `&str` over `String` for parameters
```

Rules are automatically injected into context based on file patterns.

## Testing

Create tests in `.slab/tests/` or `tests/prompt_tests/`:

```yaml
# .slab/tests/error_handling.yaml
name: rust_error_handling
prompt: "Write a Rust function that reads a file"
assertions:
  - type: contains
    value: "Result<"
  - type: not_contains
    value: "unwrap()"
  - type: max_latency
    ms: 5000
timeout_secs: 30
tags:
  - rust
  - error-handling
```

Run tests:

```bash
slab test                        # Run all tests
slab test --filter rust          # Filter by name/tag
slab test --model qwen2.5:14b    # Test specific model
```

### Assertion Types

| Type | Description |
|------|-------------|
| `contains` | Response contains string |
| `not_contains` | Response doesn't contain string |
| `regex` | Response matches pattern |
| `not_regex` | Response doesn't match pattern |
| `valid_json` | Response is valid JSON |
| `length_between` | Response length in range |
| `max_latency` | Response time under limit |

## Shell Completions

Generate completions for your shell:

```bash
# Bash
slab completions bash > ~/.local/share/bash-completion/completions/slab

# Zsh
slab completions zsh > ~/.zfunc/_slab

# Fish
slab completions fish > ~/.config/fish/completions/slab.fish
```

## Project Structure

```text
.slab/
├── config.toml      # Project configuration
├── templates/       # Prompt templates
├── rules/           # Coding guidelines
├── tests/           # Prompt tests
└── sessions/        # Saved sessions
```

## Troubleshooting

### Ollama not running

```text
Error: Failed to connect to Ollama

Suggestions:
  1. Start Ollama: ollama serve
  2. Check if it's running: curl http://localhost:11434/api/tags
```

### No models available

```bash
# Pull a model
ollama pull qwen2.5:7b

# Or a larger model for better results
ollama pull qwen2.5:32b
```

### Model not found

```bash
# List available models
slab models

# Pull the missing model
ollama pull <model-name>
```

---

## License

MIT License - See [LICENSE](LICENSE) for details.
