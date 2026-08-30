#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo fmt --all -- --check
exec cargo clippy --all-targets --locked -- -D warnings
