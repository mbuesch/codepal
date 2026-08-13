Perform a thorough **bug hunt**.

Your goal is to find *real bugs* - not style issues, not theoretical risks, but actual defects that cause incorrect behavior or undefined states.

Look for, but do not limit yourself to:

- **Logic bugs**: Incorrect conditions, off-by-one errors, wrong operator, inverted predicates, wrong variable used, etc... .
- **Coding issues**: Incorrect algorithm implementation, missing edge-case handling, incorrect return value, missed error check, etc... .
- **Interface misuse**: Calling APIs with wrong argument order, wrong argument type/range, wrong flags, wrong calling convention, incorrect assumption about return value semantics, etc... .
- **Dependency misuse**: Using a library function contrary to its documented contract, ignoring documented preconditions or invariants, relying on undocumented behavior, etc... .
- **OS interface misuse**: Incorrect syscall usage, wrong errno handling, incorrect signal handling, misuse of file descriptors, incorrect memory-mapped I/O, wrong ioctl arguments, etc... .
- **Service/protocol misuse**: Incorrect use of network protocols, wrong message ordering, missing handshake steps, incorrect framing or parsing, etc... .
- **Self-contradictions**: Code that contradicts other code, a comment, a doc-comment, documentation, or a README, etc... .
- **Documentation bugs**: Doc-comments or documentation that describe behavior the code does not implement or implements incorrectly, incorrect parameter descriptions, wrong return value descriptions, etc... .
- **Multithreading bugs**: Data races, incorrect use of atomics (wrong ordering), deadlocks, livelocks, incorrect condition-variable usage, missing memory barriers, etc... .
- **Interrupt synchronization / signal synchronization bugs**: Async-signal-unsafe functions called from signal handlers, incorrect use of `volatile` or atomics for interrupt-shared state, missing critical-section protection, incorrect use of locks, mutexes, semaphores, etc... .
- **Concurrency correctness**: Incorrect lock granularity, incorrect unlock order, missing lock on shared state, etc... .
- **Asynchronous bugs**: Incorrect use of async/await, incorrect use of futures, incorrect use of async runtimes, etc... .
- **Unsound code**: Safe abstractions that can be used to trigger Undefined Behavior without Rust `unsafe` on the caller side, violated aliasing rules, invalid pointer provenance, misuse of Rust `transmute`, use-after-free, out-of-bounds pointer arithmetic, wrong Rust `unsafe impl` for `Send`/`Sync`/`Unpin`, incorrect lifetime annotations that enable use-after-free, etc... .
- **Resource leaks**: Memory leaks, file descriptor leaks, lock not released on all paths, etc... .
- **Integer bugs**: Overflow, underflow, truncation, sign-extension errors, incorrect widening/narrowing cast, unnecessary casts, etc... .

For each finding:
1. State the **location** (file, line range, function/method name).
2. Give a **precise description** of why it is a bug and what incorrect behavior it causes or can cause.
3. Estimate the **severity** (e.g. low, medium, high, critical).

Do not report style issues, performance suggestions, or anything that is merely "could be better".
Do not run the software build, linters, or tests.

Write a detailed report to the file `BUG_HUNT_YYYY-MM-DD_hh-mm-ss.md` where `YYYY-MM-DD_hh-mm-ss` is the current date and time.
Report format:

```
# Bug hunt report

- Date: {YYYY-MM-DD}
- Scanned files: {file list}

## Finding {1}: {Brief summary}

- Severity: {High, Medium, Low}
- Files: {affected files}

{Very detailed description}

{Code snippets and code references relevant for understanding the finding}

## Finding {2}: {Brief summary}

...

## Finding {n}: {Brief summary}

...

## Summary

{Table with brief descriptions of the findings}
```

Find bugs in:
$(WHERE)
