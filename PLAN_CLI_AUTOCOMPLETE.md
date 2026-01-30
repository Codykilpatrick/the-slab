# CLI Autocomplete Enhancement Plan

This plan outlines improvements to make CLI commands have comprehensive autocomplete support, both for shell completions and in-REPL completions.

## Current State

**Shell Completions:**
- Uses `clap_complete` crate
- Generates completions via `slab completions <shell>`
- Supports bash, zsh, fish, powershell
- Only completes top-level commands and flags

**REPL Completions:**
- Tab key triggers completion for `/` commands
- Hardcoded list of built-in commands
- Dynamic template command loading
- Single/multiple match display

## Proposed Improvements

### 1. Enhanced Shell Completions

**Goal:** Rich shell completions for all commands and arguments.

#### 1.1 Dynamic Model Completion

Complete model names from Ollama:
```bash
slab -m <TAB>
# Shows: llama3, codellama, mistral, etc.
```

**Implementation:**
- Create custom completer that queries `ollama list`
- Cache results to avoid repeated API calls
- Fall back gracefully if Ollama unavailable

**Files to modify:**
- `src/cli.rs` - Add custom completers
- `src/main.rs` - Shell completion generation

#### 1.2 File Path Completion

Complete file paths for relevant arguments:
```bash
slab run --config <TAB>
# Shows: .slab/config.toml, ~/.config/slab/config.toml
```

**Implementation:**
- Use `clap_complete` file path hints
- Filter by extension where appropriate (.toml for config)

#### 1.3 Session Name Completion

Complete existing session names:
```bash
slab chat --session <TAB>
# Shows: default, project-x, debugging-session
```

**Implementation:**
- Read session files from session directory
- Extract session names from filenames

#### 1.4 Test Filter Completion

Complete test names for the test command:
```bash
slab test --filter <TAB>
# Shows: test_greeting, test_code_generation, etc.
```

**Implementation:**
- Parse test files in `.slab/tests/`
- Extract test names

### 2. Enhanced REPL Completions

**Goal:** Context-aware completions for all REPL commands and arguments.

#### 2.1 Command Argument Completion

Complete arguments for commands:
```
/model <TAB>
# Shows: llama3, codellama, mistral

/add <TAB>
# Shows: src/, Cargo.toml, README.md

/remove <TAB>
# Shows files currently in context
```

**Implementation:**
- Create completion registry mapping commands to completers
- Implement per-command completion functions:
  - `/model` - Query Ollama for models
  - `/add` - File system browsing
  - `/remove` - Current context files
  - `/help` - Available commands

**Files to modify:**
- `src/repl.rs` - Completion logic expansion

#### 2.2 Fuzzy Matching

Allow fuzzy matching for faster completion:
```
/mo<TAB> → /model
/ad<TAB> → /add
/h<TAB> → /help
```

**Implementation:**
- Implement fuzzy matching algorithm
- Rank matches by:
  1. Prefix match (highest)
  2. Substring match
  3. Fuzzy match (lowest)
- Show match score in completion list

**Consider adding:** `fuzzy-matcher` crate or implement simple algorithm

#### 2.3 Template Variable Completion

Complete template variables when using templates:
```
/summarize <TAB>
# Shows: {{input}}, {{context}}, etc.
```

**Implementation:**
- Parse template to extract variable names
- Offer completion for required variables

#### 2.4 History-Based Completion

Suggest from command history:
```
/add <TAB>
# Shows recently added files first
```

**Implementation:**
- Track command history with arguments
- Weight recent/frequent completions higher

### 3. Inline Completion Preview

**Goal:** Show completion preview inline (like fish shell).

**Implementation:**
```
/mo│del     ← dim preview of completion
   ↑ cursor

Press TAB to accept, or keep typing
```

- Show top completion as dim text after cursor
- Tab accepts completion
- Any other key continues normal input
- Right arrow accepts one character

**Files to modify:**
- `src/repl.rs` - Input display logic

### 4. Completion Menu

**Goal:** Interactive completion selection menu.

**Implementation:**
```
/add <TAB>

╭─ Completions ─────────────────╮
│ > src/                        │
│   Cargo.toml                  │
│   README.md                   │
│   .slab/                      │
╰───────────────────────────────╯
Use ↑/↓ to select, Enter to confirm
```

- Arrow keys navigate
- Enter selects
- Escape cancels
- Continue typing filters list
- Show descriptions for commands

**Files to modify:**
- `src/repl.rs` - Add completion menu UI

### 5. Smart Context Completion

