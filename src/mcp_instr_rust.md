# **MANDATORY** Rust impl rules

- crates.io: Latest versions
- **ALWAYS** `vec![]` instead of `Vec::new()`
- Cloning `Arc`? **ALWAYS** `Arc::clone(&arc)` instead of `arc.clone()`
- Multiple items from same crate? **ALWAYS** Single `use` + curly braces
- `mod` after `use`
- `use` at top of file
- Avoid `unwrap()`
- Unwrap lock guard? **ALWAYS** `expect("Lock poisoned")` instead of `unwrap()`
- `use` rather than crate-root paths
- libc syscalls? `nix` crate instead
- **ALWAYS** `cargo clippy` after changes, fix warnings
- Then **ALWAYS** `cargo fmt`
