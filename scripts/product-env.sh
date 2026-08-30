#!/usr/bin/env bash

configure_tomlsmith_benchmark_products() {
  local benchmark_root=$1

  configure_product() {
    local environment_name=$1
    local executable_path=$2
    if [[ -z "${!environment_name:-}" && -x "$executable_path" ]]; then
      export "$environment_name=$executable_path"
    fi
  }

  configure_product TOMLSMITH_BIN "$benchmark_root/.tools/bin/tomlsmith"
  configure_product TOMLSMITH_TOMBI_BIN "$benchmark_root/.tools/bin/tombi"
  configure_product TOMLSMITH_TAPLO_BIN "$benchmark_root/.tools/bin/taplo"
  configure_product TOMLSMITH_PRETTIER_BIN \
    "$benchmark_root/tools/prettier/node_modules/.bin/prettier"
  configure_product TOMLSMITH_DPRINT_BIN "$benchmark_root/.tools/bin/dprint"
  configure_product TOMLSMITH_BURNTSUSHI_TOMLV_BIN \
    "$benchmark_root/.tools/bin/burntsushi-tomlv"
  configure_product TOMLSMITH_GO_TOMLL_BIN \
    "$benchmark_root/.tools/bin/go-toml-tomll"

  if [[ -z "${TOMLSMITH_GO_BIN:-}" ]] && command -v go >/dev/null 2>&1; then
    TOMLSMITH_GO_BIN=$(command -v go)
    export TOMLSMITH_GO_BIN
  fi

  if [[ -n "${TOMLSMITH_PRETTIER_BIN:-}" \
    && -z "${TOMLSMITH_BENCH_NODE_COMMAND:-}" ]] \
    && command -v node >/dev/null 2>&1; then
    TOMLSMITH_BENCH_NODE_COMMAND=$(command -v node)
    export TOMLSMITH_BENCH_NODE_COMMAND
  fi

  if [[ -z "${TOMLSMITH_PRETTIER_PLUGIN:-}" \
    && -d "$benchmark_root/tools/prettier/node_modules/prettier-plugin-toml" ]]; then
    TOMLSMITH_PRETTIER_PLUGIN=$(cd "$benchmark_root/tools/prettier" \
      && node -p "require.resolve('prettier-plugin-toml')")
    export TOMLSMITH_PRETTIER_PLUGIN
  fi

  export DPRINT_CACHE_DIR=${DPRINT_CACHE_DIR:-"$benchmark_root/.tools/dprint-cache"}
}
