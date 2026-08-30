#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo test --locked --test contracts benchmark
exec cargo test --locked --test examples_matrix benchmark
