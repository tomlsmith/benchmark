# Contributing

Thank you for improving TomlSmith Benchmark. Keep changes reproducible, verify correctness before timing, and compare products only within the same operation.

## Setup

Install the exact published TomlSmith CLI and pinned competitor CLIs, then run the pull-request gate:

```bash
./scripts/setup-products.sh
./scripts/check.sh
```

The pull-request gate verifies the direct TomlSmith CLI on one small lane in addition to formatting, tests, Clippy, and corpus checks. Before changing product adapters or publishing measurements, run the slower cross-product verification:

```bash
./scripts/check-products.sh
```

The gate uses `cargo` from `PATH` by default; set the optional `CARGO` environment variable to an absolute path only when you need to select a specific Cargo installation.

Product correctness processes have an explicit 120-second default timeout. `check-products.sh` verifies every product in the four canonical medium-fixture publication lanes; set `TOMLSMITH_BENCH_FILTER` to verify one other exact lane or `TOMLSMITH_BENCH_FILTERS` for a space-separated set. Every benchmark invocation also verifies its selected fixture before timing. Increase the timeout explicitly for a known slow lane, and keep the effective value with the result bundle. The full measurement and publication contract is in [docs/methodology.md](docs/methodology.md).

## Product end-to-end adapters

The primary comparison boundary is a real product CLI invocation, not an in-process Rust library call. Products may be implemented in Rust, Go, TypeScript, or any other language.

Each product belongs to one or more independent lanes:

- `check`: validate one TOML document from stdin;
- `format`: format one valid TOML document from stdin to stdout.

Never rank results from different lanes together. Each timed iteration includes process startup and stdin/stdout/stderr I/O.

Every product descriptor must declare its stable ID, implementation language, official upstream, operations, TOML versions, executable environment variable, input transport, and isolation policy. The runner must use an explicitly configured absolute executable path and must not search `PATH`; companion paths such as the Prettier TOML plugin must also be absolute.

An unconfigured optional product remains visible as `skipped` with a reason. An explicitly configured but invalid product path, version, companion, or command must fail instead of falling back to another implementation.

Add tests for catalog metadata, exact command arguments, isolation, stdin/stdout behavior, and failure reporting before implementing a new adapter. Exercise the product's published CLI entry point rather than linking its internal library as a shortcut.

## Correctness verification

Product verification runs outside timed loops and must cover every supported fixture and TOML version.

- `check` must accept valid fixtures and reject invalid fixtures.
- `format` output must be UTF-8, reparse under the same TOML version, preserve the canonical semantic digest, and be byte-identical after a second formatting pass.

Keep canonical tree traversal, semantic serialization, digest calculation, reparsing, and second-pass formatting out of Criterion iterations.

## Library microbenchmarks

In-process library adapters remain secondary regression signals. Describe the exact public API and returned product, and do not present unlike document, semantic, or formatter seams as a product ranking.

Stable library benchmark IDs begin with:

```text
microbench/seam/<seam_id>/<toml-version>/<fixture-id>
microbench/format/source_to_formatted_text/<toml-version>/<fixture-id>
```

Use the public `Adapter` seam for behavior tests. Formatting adapters may produce different bytes when their policies differ, but each must satisfy its own semantic and idempotence invariants.

## Corpus changes

Fixtures are generated deterministically and checked against `fixtures/manifest.json`. Preserve explicit TOML version metadata and run:

```bash
cargo run --locked --quiet -p tomlsmith-benchmark-cli -- --root . generate
cargo run --locked --quiet -p tomlsmith-benchmark-cli -- --root . generate --check
```

Keep the corpus project-authored and synthetic: derive representative syntax patterns from the published TOML specifications without copying external configuration bytes. Inspect changed byte sizes and SHA-256 hashes, keep fixture IDs and paths unique and safe, keep tags nonempty and unique per fixture, and never include invalid fixtures in Criterion benchmarks.

Do not weaken `/fixtures/**/*.toml -text -eol` in `.gitattributes`; fixture hashes cover exact checkout bytes, including deliberate CRLF.

## Benchmark changes

The stable product benchmark ID is:

```text
e2e/<operation>/cold-stdin/<toml-version>/<fixture-id>
```

Use `std::hint::black_box`, set `Throughput::Bytes`, prepare and verify fixtures outside the timed loop, and consume only an O(1) shallow fingerprint inside it.

Run canonical benchmarks through `scripts/run-bench.sh`. Preserve invocation-directory independence and configure warm-up, measurement, sample size, result root, and the optional literal group-ID filter only through the documented `TOMLSMITH_BENCH_*` environment variables so `environment.json` records the effective settings.

Criterion measures latency and throughput. Peak RSS is collected afterward through a separate fresh-process run and saved as `peak-rss.json`; resource-meter overhead must never enter the Criterion loop.

Do not set `CARGO_PROFILE_BENCH_*`, `CARGO_INCREMENTAL`, or `CARGO_BUILD_INCREMENTAL`; those overrides invalidate the recorded benchmark profile. Preserve exact selected Cargo and Rustc commands and their verbose versions in environment capture. When a Go or Node.js product is enabled, also preserve the corresponding runtime command and version identity.

Run-ID cleanup may recursively remove staging data, but it must release only an empty owner lock with `rmdir`. A lock-release failure after publication remains a command failure.

## Documentation

Keep `README.md` and `README.zh-Hans.md` focused on how to run the benchmark and the measured results. Do not commit process documentation such as research notes, plans, or ADR drafts under `docs/`; keep those files local and ignored.

Do not add manual Markdown hard wraps or trailing double-space line breaks. Durable user-facing reference documentation such as the measurement methodology is versioned; research notes, plans, and ADR drafts remain local and ignored. This repository intentionally has no repository-wide Prettier dependency or pnpm scripts for Markdown formatting; the pinned Prettier installation under `tools/prettier/` exists only as a benchmarked product.

## Commit style

Use focused commits with imperative subjects, for example `bench: add a product adapter` or `test: strengthen product verification`.
