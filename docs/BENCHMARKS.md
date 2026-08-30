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

## Kotlin Parity Result

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

## Reference Results

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
