## Summary

Describe the product, fixture, benchmark, documentation, or infrastructure change.

## Product coverage

- [ ] Product commands use the real executable entry point and include process startup plus stdin/stdout/stderr I/O.
- [ ] Check and format results remain in separate benchmark tables.
- [ ] Product versions and dependencies are pinned, and setup remains reproducible.
- [ ] TOML 1.0 and 1.1 support claims are backed by the product declaration or the capability verification.

## Results

- [ ] Valid and invalid fixtures still pass the applicable product verification before timing.
- [ ] Benchmark IDs identify the operation, process mode, TOML version, and fixture.
- [ ] Results record the machine, date, repository revision, tool versions, and benchmark settings.
- [ ] Peak RSS is sampled separately from Criterion timing and saved with the result bundle.
- [ ] The English and Simplified Chinese READMEs show matching commands and results.

## Repository hygiene

- [ ] Process notes under `docs/` are not tracked.
- [ ] Markdown has no manual hard wraps or trailing double-space line breaks.
- [ ] No repository-wide Prettier dependency or pnpm script for Markdown formatting was added; `tools/prettier/` remains benchmark-only.

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo run --locked --quiet -p tomlsmith-benchmark-cli -- --root . generate --check`
- [ ] `cargo run --locked --quiet -p tomlsmith-benchmark-cli -- --root . verify`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo bench --locked -p tomlsmith-benchmark --bench competitors -- --test`
- [ ] `bash -n scripts/check.sh scripts/product-env.sh scripts/setup-products.sh scripts/run-bench.sh`
