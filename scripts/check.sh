#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo_command=${CARGO:-cargo}
cd "$repo_root"

tomlsmith_bin="$repo_root/.tools/bin/tomlsmith"
if [[ ! -x "$tomlsmith_bin" ]]; then
  echo "the TomlSmith native CLI is missing; run scripts/setup-products.sh" >&2
  exit 2
fi
export TOMLSMITH_BIN=${TOMLSMITH_BIN:-"$tomlsmith_bin"}
unset TOMLSMITH_TOMBI_BIN TOMLSMITH_TAPLO_BIN TOMLSMITH_PRETTIER_BIN
unset TOMLSMITH_PRETTIER_PLUGIN TOMLSMITH_DPRINT_BIN
unset TOMLSMITH_BURNTSUSHI_TOMLV_BIN TOMLSMITH_GO_TOMLL_BIN

"$cargo_command" fmt --all -- --check
"$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- --root "$repo_root" generate --check
TOMLSMITH_BENCH_FILTER=e2e/check/cold-stdin/1.0/v1_0_small \
  "$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- --root "$repo_root" verify
"$cargo_command" test --workspace --locked
"$cargo_command" clippy --workspace --all-targets --all-features --locked -- -D warnings
TOMLSMITH_BENCH_WARMUP_SECS=0.05 \
TOMLSMITH_BENCH_MEASUREMENT_SECS=0.05 \
TOMLSMITH_BENCH_SAMPLE_SIZE=10 \
TOMLSMITH_BENCH_FILTER=v1_0_small \
  "$cargo_command" bench --locked -p tomlsmith-benchmark --bench competitors -- --test
