#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo_command=${CARGO:-cargo}
cd "$repo_root"

source "$repo_root/scripts/product-env.sh"
configure_tomlsmith_benchmark_products "$repo_root"

default_filters=(
  "e2e/check/cold-stdin/1.0/v1_0_medium"
  "e2e/check/cold-stdin/1.1/v1_1_medium"
  "e2e/format/cold-stdin/1.0/v1_0_medium"
  "e2e/format/cold-stdin/1.1/v1_1_medium"
)

if [[ -n "${TOMLSMITH_BENCH_FILTER:-}" ]]; then
  filters=("$TOMLSMITH_BENCH_FILTER")
elif [[ -n "${TOMLSMITH_BENCH_FILTERS:-}" ]]; then
  read -r -a filters <<< "$TOMLSMITH_BENCH_FILTERS"
else
  filters=("${default_filters[@]}")
fi

if [[ ${#filters[@]} -eq 0 ]]; then
  echo "no product correctness lanes were selected" >&2
  exit 2
fi

for filter in "${filters[@]}"; do
  echo "verifying product lane: $filter"
  TOMLSMITH_BENCH_FILTER="$filter" \
    "$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- \
      --root "$repo_root" verify
done
