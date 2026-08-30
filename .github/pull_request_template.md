## Problem

What agent workflow, unsafe text edit or structural gap does this address?

## Changes

- Describe the implementation.

## Safety and refusal cases

- List refusal cases and validation boundaries.

## Evidence

```text
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
scripts/smoke.sh
```

## Token/latency impact

Describe measured impact when the change affects output size or query behavior.
