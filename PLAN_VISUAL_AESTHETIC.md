# Visual Aesthetic Improvement Plan

This plan outlines improvements to enhance the visual appearance and user experience of The Slab CLI tool.

## Current State

The tool currently uses:
- `console` crate for colored text (cyan, green, yellow, red, blue, dim)
- `indicatif` for spinners
- `syntect` for code syntax highlighting
- Simple Unicode decorators (✓, ✗, ⚠, →, ┃, ─)

## Proposed Improvements

### 1. Theme System

**Goal:** Allow users to customize colors and choose between built-in themes.

**Implementation:**
- Add theme configuration to `config.toml`:
  ```toml
  [ui]
  theme = "default"  # or "monokai", "nord", "solarized", "minimal"
  ```
- Create `src/theme.rs` with predefined color palettes
- Define semantic color roles: `primary`, `secondary`, `success`, `warning`, `error`, `muted`
- Apply theme colors consistently across all output

**Files to modify:**
- `src/config.rs` - Add theme configuration
- Create `src/theme.rs` - Theme definitions and loading
- `src/repl.rs` - Apply theme colors to prompts and output
- `src/main.rs` - Apply theme to command output
- `src/file_ops.rs` - Apply theme to diff display

### 2. Enhanced Box Drawing & Borders

**Goal:** Use box-drawing characters for structured output sections.

**Implementation:**
- Create bordered panels for:
  - Welcome message
  - Help output
  - Context summary
  - Error messages
  - File operation confirmations
- Box styles:
  ```
  ╭─────────────────────────╮
  │  Content here           │
  ╰─────────────────────────╯
  ```
- Add utility functions for consistent box rendering

**New module:** `src/ui/boxes.rs`

### 3. Improved Response Display

**Goal:** Make LLM responses more visually distinct and readable.

**Implementation:**
- Add a header bar for responses:
  ```
  ╭─ Response ──────────────────────────────╮
  │                                         │
  │ The response text goes here...          │
  │                                         │
  ╰─────────────────────────────────────────╯
  ```
- Better code block styling with language labels:
  ```
  ╭─ rust ─────────────────────────────────╮
  │ fn main() {                            │
  │     println!("Hello!");                │
  │ }                                      │
  ╰────────────────────────────────────────╯
  ```
- Improve markdown rendering (bold, italic, lists, headers)

**Files to modify:**
- `src/repl.rs` - Response display logic
- `src/highlight.rs` - Code block formatting

### 4. Status Bar / Header

**Goal:** Persistent information display at screen top.

**Implementation:**
- Show current model, session name, context stats
- Format:
  ```
  ┌─ slab ─────────────────────────────────────────────────┐
  │ Model: llama3  │ Session: default │ Context: 3f│256t   │
  └────────────────────────────────────────────────────────┘
  ```
- Update on model/session changes
- Optional: Make toggleable with `/status` command

**Files to modify:**
- `src/repl.rs` - Add status bar rendering

### 5. Animated Transitions

**Goal:** Smooth visual feedback for state changes.

**Implementation:**
- Fade-in effect for responses (character-by-character for non-streaming)
- Subtle typing indicator animation
- Spinner variations based on operation type:
  - Thinking: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`
  - Loading files: `◐◓◑◒`
  - Saving: `⣾⣽⣻⢿⡿⣟⣯⣷`

**Files to modify:**
- `src/repl.rs` - Animation handling

### 6. Improved Diff Display

**Goal:** More intuitive file operation previews.

**Implementation:**
- Side-by-side diff option for wider terminals
- Line numbers for context
- Syntax highlighting in diffs
- Collapsible unchanged sections:
  ```
  ─── file.rs ───────────────────────────────
   10 │   fn old_function() {
  -11 │-      let x = 1;
  +11 │+      let x = 2;
   12 │   }
      │ ... 15 unchanged lines ...
   28 │   fn another() {
  ```

**Files to modify:**
- `src/file_ops.rs` - Diff rendering

### 7. Prompt Enhancements

**Goal:** More informative and visually appealing prompt.

**Implementation:**
- Multi-line prompt with context preview:
  ```
  ╭─ Context: 3 files │ 256 tokens ─────────────╮
  │ main.rs, lib.rs, config.rs                  │
  ╰─────────────────────────────────────────────╯
  ➜
  ```
- Git-style prompt showing session state
- Visual indicator when context is near token limit

**Files to modify:**
- `src/repl.rs` - Prompt rendering

### 8. Better Error Messages

**Goal:** Clear, actionable error display.

**Implementation:**
- Styled error boxes with suggestions:
  ```
  ╭─ Error ─────────────────────────────────────╮
  │ Could not connect to Ollama                 │
  │                                             │
  │ Suggestion: Run `ollama serve` first        │
  ╰─────────────────────────────────────────────╯
  ```
- Different styling for warnings vs errors
- Include error codes for documentation reference

**Files to modify:**
- `src/error.rs` - Error display formatting
- All files that display errors

### 9. Progress Indicators

**Goal:** Better feedback for long operations.

**Implementation:**
- Progress bars for file loading with ETA
- Token counting progress for large contexts
- Model loading indicator with status

**Files to modify:**
- `src/context.rs` - File loading progress
- `src/ollama.rs` - Model loading status

### 10. ASCII Art / Branding

**Goal:** Memorable visual identity.

**Implementation:**
- Subtle ASCII banner on startup (optional, off by default):
  ```
   _____ _       _
  |   __| |___ _| |_
  |__   | | .'| . |
  |_____|_|__,|___|
  ```
- Version display in help
- Add `--quiet` flag to suppress banner

**Files to modify:**
- `src/main.rs` - Banner display
- `src/cli.rs` - Quiet flag

## Implementation Priority

1. **High Priority** (Core UX improvements)
   - Theme system
   - Enhanced box drawing
   - Improved response display
   - Better error messages

2. **Medium Priority** (Polish)
   - Status bar
   - Prompt enhancements
   - Improved diff display

3. **Low Priority** (Nice to have)
   - Animated transitions
   - Progress indicators
   - ASCII art branding

## Dependencies to Add

```toml
# Optional additions to Cargo.toml
unicode-width = "0.1"     # Proper Unicode width calculation
textwrap = "0.16"         # Text wrapping utilities
```

## Configuration Options

Add to `config.toml`:
```toml
[ui]
theme = "default"
show_status_bar = true
show_banner = false
box_style = "rounded"  # "rounded", "sharp", "double", "ascii"
code_block_style = "bordered"  # "bordered", "plain"
diff_style = "unified"  # "unified", "side-by-side"
```

## Testing Plan

1. Test all themes across different terminal emulators
2. Verify box drawing renders correctly in:
   - iTerm2
   - Terminal.app
   - Windows Terminal
   - Linux terminals (gnome-terminal, konsole)
3. Test with different terminal widths (80, 120, 200 cols)
4. Verify color output with `NO_COLOR` environment variable
5. Test accessibility with screen readers
