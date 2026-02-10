# Citizen Style Guide

> "This code is what it is because of its citizens" — Plato

## Core Principle

Decrease net cognitive complexity for developers.

**Priority:** Readability > Writeability

---

## Writing Style

### Typography

- **Use contractions** – "can't" not "cannot"
- **Use curly quotes** – "hello" and 'world' not "hello" and 'world'
- **Use proper punctuation** – ellipsis (…), en dash (–), em dash (—)
- **Use glyphs** – ←, →, 45° instead of <-, ->, 45 degrees
- **Use canonical spelling** – Python, Node.js, PostgreSQL (not python, node, postgres)

### Comments & Documentation

- **Communicate intent** – explain why, not what
- **Show examples** when possible
- **Use sentence case** – capitalize first letter only
- **Punctuate block comments** – end with periods
- **Inline short comments** – `speed = …  # Meters per hour`

```ts
// Get a unit's globally unique identifier.
//
// ("Zumwalt-class destroyer", 3) → "Unit:ZumwaltClassDestroyer:3"
const getUnitId = (name: string, index: number) => `Unit:${pascalCase(name)}:${index}`;
```

### Documentation Patterns

- **Types:** Begin with indefinite article – "A unit."
- **Properties:** Begin with definite article – "The name of a unit."

---

## Naming Conventions

### General Rules

- **Follow language conventions** – camelCase (JS/TS), snake_case (Python), etc.
- **Favor readability over brevity** – `docker_image` not `image`
- **Group with prefixes** – `cnn_kernel`, `cnn_stride`
- **Space out affixes** – `hidden_layer_0` not `hidden_layer0`

### Abbreviations & Acronyms

- **DON'T abbreviate** – `directory` not `dir`, `response` not `res`
- **DO use well-known acronyms** – `nato_classification`, `radar_range`, `url`, `s3_bucket`
- **DON'T invent acronyms** – `aerial_unit_speed` not `au_speed`

### Type Clarity

- **Add type hints to names** – `created_date` not `created`, `was_published` not `published`
- **Differentiate types** – don't reuse variable names for different types
- **DON'T add type suffixes** – `age` not `age_int`, `confidence` not `confidence_float`

```py
# ✓ Good
url = "https://spear.ai"
parsed_url = urlparse("https://spear.ai")

# ✗ Bad
url = "https://spear.ai"
url = urlparse("https://spear.ai")
```

### Booleans

- **Use appropriate verbs** – `can_delete`, `has_feature`, `should_reset`
- **Be positive** – `is_enabled` not `is_disabled`
- **Use correct tense** – `was_suspended`, `is_suspended`

### Collections

- **DON'T pluralize** – specify collection type instead

```py
# ✓ Good
equipment_list = [{…}, {…}]
equipment = equipment_list[0]
id_set = {"a", "b", "c"}

# ✗ Bad
equipment = [{…}, {…}]
equipment = equipment[0]
ids = {"a", "b", "c"}
```

---

## Data Formats

### Angles

- **Prefer degrees to radians** – easier to reason about, serialize better
- **Display with symbol** – `f"heading {heading}°"`

```ts
// ✓ Good
const turnLeft = (angle: number) => (360 + (angle - 90)) % 360;
turnLeft(0); // ⇒ 270
```


```ts
// ✓ Good
const backgroundColor = "#edf6ff";
const foregroundColor = "#006adc";
```

---

## Quick Reference

| Do | Don't |
|---|---|
| `can_delete = True` | `is_deletable = True` |
| `equipment_list = […]` | `equipment = […]` |
| `docker_image = "…"` | `image = "…"` |
| `response = request()` | `res = req()` |
| `"It's a quote"` | `"It's a quote"` |
| `// 1, 2, 3, …, 10` | `// 1, 2, 3, ..., 10` |
| `// Duration: 2–3 weeks` | `// Duration: 2-3 weeks` |
| `created_date = "…"` | `created = "…"` |
| `is_enabled = True` | `is_disabled = False` |