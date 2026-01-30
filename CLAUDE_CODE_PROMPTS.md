# The Slab - Claude Code Prompts

## Project Overview

**The Slab** is an offline CLI tool for interacting with locally hosted LLMs via Ollama, designed for air-gapped environments.

**Target Models:** Qwen 30B, GPT-OSS 120B, any Ollama-compatible model

**Core Features:**
- 100% offline operation (air-gap compatible)
- File system operations (create/edit/delete with diff preview)
- Configurable models via YAML/TOML
- Reusable prompt templates with slash commands
- Context window control
- Testing pipeline for prompt validation
- Custom rules system
- Polished terminal UI

---

## PROMPT 1: Core Infrastructure

```
Create "The Slab" - an offline Rust CLI tool for local LLMs via Ollama.

Requirements:
1. CLI with commands: `slab chat` (REPL), `slab run "<prompt>"`, `slab config`, `slab models`, `slab test`
2. Global flags: --model, --config, --verbose, --no-stream
3. Config system (TOML) with: ollama_host, default_model, model configs (temperature, top_p, system_prompt), context_limit, paths for templates/rules
4. Config locations: ./.slab/config.toml (project) or ~/.config/slab/config.toml (global)
5. Ollama client: /api/generate, /api/chat, /api/tags endpoints with streaming support
6. Basic REPL: prompt with model name, send to Ollama, stream response, handle Ctrl+C/Ctrl+D
7. MUST use rustls (NO openssl) - this is for air-gapped environments

Key dependencies: clap 4.x, tokio, reqwest (rustls-tls only), serde, serde_yaml, toml, anyhow, thiserror, crossterm, console, indicatif, futures-util

Show helpful error when Ollama isn't running. Make it feel polished from the start.
```

---

## PROMPT 2: File System Operations

```
Add file system operations to The Slab.

Requirements:
1. FileOperation enum: Create, Edit, Delete, Rename - each with preview(), execute(), rollback()
2. Diff generation using `similar` crate with syntax highlighting via `syntect`
3. Parse LLM output for file operations - detect code blocks with filenames like ```rust:src/main.rs
4. Safety checks: prevent operations outside project root, protect .git/**, warn on destructive ops
5. Confirmation UI: show diff preview, ask [a]pply/[v]iew/[s]kip, execute with progress
6. Auto-detect file operations in REPL responses and prompt user

The flow: LLM responds → detect file blocks → show colored diff → confirm → apply changes

Add dependencies: similar, syntect, walkdir
```

---

## PROMPT 3: Context & Prompt Templates

```
Add context window management and prompt templates to The Slab.

Requirements:
1. ContextManager: tracks system_prompt, rules, files (HashMap<PathBuf, content>), messages, token_budget
   - Methods: add_file, remove_file, add_message, build_prompt, token_count, prune_to_fit
2. Simple offline tokenizer (chars/4 approximation, no external deps)
3. Prompt templates in YAML with Handlebars syntax:
   - Fields: name, command, description, variables (with defaults), prompt template
   - Built-in vars: {{content}}, {{file:path}}, {{language}}, {{project}}, {{date}}
4. Slash commands in REPL:
   - Built-in: /help, /exit, /clear, /context, /files, /add <file>, /remove <file>, /model, /tokens
   - Template commands: /review, /explain, /refactor, /test (loaded from templates)
5. Load templates from: built-in defaults, ~/.config/slab/templates/, .slab/templates/

Add dependencies: handlebars, glob, chrono

Create default templates: code_review.yaml, explain.yaml, refactor.yaml, test_gen.yaml
```

---

## PROMPT 4: Rules Engine & Testing

```
Add rules engine and testing pipeline to The Slab.

RULES ENGINE:
1. Rule struct: name, description, applies_to (glob patterns), priority, enabled, content
2. Load from .slab/rules/ - support .yaml, .md, .txt formats
3. RuleEngine: load rules, filter by file pattern, build combined rules prompt
4. Rules inject into context before user messages

TESTING PIPELINE:
1. TestCase: name, prompt, system_prompt, model, assertions, timeout_secs, tags
2. Assertion types: contains, not_contains, regex, max_latency, valid_json, length_between
3. Test runner: load tests from directory, run against Ollama, check assertions
4. CLI: `slab test`, `slab test --filter <pattern>`, `slab test --model <model>`
5. Reports: table format with pass/fail, timing stats, detailed failures in verbose mode

Test file format (YAML):
```yaml
name: rust_error_handling
prompt: "Write a function that reads a file"
assertions:
  - type: contains
    value: "Result<"
  - type: not_contains
    value: "unwrap()"
```

Add dependency: regex
```

---

## PROMPT 5: Polish & Final Integration

```
Polish The Slab CLI for a professional feel.

Requirements:
1. Enhanced prompt: `[qwen:30b] ➜ ` with model indicator
2. Syntax-highlighted code blocks in responses using syntect
3. Spinner during LLM thinking, progress bars for batch operations
4. Status display: current model, context usage (tokens used/limit), files in context
5. Keyboard shortcuts: Ctrl+C (cancel), Ctrl+D (exit), Ctrl+L (clear), Up/Down (history), Tab (autocomplete commands)
6. Session management: auto-save on exit, --continue flag, --session <name>
7. `slab init` wizard: detect Ollama models, create config, setup directories
8. Shell completions: `slab completions bash/zsh/fish`
9. Friendly errors with suggestions, not raw stack traces
10. Help system: --help, /help in REPL, /help <command>

Make it feel like a premium developer tool - responsive, clear, consistent styling.
```

---

## Example Files to Create

### .slab/config.toml
```toml
ollama_host = "http://localhost:11434"
default_model = "qwen2.5:30b"
context_limit = 32768

[models.qwen]
name = "qwen2.5:30b"
temperature = 0.7
system_prompt = "You are a helpful coding assistant."

[ui]
theme = "dark"
streaming = true
```

### .slab/templates/review.yaml
```yaml
name: code_review
command: /review
description: Review code for issues
variables:
  - name: focus
    default: all
prompt: |
  Review this code, focus on {{focus}}:
  {{content}}
```

### .slab/rules/rust.md
```markdown
# Rust Rules
- Use thiserror for errors
- No unwrap() in production
- Use tracing for logging
```

### tests/prompt_tests/basic.yaml
```yaml
name: error_handling
prompt: "Write a Rust file reader function"
assertions:
  - type: contains
    value: "Result<"
```

---

## Quick Start

Run these prompts in order with Claude Code in your project directory:

1. Create project: `cargo new the-slab && cd the-slab`
2. Run Prompt 1 (Infrastructure)
3. Run Prompt 2 (File Operations)
4. Run Prompt 3 (Context & Templates)
5. Run Prompt 4 (Rules & Testing)
6. Run Prompt 5 (Polish)
7. Build: `cargo build --release`
8. Test: `./target/release/slab init && ./target/release/slab chat`
