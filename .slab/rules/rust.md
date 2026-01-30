# Rust Coding Rules

- Use `thiserror` for custom error types
- No `unwrap()` or `expect()` in production code - use proper error handling
- Use `tracing` for logging instead of `println!`
- Prefer `&str` over `String` for function parameters when ownership isn't needed