**Goal:** Suggest based on current context.

**Implementation:**
- If context has code files, suggest code-related templates
- If error in recent response, suggest debugging templates
- Time-based suggestions (e.g., `/clear` if long session)

### 6. Path Completion Enhancements

**Goal:** Better file/directory completion.

**Implementation:**
```
/add src/<TAB>

src/
├── main.rs
├── lib.rs
├── cli.rs
└── [4 more...]
```

- Show directory tree preview
- Different icons for files vs directories
- Show file sizes
- Respect `.gitignore`

**Files to modify:**
- `src/repl.rs` - File completion logic

### 7. Completion Configuration

**Goal:** User-customizable completion behavior.

**Add to config.toml:**
```toml
[completion]
enabled = true
fuzzy = true
inline_preview = true
menu_style = "popup"  # "popup", "list", "inline"
max_suggestions = 10
history_weight = 0.3
case_sensitive = false
```

**Files to modify:**
- `src/config.rs` - Completion config
- `src/repl.rs` - Apply config

### 8. Completion Documentation

**Goal:** Show help text in completions.

**Implementation:**
```
/add <TAB>

╭─ Completions ─────────────────────────────────────╮
│ /add <path>    Add file or directory to context   │
│ /addurl <url>  Add content from URL to context    │
╰───────────────────────────────────────────────────╯
```

- Show command descriptions inline
- Show argument types and requirements
- Link to `/help <command>` for more info

## Implementation Details

### New Module: `src/completion.rs`

```rust
pub struct CompletionEngine {
    command_completers: HashMap<String, Box<dyn Completer>>,
    history: Vec<String>,
    config: CompletionConfig,
}

pub trait Completer {
    fn complete(&self, input: &str, context: &CompletionContext) -> Vec<Completion>;
}

pub struct Completion {
    pub text: String,
    pub display: String,
    pub description: Option<String>,
    pub score: f32,
    pub kind: CompletionKind,
}

pub enum CompletionKind {
    Command,
    File,
    Directory,
    Model,
    Session,
    Template,
    History,
}
```

### Built-in Completers

1. **CommandCompleter** - REPL commands
2. **FileCompleter** - File system paths
3. **ModelCompleter** - Ollama models
4. **SessionCompleter** - Saved sessions
5. **TemplateCompleter** - Available templates
6. **HistoryCompleter** - Command history
7. **ContextFileCompleter** - Files in current context

## Shell Completion Script Enhancements

### Zsh Custom Completion

```zsh
_slab_models() {
    local models
    models=(${(f)"$(slab models --names-only 2>/dev/null)"})
    _describe 'model' models
}

_slab_sessions() {
    local sessions
    sessions=(${(f)"$(ls ~/.config/slab/sessions/ 2>/dev/null | sed 's/.json$//')"})
    _describe 'session' sessions
}
```

### Bash Custom Completion

```bash
_slab_completions() {
    case "${prev}" in
        -m|--model)
            COMPREPLY=($(compgen -W "$(slab models --names-only 2>/dev/null)" -- "${cur}"))
            ;;
        --session|-s)
            COMPREPLY=($(compgen -W "$(ls ~/.config/slab/sessions/ 2>/dev/null | sed 's/.json$//')" -- "${cur}"))
            ;;
    esac
}
```

## Implementation Priority

1. **High Priority** (Most impactful)
   - Command argument completion in REPL
   - File path completion for `/add`
   - Model name completion
   - Completion menu UI

2. **Medium Priority** (Quality of life)
   - Fuzzy matching
   - Inline preview
   - History-based suggestions
   - Enhanced shell completions

3. **Low Priority** (Polish)
   - Smart context completion
   - Completion documentation
   - Configuration options

## Testing Plan

1. **Unit Tests**
   - Test each completer in isolation
   - Test fuzzy matching algorithm
   - Test completion ranking

2. **Integration Tests**
   - Test REPL completion flow
   - Test shell completion scripts

3. **Manual Testing**
   - Test in bash, zsh, fish shells
   - Test with various terminal widths
   - Test with large directories
   - Test with many models

## Dependencies to Add

```toml
# Optional additions to Cargo.toml
fuzzy-matcher = "0.3"  # For fuzzy completion matching
# Or implement simple fuzzy algorithm to avoid dependency
```

## Migration Path

1. Phase 1: Add argument completion for existing commands
2. Phase 2: Add completion menu UI
3. Phase 3: Add fuzzy matching and inline preview
4. Phase 4: Add configuration options
5. Phase 5: Enhanced shell completions with custom scripts
