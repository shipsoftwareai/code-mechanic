#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUT=${CODE_MECHANIC_KOTLIN_OUT:-"$ROOT/target/code-mechanic-kotlin"}

case "$OUT" in
    "$ROOT"/target/*) ;;
    *) echo "Kotlin evidence must remain below target/" >&2; exit 64 ;;
esac

cd "$ROOT"
mkdir -p "$OUT"

cargo test --locked language::tests::kotlin
cargo test --locked --test contracts kotlin
cargo test --locked --test examples_matrix kotlin
cargo build --locked

BIN="$ROOT/target/debug/code-mechanic"
DB="$OUT/index.sqlite"
"$BIN" --root fixtures/benchmark --database "$DB" index >"$OUT/index.json"
"$BIN" --root fixtures/benchmark --database "$DB" diagnostics >"$OUT/diagnostics.json"
"$BIN" --root fixtures/benchmark --database "$DB" bench \
    --case kotlinEasy:kotlin/examples.kt \
    --case kotlinComplex:kotlin/examples.kt \
    --warm-runs 10 --window-lines 120 --min-token-reduction-pct 70 \
    --output "$OUT/benchmark.json" >"$OUT/benchmark-stdout.json"

python3 - "$OUT" <<'PY'
import json
from pathlib import Path
import sys

out = Path(sys.argv[1])
index = json.loads((out / "index.json").read_text())
assert index["parse_failures"] == 0
assert json.loads((out / "diagnostics.json").read_text()) == []
benchmark = json.loads((out / "benchmark.json").read_text())
assert benchmark["passed"]
assert benchmark["aggregate"]["cases_passed"] == 2
assert benchmark["aggregate"]["token_reduction_pct"] >= 70
complex_case = next(case for case in benchmark["cases"] if case["symbol"] == "kotlinComplex")
assert complex_case["locator_vs_full_source_reduction_pct"] > 50
PY

echo "Kotlin smoke passed"
echo "evidence: $OUT"
