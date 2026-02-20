# Changelog

All notable changes to The Slab are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

---

## [1.1.1] - 2026-02-20

### Fixed

- **`/add` ghost completion resolves paths from launch directory, not project root** — tab completion and inline preview for `/add` were using the project root (where `.slab/` lives) as the working directory instead of the directory the user ran `slab chat` from, causing file suggestions to be wrong when running from a subdirectory.

---

## [1.1.0] - 2026-02-20

### Added

- **`/c-rationale` template** — new template seeded by `slab init` that produces two outputs in one pass: the MISRA/CERT-refactored C source file and a self-contained HTML change rationale report (`.slab/reports/change-rationale-{basename}.html`). The HTML includes an executive summary, a capability map table (RETAINED / TRANSFORMED / DROPPED badges per function/behavior), collapsible before/after category sections (Type Safety, Error Handling, Memory Safety, Interface Clarity), and a standards compliance table with MISRA C:2012 and CERT C citations. Dark-themed, inline CSS only, no external dependencies.

---

## [1.0.0] - 2026-02-19

### Added

- **Phase loop `feedback` field** — each phase can now declare `feedback: always` (inject output into LLM context even on exit 0), `feedback: never` (print to terminal only), or `feedback: on_failure` (default, preserves existing behavior). Enables monitoring/reporting tools like complexity checkers that should always report to the LLM but only re-loop on violations
- **Per-phase `follow_up` prompts** — each phase can specify its own follow-up message sent to the LLM when that phase triggers `continue`. Precedence: per-phase → template-level `phases_follow_up` → hardcoded default
- **`phases_follow_up` template field** — template-level fallback follow-up prompt, used when no triggered phase has its own `follow_up`
- **`max_phases` template field** — caps the number of phase loop iterations; defaults to 10 if unset
- **`/c-quality` template** — new template seeded by `slab init`; runs an iterative compile check (`gcc -fsyntax-only`) followed by a complexity check (`lizard --CCN 10`, swappable). Uses `feedback: always` on the complexity phase so the LLM always sees the report, and only re-loops on violations

### Fixed

- **Phase loop sent two consecutive user messages per pass** — phase results were added to context via `add_message`, then `send_message` added a second user turn before any assistant response, violating chat API conventions. Both are now combined into a single `send_message` call
- **Exec errors always triggered loop continuation** — shell errors (command not found, permission denied) unconditionally set `any_continue = true`, ignoring the phase's `on_failure` setting. Now correctly gated: only continues when `on_failure == continue`
- **`{{file}}` with empty context ran broken commands** — when no files were in context, `{{file}}` expanded to `""`, producing commands like `gcc -Wall ""` that failed for the wrong reason and triggered looping incorrectly. Phases using `{{file}}` or `{{files}}` are now skipped with a warning when context is empty

### Changed

- **`LlmBackend` trait** — `OllamaClient` now implements a `LlmBackend` trait; `Repl<B: LlmBackend>` is generic over the backend. Existing behavior is unchanged (default type parameter is `OllamaClient`); this enables unit testing of the phase loop without a real Ollama server

---

## [0.6.1] - 2026-02-19

### Added

- **`/watch` command — auto-refresh context files** — context files are now re-read from disk before every LLM call by default, so the model always sees the latest on-disk state after file operations are applied. Toggle with `/watch`; enabled by default.

### Fixed

- **Pasted content overwrote context bar** — when pasting text (especially large files) into the REPL, the backward cursor movement used the post-paste input length instead of the pre-paste screen position, causing `\x1b[{}D` to overshoot past the start of the input and overwrite the prompt and context bar. The fix captures the screen cursor byte offset before mutating the input buffer and uses that for the initial backward movement.

---

## [0.6.0] - 2026-02-19

### Added

- **Bracketed paste support** — multi-line text pasted into the REPL is now handled as a single event instead of character-by-character, preventing garbled input
- **Context bar in prompt** — the token count in the prompt line is now rendered as a visual fill bar (`█░░░ 12% (1024/8192t)`) so context pressure is visible at a glance
- **Template `--output` / `-o` flag** — template commands (e.g. `/c-improve`, `/review`) now accept an output file path; the response is written there after generation. If the flag is omitted, an interactive prompt asks whether to save
- **Cross-project file writes** — file operations that target absolute paths (e.g. a file loaded via `@file` from another project) are now allowed with user confirmation, instead of being silently blocked. Relative-path traversal protection is unchanged; `.git/` writes are still blocked for all paths
- **`/explain` template via `slab init`** — `slab init` now writes an `/explain` template to `.slab/templates/explain.yaml` alongside the other project templates
- **`c-improve` function mapping output** — the `/c-improve` template now emits a second fenced block written to `.slab/reports/function-map.md` listing functions that were split, renamed, added, or removed

### Fixed

- **`/c-improve` cross-project auto-save was silently blocked** — when running slab from project A and refactoring a file from project B (loaded via `@file`), the safety check rejected the absolute output path and printed "All file operations failed safety checks" without displaying a save prompt. Absolute paths now pass the safety check and reach the confirmation UI
- **`c-improve` template format example was malformed** — `{{content}}` in the fenced code block example rendered as an empty string (`c:` instead of `c:/path/to/file.c`), making the format instruction ambiguous to the model. Replaced with a literal example path and an explicit instruction to use the source file's path
- **`@file` and `/add` paths now resolve from the working directory** — relative paths supplied by the user (e.g. `@src/main.rs`) are now resolved from the directory where slab was launched (`initial_cwd`) rather than the `.slab/` project root, fixing cases where the two differ
- **Stale `test_load_defaults` assertion** — the `/explain` template was removed from the hardcoded built-in defaults (moved to `slab init`) but the unit test still asserted its presence; assertion removed

### Changed

- **`/explain` is no longer a hardcoded built-in template** — it is now written to disk by `slab init` alongside `c-to-rust`, `c-improve`, and `analyze`, keeping built-in defaults minimal
- **README Auto-Apply section** — corrected the inaccurate claim that safety checks "prevent operations outside the project root"; the restriction now applies only to relative paths

---

## [0.5.5] - 2026-01-xx

- Fix streaming output

## [0.5.4] - 2026-01-xx

- Add RELEASING.md with release process docs

## [0.5.3] - 2026-01-xx

- Bump version

## [0.5.2] - 2026-01-xx

- Cargo fmt

## [0.5.1] - 2026-01-xx

- Earlier releases
