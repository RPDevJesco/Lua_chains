# Lua_chains Concept Test Results

## Single Execution Comparison

| Metric | Lua Chain | Hardcoded Rust | Difference |
|--------|-----------|----------------|-----------|
| Setup Time | 1,029.4µs | N/A | — |
| Execution Time (1x) | 710µs | 81.9µs | 628.1µs |
| Overhead | — | — | **766%** |

## Repeated Execution (100 iterations)

| Metric | Lua Chain | Hardcoded Rust | Ratio |
|--------|-----------|----------------|-------|
| Total Time (100x) | 74.3146ms | 303.2µs | 245x slower |
| Per-Execution Average | 743.00µs | 3.00µs | **24,667%** |

## Cost Breakdown

| Component | Cost | Notes |
|-----------|------|-------|
| Lua parsing + setup | 999.4µs | One-time cost (amortized) |
| Lua per-execution | 743.00µs | FFI + dynamic invocation overhead |
| Hardcoded per-execution | 3.00µs | Baseline (2 events + 1 middleware) |
| **Interpretation tax** | **740µs per execution** | Lua layer cost |

## Key Findings

| Finding | Value |
|---------|-------|
| EventChains pattern overhead | ~0.5µs per operation |
| Lua FFI + execution overhead | ~740µs per run |
| Setup amortization point | ~1,340 executions |
| Per-execution slowdown | **248x** |

## Setup Amortization Analysis

| Executions | Lua Total | Hardcoded Total | Break-even? |
|------------|-----------|-----------------|-------------|
| 1 | 1,739.4µs | 81.9µs | ✗ (21x slower) |
| 100 | 74,314.6µs | 303.2µs | ✗ (245x slower) |
| 1,000 | 743,999µs | 3,032µs | ✗ (245x slower) |
| 10,000 | 7,439,999µs | 30,320µs | ✗ (245x slower) |

**Note:** Setup cost is negligible; per-execution overhead dominates entirely.
