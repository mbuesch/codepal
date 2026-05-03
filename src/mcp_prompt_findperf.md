Search for **performance improvement opportunities**.

Your goal is to find code that could be rewritten to run faster or use less memory, with a clear and concrete benefit.

**Memory vs. speed tradeoffs**:
- Identify cases where memory usage can be reduced without meaningfully impacting speed.
- Identify cases where using more memory (e.g. caching, lookup tables, precomputation, etc...) would meaningfully increase speed.
- Present both sides so the user can choose.

**Algorithm improvements**:
- Check whether a better-performing algorithm exists for the task (e.g. O(n log n) vs O(n^2), hash map vs linear scan, binary search vs sequential search, etc...).
- For each candidate, state the asymptotic complexity of both the current and the proposed algorithm.
- Only suggest an algorithm change if the improvement is clear and concrete.

**Bit tricks and low-level optimizations**:
- Suggest bit manipulation tricks (e.g. power-of-two checks, popcount, trailing-zero count, bitmask operations, etc...), but only if they provide a measurable performance benefit over the current code.
- Do not suggest bit tricks for the sake of cleverness; the improvement must be justifiable.

**Hardware- or condition-specific optimizations** (suggest only when performance would *greatly* benefit):
- SIMD / vectorized operations (SSE, AVX, NEON, etc...).
- CPU cache locality improvements (data layout, prefetching, loop tiling).
- Branch-prediction-friendly restructuring.
- Platform-specific intrinsics or compiler hints.
- Always clearly state which hardware, OS, or runtime condition the optimization requires.

**Memory footprint** (only when large memory use is observed):
- If code uses large amounts of memory, suggest concrete alternatives with lower footprint.
- Only suggest them if the reduction would not noticeably decrease performance.
- Quantify the expected savings where possible.

**Rules**:
- Do not change code for the sake of being more clever or idiomatic; there must be a clear, concrete performance benefit.
- Do not report style issues, refactoring suggestions, or anything that is not a performance improvement.
- Do not run the software build, linters, or tests.
- Ignore code that is not performance-sensitive (e.g. initialization code, error handling, etc...), unless explicitly asked to analyze it.
- For each finding, state:
  1. **Location** (file, line range, function/method name).
  2. **Current behavior** and why it is suboptimal.
  3. **Proposed improvement** with concrete reasoning (complexity, memory, latency, etc.).
  4. **Tradeoffs** (e.g. increased code complexity, hardware dependency, memory increase).
  5. **Severity** of the opportunity: low / medium / high.

Write a detailed report to the file `PERF_HUNT_$DATE.md` where `$DATE` is the current date in `YYYY-MM-DD` format.

Find performance improvement opportunities in:
