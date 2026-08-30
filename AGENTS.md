# TomlSmith Benchmark agent instructions

This repository defines an end-to-end product measurement protocol, not a parser-library shootout. Read `CONTRIBUTING.md` before changing fixtures, product adapters, benchmark IDs, timed execution, result publication, or README claims.

## Workspace and product boundary

- Resolve the in-process TomlSmith adapter from the exact `tomlsmith` version in `Cargo.toml` and `Cargo.lock`; do not add a sibling-repository path or Git dependency.
- Install the TomlSmith product from the exact published `tomlsmith-cli` crate version in `scripts/setup-products.sh`; CI and local measurements must exercise that registry artifact rather than a source checkout.
- Exercise published product CLI entry points through fresh processes and stdin/stdout/stderr. Products may be written in any language; do not replace an end-to-end lane with an in-process library shortcut.
- Use explicitly configured absolute executable and companion paths, disable ambient configuration discovery, and isolate per-product configuration. A missing optional product is `skipped`; an explicitly configured invalid path, version, plugin, or command is a failure.
- Keep `check` and `format` as independent operations. Do not add a TOML-to-JSON lane or compare unlike operations and seams in one ranking.

## Fixtures

- Edit the deterministic generator in `crates/benchmark/src/corpus.rs`, then regenerate the corpus. Do not hand-edit generated TOML files or `fixtures/manifest.json`.
- Keep fixtures project-authored and synthetic from the published TOML 1.0 and 1.1 specifications; do not copy bytes from external configuration files.
- Preserve explicit TOML-version metadata, unique safe fixture IDs and paths, nonempty unique tags, byte and line counts, and SHA-256 hashes.
- Keep invalid and version-boundary fixtures in correctness verification only; timed benchmarks contain valid fixtures only.
- Preserve `/fixtures/**/*.toml -text -eol` in `.gitattributes`; hashes cover exact checkout bytes, including deliberate CRLF.

## Correctness and measurement

- Run correctness verification before timing and outside Criterion iterations. `check` must accept valid fixtures and reject applicable invalid fixtures. `format` must emit UTF-8 TOML that reparses under the same version, preserves canonical semantics, and is byte-identical after a second format pass.
- Keep fixture preparation, canonical traversal, semantic digests, reparsing, second-pass formatting, and peak-RSS sampling outside timed loops. E2E timed loops launch the real product CLI and consume only an O(1) shallow output fingerprint; library microbenchmarks use the declared public `Adapter` seam.
- Preserve the stable product ID shape `e2e/<operation>/cold-stdin/<toml-version>/<fixture-id>`.
- Run publishable measurements only through `scripts/run-bench.sh`, select one exact e2e lane with `TOMLSMITH_BENCH_FILTER`, and configure runs only through documented `TOMLSMITH_BENCH_*` variables.
- Keep peak RSS in separate fresh processes on the same lane as latency. Leave `CARGO_PROFILE_BENCH_*`, `CARGO_INCREMENTAL`, and `CARGO_BUILD_INCREMENTAL` unset so the recorded benchmark profile remains comparable.

## Results and local state

- Treat each `results/<run-id>/` as an immutable run bundle. Use a new safe run ID, preserve the script's atomic staging and owner-lock behavior, and never overwrite another run.
- Raw `results/*`, tool caches, build output, isolation configs, and profiling data are ignored local artifacts. Preserve existing ignored state and never force-add or bulk-delete it.
- Curated charts belong in `assets/`. Publish only measured values from a complete, passing result bundle, and keep both README tables and SVG values synchronized.
- `tools/prettier/` pins a benchmarked product only; do not turn it into a repository-wide formatter or add pnpm scripts or dependencies for Markdown.

## Verification and documentation

- Run `./scripts/check.sh` before finishing a code, fixture, or adapter change. Before publishing product results, also run `./scripts/check-products.sh`. When shell scripts change, run `bash -n scripts/check.sh scripts/check-products.sh scripts/product-env.sh scripts/setup-products.sh scripts/run-bench.sh`.
- New or changed product adapters require tests for catalog metadata, exact arguments, isolation, stdin/stdout/stderr behavior, and failure reporting.
- After changing the corpus generator or metadata, run `cargo run --locked --quiet -p tomlsmith-benchmark-cli -- --root . generate`, then rerun it with `--check` and inspect the changed sizes, line counts, hashes, and line endings.
- Before publishing product results, provision missing pinned products with `./scripts/setup-products.sh`, confirm every product claimed in the result is enabled and passes verification, then measure each intended operation/version lane separately with a unique run ID.
- Keep `README.md` and `README.zh-Hans.md` focused on scenarios, commands, tables, and charts, and update them together. Durable user-facing reference documentation such as `docs/methodology.md` is versioned; keep process notes, research, plans, and ADR drafts under the ignored `docs/` paths.
- Keep each ordinary Markdown paragraph, list item, and blockquote paragraph on one physical source line. Do not add trailing double-space hard breaks.
