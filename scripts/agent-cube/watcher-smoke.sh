#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo test --locked --test contracts bounded_watcher_refreshes_then_explicitly_unwatches -- --exact
cargo test --locked --test contracts watcher_registry_lists_metadata_tracks_create_rename_delete_and_stops_all -- --exact
exec cargo test --locked --test contracts forced_stop_all_signals_a_separate_watcher_process_and_it_cleans_up -- --exact
