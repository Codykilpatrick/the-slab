# The Slab

An offline CLI tool for local LLMs via Ollama. Air-gap compatible.

## Tech Stack
- Rust with rustls (NO openssl)
- Ollama API
- clap for CLI, tokio for async, ratatui for TUI

## Key Constraints
- Must work 100% offline
- Config: .slab/config.toml or ~/.config/slab/config.toml
- Templates: .slab/templates/*.yaml
- Rules: .slab/rules/*.md

## Current Phase

Phase 5 complete: Polish and final integration. All phases completed.

### Phase 5 Features

- Syntax highlighting for code blocks in responses (src/highlight.rs)
- Enhanced keyboard shortcuts: Ctrl+L (clear), Up/Down (history), Tab (autocomplete)
- Session management: `--continue` flag, `--session <name>` for named sessions (src/session.rs)
- `slab init` wizard with Ollama model detection
- `slab completions <shell>` for bash/zsh/fish/powershell completions
- `/help <command>` for detailed command help

### Previous Phases

- Phase 1: Core CLI, config, Ollama client with streaming
- Phase 2: File operations with diff preview and confirmation UI
- Phase 3: Context management, token budgeting, Handlebars templates
- Phase 4: Rules engine, testing framework with assertions

See CLAUDE_CODE_PROMPTS.md for detailed implementation plan.
