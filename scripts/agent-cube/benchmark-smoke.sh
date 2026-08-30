#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo test --locked --test contracts benchmark
exec cargo test --locked --test examples_matrix multi_case_benchmark_reports_clear_per_case_and_aggregate_savings -- --exact
