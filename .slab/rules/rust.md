When translating C to Rust follow the following rules:

REQUIREMENTS:
1. NO `unsafe` blocks - the code must be 100% safe Rust
2. NO global/static mutable state - use structs with methods instead
3. Use `Result<T, E>` for all operations that can fail
4. Use `Option<T>` for nullable values
5. Use iterators instead of manual loops where possible
6. Follow Rust naming conventions (snake_case for functions/variables, CamelCase for types)
7. Add appropriate derive macros (Debug, Clone, etc.) where useful
8. The code must compile with `cargo build` without warnings
9. The code must pass `cargo clippy` with no warnings
10. NEVER use `OnceLock`, `Lazy`, `static mut`, `Rc`, `RefCell`, or any global/static storage. Encapsulate ALL state into owned structs with methods. If the C code has free-standing functions that operate on global state, convert them to methods on the struct - do NOT create wrapper functions with hidden global state. The caller is responsible for creating and owning the struct instance.
11. Do NOT include unused imports - only import what is actually used in the code.
12. Use `match` expressions for branching instead of if/else chains where possible.
13. Use iterators (`.iter()`, `.map()`, `.filter()`, `.collect()`) instead of manual loops where possible.
14. Keep code concise - avoid verbose getter/setter boilerplate when direct public field access or builder patterns would be more idiomatic.
15. NEVER negate unsigned integers directly. Cast to the signed type BEFORE negating: use `-(x as i32)` not `-(x)` where x is u32. For zigzag decoding use wrapping operations like `.wrapping_neg()` or explicit casts.
16. C `union` types must be translated to Rust `enum` variants with data, NOT Rust `union`. Avoid `#[repr(C, packed)]` - instead model the wire format with serialization/deserialization methods that read and write byte slices.
17. Never return `&mut T` references from methods that also require `&mut self` - this causes multiple mutable borrow errors. Instead return indices or owned handles, and provide separate methods to access elements by index.

C Header:
```c
{c_header}
```

C Source:
```c
{c_code}
```