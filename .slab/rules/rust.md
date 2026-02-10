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
9.  NEVER use `OnceLock`, `Lazy`, `static mut`, `Rc`, `RefCell`, or any global/static storage. Encapsulate ALL state into owned structs with methods. If the C code has free-standing functions that operate on global state, convert them to methods on the struct - do NOT create wrapper functions with hidden global state. The caller is responsible for creating and owning the struct instance.
10. Do NOT include unused imports - only import what is actually used in the code.
11. Use `match` expressions for branching instead of if/else chains where possible.
12. Use iterators (`.iter()`, `.map()`, `.filter()`, `.collect()`) instead of manual loops where possible.
13. Keep code concise - avoid verbose getter/setter boilerplate when direct public field access or builder patterns would be more idiomatic.
14. NEVER negate unsigned integers directly. Cast to the signed type BEFORE negating: use `-(x as i32)` not `-(x)` where x is u32. For zigzag decoding use wrapping operations like `.wrapping_neg()` or explicit casts.
15. C `union` types must be translated to Rust `enum` variants with data, NOT Rust `union`. Avoid `#[repr(C, packed)]` - instead model the wire format with serialization/deserialization methods that read and write byte slices.
16. Never return `&mut T` references from methods that also require `&mut self` - this causes multiple mutable borrow errors. Instead return indices or owned handles, and provide separate methods to access elements by index.
17. C `#define` integer constants used as type tags/categories (e.g., `#define TYPE_FOO 1`) MUST become Rust enums with `Display` impl. Never use raw integers with magic numbers for classification.
18. Remove dead code from the C original. If a variable is computed but never read (like a running total that's never used), delete it — do NOT preserve it with an underscore prefix.
19. Choose Rust-native types. Don't mechanically map C `int` to `i32`. Use `usize` for counts, indices, and sizes. Use `f32`/`f64` for thresholds that are compared against floats. Use the type that makes casts unnecessary at usage sites.
20. Convert C output-parameter patterns (`void foo(float *out, int len)`) to Rust return values (`fn foo() -> Vec<f32>`). Functions should return owned data, not write to caller-provided buffers.
21. Methods that don't read or write `self` must not take `self`. Make them associated functions (`fn foo() -> T`, no self) or free functions. Never add `&mut self` just because a method lives in an `impl` block.
22. Do not carry over C `#define MAX_FOO` buffer sizes as hardcoded allocations. Size containers dynamically to actual input (e.g., `Vec::with_capacity(input.len())`). If a limit is needed, make it a parameter or config field.
23. Prefer struct literal initialization over `new()` + field-by-field mutation. Build the complete struct in one expression.
24. Implement the `Default` trait for any type that has a `new()` taking no arguments. Derive it when field defaults match Rust's defaults (0, false, None); implement manually otherwise.
25. Never use `.unwrap()` or `.expect()` in library code. Use `?` with proper error types, or validate inputs at function entry and return `Result`. Panicking conversions like `.try_into().unwrap()` indicate a wrong type choice (see rule 20).
26. Doc comments must accurately describe the Rust function signature. Do not copy C comments verbatim if the return type or parameters changed. If the C function returned `int` count and the Rust function returns `Vec<T>`, update the docs.
27. Write tests that exercise core logic, not just constructors. Include: a basic happy-path test with known input/output, at least one edge case (empty input, boundary values), and a test that verifies error/failure paths.