#!/usr/bin/env sh
set -eu

export LC_ALL=C
export LANG=C

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUT=${CODE_MECHANIC_SMOKE_OUT:-"$ROOT/target/code-mechanic-smoke"}

case "$OUT" in
    "$ROOT"/target/*) ;;
    *) echo "smoke output must remain below target/" >&2; exit 64 ;;
esac

mkdir -p "$OUT"
rm -rf "$OUT/workspace" "$OUT/state"
mkdir -p "$OUT/workspace" "$OUT/state"
cp -R "$ROOT/fixtures/benchmark/." "$OUT/workspace/"
export CODE_MECHANIC_STATE_DIR="$OUT/state"

cargo build --release --locked
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings

BIN="$ROOT/target/release/code-mechanic"
DB="$OUT/index.sqlite"

"$BIN" capabilities >"$OUT/capabilities.json"
"$BIN" --root "$OUT/workspace" --database "$DB" index >"$OUT/index.json"
"$BIN" --root "$OUT/workspace" --database "$DB" diagnostics >"$OUT/diagnostics.json"

for SPEC in \
    'rust_complex:src/examples.rs:rust' \
    'c_complex:native/examples.c:c' \
    'goComplex:cmd/tool/main.go:go' \
    'cppComplex:native/examples.cpp:cpp' \
    'renderFrame:native/examples.m:objective-c' \
    'glslComplex:shaders/examples.frag:glsl'
do
    SYMBOL=${SPEC%%:*}
    REST=${SPEC#*:}
    FILE=${REST%%:*}
    LANGUAGE=${REST##*:}
    "$BIN" --root "$OUT/workspace" --database "$DB" \
        locate "$SYMBOL" --file "$FILE" >"$OUT/locator-$LANGUAGE.json"
done

"$BIN" --root "$OUT/workspace" --database "$DB" \
    search-body rust_complex --file src/examples.rs --pattern consume --max-results 5 \
    >"$OUT/body-search.json"

"$BIN" --root "$OUT/workspace" --database "$DB" \
    append-parameter --symbol goComplex --parameter 'enabled bool' --argument true \
    >"$OUT/append-preview.json"
PLAN_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["plan_id"])' \
    "$OUT/append-preview.json")
"$BIN" --root "$OUT/workspace" --database "$DB" \
    append-parameter --symbol goComplex --parameter 'enabled bool' --argument true \
    --apply --expect-plan "$PLAN_ID" >"$OUT/append-apply.json"

"$BIN" --root "$OUT/workspace" --database "$DB" \
    replace-body --symbol glslComplex --code 'return value * 2.0;' \
    >"$OUT/body-preview.json"
PLAN_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["plan_id"])' \
    "$OUT/body-preview.json")
"$BIN" --root "$OUT/workspace" --database "$DB" \
    replace-body --symbol glslComplex --code 'return value * 2.0;' \
    --apply --expect-plan "$PLAN_ID" >"$OUT/body-apply.json"

"$BIN" --root "$OUT/workspace" --database "$DB" watch \
    --duration-seconds 3 --until-idle-seconds 1 >"$OUT/watch.json"
"$BIN" watchers list >"$OUT/watchers.json"

"$BIN" --root "$OUT/workspace" --database "$DB" bench \
    --case rust_complex:src/examples.rs \
    --case c_complex:native/examples.c \
    --warm-runs 5 --window-lines 60 --min-token-reduction-pct 50 \
    --output "$OUT/benchmark.json" >"$OUT/benchmark-stdout.json"

python3 - "$OUT" <<'PY'
import json
from pathlib import Path
import sys

out = Path(sys.argv[1])
assert json.loads((out / "diagnostics.json").read_text()) == []
assert json.loads((out / "append-apply.json").read_text())["applied"]
assert json.loads((out / "body-apply.json").read_text())["applied"]
assert json.loads((out / "watch.json").read_text())["unwatched"]
watchers = json.loads((out / "watchers.json").read_text())
assert watchers["active"] == 0 and watchers["watchers"] == []
benchmark = json.loads((out / "benchmark.json").read_text())
assert benchmark["passed"] and benchmark["aggregate"]["token_reduction_pct"] >= 50
for language in ["rust", "c", "go", "cpp", "objective-c", "glsl"]:
    locator = json.loads((out / f"locator-{language}.json").read_text())
    assert locator["language"] == language
PY

echo "code-mechanic smoke passed"
echo "evidence: $OUT"
