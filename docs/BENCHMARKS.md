# Benchmark Methodology

Code Mechanic ships a local benchmark because token savings should be measured,
not inferred from byte counts.

## Three Payloads

For each unique function case, `bench` records:

1. **baseline** — an exact-word workspace scan plus one targeted source window;
2. **indexed** — the exact raw function returned by `symbol --raw`; and
3. **locator** — compact fresh function/signature/body spans without source.

The baseline must contain the exact indexed function. The locator's byte span
must reconstruct that same function from the fresh file. A case cannot pass on
token reduction without answer equivalence and locator exactness.

All payloads use the local `o200k_base` tokenizer from `tiktoken-rs`. No API key,
network request or estimated bytes-per-token ratio is involved.

## Run It On Your Workload

```sh
code-mechanic --root . index --reconcile --force-hash
code-mechanic --root . bench \
  --case small_function:src/small.rs \
  --case large_function:native/runtime.cpp \
  --warm-runs 20 \
  --window-lines 120 \
  --min-token-reduction-pct 0 \
  --output target/code-mechanic-benchmark.json
```

## v0.2.0 Cross-Language Result

The retained fixture corpus contains an easy and a complex case for every
supported language family. All 14 cases must parse without diagnostics, prove
exact-answer and locator-range equivalence, and return fewer exact-source tokens
than the baseline. The seven complex cases additionally require the locator to
be smaller than the exact function source. The integration test enforces at
least 60% aggregate exact-source reduction.

The 20-run, 120-line evidence is retained in
[`benchmarks/all-languages-v0.2.0.json`](benchmarks/all-languages-v0.2.0.json).
Aggregating each language's easy and complex case gives:

| Language | Baseline tokens | Exact source | Locator | Exact reduction | Locator vs source |
| --- | ---: | ---: | ---: | ---: | ---: |
| Rust | 1,372 | 197 | 154 | 85.64% | 21.83% |
| C | 1,131 | 176 | 154 | 84.44% | 12.50% |
| Go | 532 | 144 | 154 | 72.93% | -6.94% |
| C++ | 503 | 189 | 156 | 62.43% | 17.46% |
| Objective-C | 482 | 164 | 156 | 65.98% | 4.88% |
| GLSL | 613 | 236 | 164 | 61.50% | 30.51% |
| Kotlin | 2,156 | 269 | 172 | 87.52% | 36.06% |
| **All 14 cases** | **6,789** | **1,375** | **1,110** | **79.75%** | **19.27%** |

The mixed locator total is deliberately modest because each language's tiny
function is cheaper to return directly than to describe. For the seven complex
cases—the intended locator-first workload—the baseline is 3,441 tokens, exact
source is 1,269 tokens, and locators are 557 tokens: 63.12% less than baseline
for exact retrieval, then another 56.11% less when only locations are needed.

| Language | Complex case | Baseline | Exact source | Locator | Exact reduction | Locator vs source |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Rust | `rust_complex` | 680 | 180 | 78 | 73.53% | 56.67% |
| C | `c_complex` | 569 | 161 | 78 | 71.70% | 51.55% |
| Go | `goComplex` | 282 | 129 | 77 | 54.26% | 40.31% |
| C++ | `cppComplex` | 263 | 174 | 78 | 33.84% | 55.17% |
| Objective-C | `renderFrame` | 247 | 151 | 78 | 38.87% | 48.34% |
| GLSL | `glslComplex` | 324 | 218 | 82 | 32.72% | 62.39% |
| Kotlin | `kotlinComplex` | 1,076 | 256 | 86 | 76.21% | 66.41% |

These are fixture results, not a universal billing claim. In particular, the
smaller C++, Objective-C and GLSL files leave less irrelevant source to remove,
so their exact-source reduction is lower even though their complex locators
save 48–62% versus returning the full function.

## Kotlin Detail

The retained Kotlin fixture contains expression-body, extension, generic,
suspend, higher-order, collection-pipeline, retry/error and trailing-lambda
shapes surrounded by enough realistic source to make retrieval differences
visible. The 20-run, 120-line result retained in
[`benchmarks/kotlin-v0.2.0.json`](benchmarks/kotlin-v0.2.0.json) is:

| Case | Baseline tokens | Exact function | Locator | Exact reduction | Locator vs body |
| --- | ---: | ---: | ---: | ---: | ---: |
| `kotlinEasy` | 1,080 | 13 | 86 | 98.80% | -561.54% |
| `kotlinComplex` | 1,076 | 256 | 86 | 76.21% | 66.41% |
| Aggregate | 2,156 | 269 | 172 | 87.52% | 36.06% |

Both cases prove exact-answer and locator-range equivalence. The tiny function
also demonstrates the honest crossover: returning its 13-token body is cheaper
than an 86-token locator. The complex function is where locator-first retrieval
earns its place.

Use `--min-token-reduction-pct` as a gate only after collecting representative
cases. The threshold applies to exact-source retrieval versus the baseline;
locator-versus-body savings are reported separately.

## Real-Repository Reference Result

The original adoption repository contained 2,009 indexed static-language files,
18,736 function/prototype symbols and 137,433 call references. A representative
Go/C++/Objective-C/GLSL set measured:

| Payload | Tokens |
| --- | ---: |
| Broad scan + source ranges | 30,659 |
| Exact function bodies | 6,092 |
| Fresh locators | 369 |

This is an 80.13% exact-body reduction against the broad baseline and a 93.94%
locator reduction against retrieving the full bodies. Locator p50 latency was
roughly 0.95–1.07 ms on the measured machine.

These results are evidence for that workload, not a universal claim. Very small
functions can be cheaper than their locator metadata. A deliberately large
fixture in this repository makes the opposite case: exact body retrieval saves
little when the answer itself is large, while a locator remains small.

## What This Does Not Measure

The benchmark does not predict total model billing. Prompts also contain tool
schemas, instructions, conversation history, reasoning and generated output. It
does not measure compiler verification or claim that its in-process baseline is
faster or slower than the optimized `rg` executable.
