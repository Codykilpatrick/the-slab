# Changelog

All notable changes to The Slab are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

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
