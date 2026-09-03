#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo_command=${CARGO:-cargo}
rustc_command=${RUSTC:-rustc}
cd "$repo_root"

source "$repo_root/scripts/product-env.sh"
configure_tomlsmith_benchmark_products "$repo_root"

skip_peak_rss=${TOMLSMITH_BENCH_SKIP_PEAK_RSS:-0}
time_command=${TOMLSMITH_BENCH_TIME_COMMAND:-/usr/bin/time}
case "$time_command" in
  /*) ;;
  *)
    echo "TOMLSMITH_BENCH_TIME_COMMAND must be an absolute path: $time_command" >&2
    exit 2
    ;;
esac
if [[ "$skip_peak_rss" != 1 && ! -x "$time_command" ]]; then
  echo "peak RSS resource meter is not executable: $time_command (set TOMLSMITH_BENCH_SKIP_PEAK_RSS=1 to skip)" >&2
  exit 1
fi
export TOMLSMITH_BENCH_TIME_COMMAND=$time_command
export TOMLSMITH_BENCH_FILTER=${TOMLSMITH_BENCH_FILTER:-"e2e/check/cold-stdin/1.0/v1_0_medium"}
if [[ "$TOMLSMITH_BENCH_FILTER" =~ ^e2e/(check|format)/cold-stdin/(1\.0|1\.1)/([A-Za-z0-9._-]+)$ ]]; then
  peak_rss_operation=${BASH_REMATCH[1]}
  peak_rss_fixture=${BASH_REMATCH[3]}
else
  echo "TOMLSMITH_BENCH_FILTER must select one exact e2e check or format lane" >&2
  exit 2
fi
peak_rss_samples=${TOMLSMITH_BENCH_PEAK_RSS_SAMPLES:-3}

if [[ $# -gt 0 ]]; then
  run_id=$1
  shift
else
  run_id=$(date -u +%Y%m%dT%H%M%SZ)
fi
if [[ $# -ne 0 ]]; then
  echo "run-bench.sh does not accept Criterion arguments; use documented TOMLSMITH_BENCH_* environment variables" >&2
  exit 2
fi
result_setting=${TOMLSMITH_BENCH_RESULT_ROOT:-results}

case "$run_id" in
  *[!A-Za-z0-9._-]* | "")
    echo "run id must contain only letters, numbers, dot, underscore, or hyphen" >&2
    exit 2
    ;;
esac

case "$result_setting" in
  /*) result_root=$result_setting ;;
  *) result_root="$repo_root/$result_setting" ;;
esac

mkdir -p "$result_root"

run_directory="$result_root/$run_id"
lock_directory="$result_root/.${run_id}.lock"
if ! mkdir "$lock_directory" 2>/dev/null; then
  echo "run id is locked by another invocation: $lock_directory" >&2
  exit 2
fi
staging_directory=""
cleanup_run() {
  if [[ -n "$staging_directory" && -d "$staging_directory" ]]; then
    rm -rf -- "$staging_directory" || true
  fi
  if [[ -n "$lock_directory" && -d "$lock_directory" ]]; then
    rmdir "$lock_directory" 2>/dev/null || true
  fi
}
trap cleanup_run EXIT
trap 'exit 130' HUP INT TERM

if [[ -e "$run_directory" ]]; then
  echo "result directory already exists: $run_directory" >&2
  exit 2
fi
staging_directory=$(mktemp -d "$result_root/.${run_id}.staging.XXXXXX")
mkdir -p "$staging_directory/criterion"

export TOMLSMITH_BENCH_RESULT_ROOT="$result_setting"
export TOMLSMITH_BENCH_CARGO_COMMAND="$cargo_command"
export TOMLSMITH_BENCH_RUSTC_COMMAND="$rustc_command"

"$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- --root "$repo_root" generate --check
"$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- --root "$repo_root" list --json > "$staging_directory/catalog.json"
"$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- --root "$repo_root" verify --json > "$staging_directory/verification.json"
"$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- --root "$repo_root" env --json > "$staging_directory/environment.json"

CRITERION_HOME="$staging_directory/criterion" \
  "$cargo_command" bench --locked -p tomlsmith-benchmark --bench competitors \
  2>&1 | tee "$staging_directory/criterion.log"

(
  cd "$staging_directory"
  find criterion -type f -path '*/new/raw.csv' -print | sort > csv-files.txt
)
if [[ ! -s "$staging_directory/csv-files.txt" ]]; then
  echo "benchmark produced no Criterion raw.csv files; check TOMLSMITH_BENCH_FILTER" >&2
  exit 1
fi

if [[ "$skip_peak_rss" == 1 ]]; then
  # Diagnostic lanes without a GNU time meter publish an explicit marker so
  # aggregation never mistakes a skipped sample for a missing bundle.
  printf '{"skipped": true, "reason": "TOMLSMITH_BENCH_SKIP_PEAK_RSS=1"}\n' > "$staging_directory/peak-rss.json"
else
  env -u TOMLSMITH_BENCH_FILTER \
    "$cargo_command" run --locked --quiet -p tomlsmith-benchmark-cli -- \
    --root "$repo_root" peak-rss \
    --fixture "$peak_rss_fixture" \
    --operation "$peak_rss_operation" \
    --samples "$peak_rss_samples" \
    --json > "$staging_directory/peak-rss.json"
fi

mv "$staging_directory" "$run_directory"
staging_directory=""
if ! rmdir "$lock_directory"; then
  echo "benchmark was published but its empty run-id lock could not be released: $lock_directory" >&2
  exit 1
fi
lock_directory=""
trap - EXIT HUP INT TERM
echo "benchmark results: $run_directory"
