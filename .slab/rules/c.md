When refactoring or reviewing C code follow these rules:

REQUIREMENTS:
1. Use fixed-width integer types from stdint.h — uint8_t, uint16_t, uint32_t, uint64_t, int8_t, int16_t, int32_t, int64_t, size_t. Never use plain int, long, or unsigned for values where size matters.
2. Use bool from stdbool.h instead of int flags. Never use 0/1 integers as booleans.
3. All functions that can fail must return an explicit enum error code. Never return -1, NULL, or a magic value to signal failure without documentation.
4. Error codes must be a named enum — never raw integer literals for status returns.
5. No static or global mutable state. All state must be passed explicitly via context structs. If the C code has free-standing functions that modify global state, convert them to take a context pointer parameter.
6. All read-only pointer parameters must be const-qualified. All non-aliasing pointer parameters must use restrict.
7. Do NOT use restrict on main's argv, on array declarators (e.g. float data[MAX_N] not float data[restrict MAX_N]), or on non-pointer parameters. restrict is only valid on pointer parameters.
8. Every function must check all pointer parameters for NULL at entry before dereferencing.
9. Every array access must have a bounds check before the access. Never assume a caller-provided index is in range.
10. Cyclomatic complexity must be below 10 per function. If a function exceeds this, split it.
11. Maximum function length is 50 lines (excluding blank lines and comments). If a function exceeds this, refactor.
12. No heap allocations in hot paths. Prefer stack allocation and fixed-size buffers where performance matters.
13. All functions must have documentation comments describing: purpose, parameters (including units/ranges), return values, and any preconditions.
14. No implicit fallthrough in switch statements. Every case must end with break, return, or an explicit /* fallthrough */ comment.
15. All variables must be initialized at declaration. Never declare a variable and assign it on a separate line unless the initial value is genuinely unknown.
16. Use compound literals and designated initializers for struct initialization — never partially initialize a struct.
17. Integer arithmetic that can overflow must use explicit range checks before the operation. Do not rely on undefined behavior from signed overflow.
18. String operations must use length-bounded variants — strncpy/strncat/snprintf, never strcpy/strcat/sprintf. Always null-terminate manually when using strncpy.
19. Memory returned by malloc must be checked for NULL before use. Every malloc has a matching free. No memory is freed twice.
20. Do not cast away const. If a function needs const char * and receives char *, that is fine; the reverse is a violation.
21. Remove dead code from the original. If a variable is computed but never read, delete it. Do not preserve it with an unused attribute or comment.
22. No magic numbers in logic. All numeric constants must be named #define or enum constants with clear names.
23. Prefer explicit comparison over implicit truthiness — write if (ptr != NULL) not if (ptr), write if (count == 0) not if (!count).
24. Target standards: MISRA C:2012 and CERT C. When a rule above corresponds to a MISRA or CERT rule, prefer the more restrictive interpretation.
