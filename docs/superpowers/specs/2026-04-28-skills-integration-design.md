# Skills Integration Design

**Date:** 2026-04-28
**Status:** Approved

---

## Overview

Add a skills system to the slab CLI that mirrors Claude Code's skill format. Skills are Markdown files with YAML frontmatter that inject behavioral instructions into the LLM's context. They are auto-loaded based on the user's prompt using a hybrid Rust keyword pre-filter + LLM confirmation pass, then remain active for the rest of the session.

---

## Section 1: Skill Format & Discovery

### File Format

Skills use the same format as Claude Code skills — a named directory containing a `SKILL.md` file:

```
.slab/skills/
└── systematic-debugging/
    └── SKILL.md
```

`SKILL.md` structure:

```markdown
---
name: systematic-debugging
description: "Use when encountering any bug, test failure, or unexpected behavior"
---

# Systematic Debugging

...instructions for the LLM...
```

The `name` and `description` frontmatter fields are required. The body is freeform Markdown and becomes the instruction content injected into LLM context.

### Discovery Paths

Slab discovers skills from two directories, in priority order (project overrides global):

1. `.slab/skills/` — project-level, created by `slab init`
2. `~/.config/slab/skills/` — user-level global skills

### `slab init` Seeding

`slab init` creates `.slab/skills/` and seeds it with one example skill: `code-review/SKILL.md`. This skill is adapted to slab's C/Rust use case and serves as a format reference.

`slab init` output gains one line:
```
    .slab/skills/code-review/SKILL.md
```

---

## Section 2: SkillManager & Session State

### Data Structures

A new `src/skills.rs` module:

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

pub struct SkillManager {
    available: HashMap<String, Skill>,  // all discovered skills, keyed by name
    active: Vec<String>,                // names of skills active this session
}
```

### Lifecycle

- On REPL startup: `SkillManager` discovers and loads all available skills from both paths. Global skills are loaded first; project skills are loaded second and overwrite any global skill with the same name. The active list starts empty.
- Skills are activated during the session (auto or manual) and stay active until removed or the session ends.
- `Repl` gains a `skills: SkillManager` field alongside the existing `templates` and `rules` fields.

### Context Injection

Active skill content is injected into the LLM context just before each call, after rules, as a `## Active Skills` section. Each active skill contributes its full `content` under a `### <name>` subheading. If no skills are active, nothing is injected — zero overhead.

### REPL Commands

| Command | Description |
|---|---|
| `/skills` | List all available skills; marks active ones with `[active]` |
| `/skill add <name>` | Manually activate a skill by name |
| `/skill remove <name>` | Deactivate an active skill |

`/help` output is updated to include these commands. Tab completion covers skill names for `/skill add` and `/skill remove`.

---

## Section 3: Hybrid Auto-Loading

### Trigger

Before each `send_message` call, the hybrid matcher runs against skills that are **available but not yet active**. If the active list already covers all available skills, the matcher is skipped.

### Step 1 — Rust Keyword Pre-filter

Tokenize the user's prompt (lowercase, split on whitespace and punctuation). For each inactive skill, tokenize the skill's `name` + `description`. If there is zero token overlap between prompt and skill metadata, the skill is eliminated immediately. Skills with at least one overlapping token become **candidates**.

This step is synchronous, runs in microseconds, and requires no LLM call.

### Step 2 — LLM Confirmation

If the pre-filter produces one or more candidates, slab sends a single short non-streaming request to the LLM:

```
Given this user message: "<prompt>"

Which of these skills are relevant? Reply with a JSON array of skill names only. Return [] if none apply.

Candidates:
- "systematic-debugging": "Use when encountering any bug, test failure..."
- "code-review": "Review code for correctness and style..."
```

The response is parsed as a JSON array of skill names. If the response is malformed or cannot be parsed as JSON, it is treated as `[]` and no skills are loaded. Unrecognized names are silently ignored. Matched skills are added to the active list.

### User Feedback

When one or more skills are auto-loaded, a dim status line is printed before the LLM response:

```
↑ Loaded skill: systematic-debugging
```

Multiple skills on one line, comma-separated if more than one.

### Configuration

`config.toml` gains a `[skills]` section:

```toml
[skills]
auto_load = true  # default
```

Setting `auto_load = false` disables the hybrid matcher entirely. Manual `/skill add` still works regardless.

---

## Section 4: Config Changes

`Config` struct gains a `skills: SkillsConfig` field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_true")]
    pub auto_load: bool,
}
```

`slab config --show` output gains a `Skills:` section showing `auto_load` status.

`slab config --set skills.auto_load=false` is supported.

---

## Architecture Summary

```
src/
  skills.rs        — new: Skill, SkillManager, hybrid matcher
  repl.rs          — add skills field, hook into send_message, add REPL commands
  config.rs        — add SkillsConfig, auto_load default true
  main.rs          — slab init seeds .slab/skills/code-review/SKILL.md
```

No new dependencies required — the keyword tokenizer uses std, and the LLM confirmation call uses the existing `LlmBackend` trait.

---

## Out of Scope

- Skill chaining or skill-invoking-skills
- Skills that can execute shell commands (use templates/phases for that)
- Importing skills from `~/.claude/skills/` directly (user copies manually)
- Per-skill enable/disable persistence across sessions (session-only)
