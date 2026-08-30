#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo test --locked --test contracts rust_rename_is_previewed_plan_bound_and_ast_only -- --exact
cargo test --locked --test contracts c_rename_updates_prototype_definition_and_call_but_not_comments -- --exact
cargo test --locked --test contracts parameter
cargo test --locked --test contracts replace_body
cargo test --locked --test contracts injection
exec cargo test --locked --test contracts stale_preview
