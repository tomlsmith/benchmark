#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tools_root="$repo_root/.tools"
bin_dir="$tools_root/bin"
temporary_directory=$(mktemp -d)
readonly tomlsmith_cli_version=0.3.0

cleanup_setup() {
  rm -rf -- "$temporary_directory"
}
trap cleanup_setup EXIT HUP INT TERM

for command_name in cargo curl go gzip install node pnpm tar unzip; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "required setup command is unavailable: $command_name" >&2
    exit 2
  fi
done

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)
    tombi_target=aarch64-apple-darwin
    taplo_target=darwin-aarch64
    dprint_target=aarch64-apple-darwin
    ;;
  Darwin-x86_64)
    tombi_target=x86_64-apple-darwin
    taplo_target=darwin-x86_64
    dprint_target=x86_64-apple-darwin
    ;;
  Linux-aarch64 | Linux-arm64)
    tombi_target=aarch64-unknown-linux-musl
    taplo_target=linux-aarch64
    dprint_target=aarch64-unknown-linux-musl
    ;;
  Linux-x86_64)
    tombi_target=x86_64-unknown-linux-musl
    taplo_target=linux-x86_64
    dprint_target=x86_64-unknown-linux-musl
    ;;
  *)
    echo "setup-products.sh supports macOS and Linux on arm64 or x86_64" >&2
    exit 2
    ;;
esac

mkdir -p "$bin_dir" "$tools_root/dprint-cache"

curl -fsSL --retry 3 \
  "https://github.com/tombi-toml/tombi/releases/download/v1.4.1/tombi-cli-1.4.1-${tombi_target}.tar.gz" \
  -o "$temporary_directory/tombi.tar.gz"
mkdir -p "$temporary_directory/tombi"
tar -xzf "$temporary_directory/tombi.tar.gz" -C "$temporary_directory/tombi"
tombi_binary=$(find "$temporary_directory/tombi" -type f -name tombi -print -quit)
if [[ -z "$tombi_binary" ]]; then
  echo "the Tombi release archive did not contain a tombi executable" >&2
  exit 1
fi
install -m 755 "$tombi_binary" "$bin_dir/tombi"

curl -fsSL --retry 3 \
  "https://github.com/tamasfe/taplo/releases/download/0.10.0/taplo-${taplo_target}.gz" \
  -o "$temporary_directory/taplo.gz"
gzip -dc "$temporary_directory/taplo.gz" > "$temporary_directory/taplo"
install -m 755 "$temporary_directory/taplo" "$bin_dir/taplo"

curl -fsSL --retry 3 \
  "https://github.com/dprint/dprint/releases/download/0.56.1/dprint-${dprint_target}.zip" \
  -o "$temporary_directory/dprint.zip"
mkdir -p "$temporary_directory/dprint"
unzip -q "$temporary_directory/dprint.zip" -d "$temporary_directory/dprint"
dprint_binary=$(find "$temporary_directory/dprint" -type f -name dprint -print -quit)
if [[ -z "$dprint_binary" ]]; then
  echo "the dprint release archive did not contain a dprint executable" >&2
  exit 1
fi
install -m 755 "$dprint_binary" "$bin_dir/dprint"

mkdir -p "$temporary_directory/go-bin"
GOBIN="$temporary_directory/go-bin" go install \
  github.com/BurntSushi/toml/cmd/tomlv@v1.6.0
GOBIN="$temporary_directory/go-bin" go install \
  github.com/pelletier/go-toml/v2/cmd/tomll@v2.4.3
install -m 755 "$temporary_directory/go-bin/tomlv" "$bin_dir/burntsushi-tomlv"
install -m 755 "$temporary_directory/go-bin/tomll" "$bin_dir/go-toml-tomll"

cargo install tomlsmith-cli \
  --version "=${tomlsmith_cli_version}" \
  --locked \
  --root "$temporary_directory/tomlsmith-cli"
install -m 755 "$temporary_directory/tomlsmith-cli/bin/tomlsmith" "$bin_dir/tomlsmith"

pnpm install --dir "$repo_root/tools/prettier" --frozen-lockfile

dprint_config="$temporary_directory/dprint.json"
printf '{\n  "plugins": ["https://plugins.dprint.dev/toml-0.8.0.wasm"]\n}\n' > "$dprint_config"
printf 'name="cache-prime"\n' | \
  DPRINT_CACHE_DIR="$tools_root/dprint-cache" \
  "$bin_dir/dprint" fmt \
    --config "$dprint_config" \
    --config-discovery=false \
    --log-level silent \
    --stdin fixture.toml >/dev/null

"$bin_dir/tomlsmith" --version
"$bin_dir/tombi" --version
"$bin_dir/taplo" --version
"$repo_root/tools/prettier/node_modules/.bin/prettier" --version
"$bin_dir/dprint" --version
go version -m "$bin_dir/burntsushi-tomlv"
go version -m "$bin_dir/go-toml-tomll"
