# Measurement methodology

TomlSmith Benchmark measures complete command-line products, not parser functions. A publishable result must identify the exact product distribution, operation, TOML version, fixture, machine, toolchain, benchmark source revision, package versions, and raw run directory described below.

## Comparison boundary

The primary comparison uses each product's direct CLI executable. TomlSmith is measured as the release-mode Rust `tomlsmith` binary, Tombi, Taplo, and dprint as their released native CLIs, and the Go products as their installed native executables. This keeps the cold-start boundary to one product process per row. Every table labels this boundary `TomlSmith native CLI`.

Package-manager launchers and wrapper processes are excluded from this benchmark. They add a second process boundary that the other native rows do not share and therefore are not comparable CLI implementation measurements.

Products participate only in operations and TOML versions declared by their catalog descriptors. `check` and `format` are separate comparisons. A semantic re-printer that discards comments may appear in the format table only with that workload difference stated next to the result.

## Fixtures and correctness

Fixtures are deterministic project-authored documents generated from the published TOML 1.0 and 1.1 specifications. Their IDs, versions, validity expectations, tags, byte counts, line counts, and SHA-256 hashes are locked in `fixtures/manifest.json`; Git attributes preserve exact checkout bytes.

Correctness runs before timing. Check adapters must accept applicable valid fixtures and reject applicable invalid fixtures. Format adapters must emit UTF-8, reparse under the selected version, preserve the canonical semantic digest, and produce byte-identical output on a second formatting pass. Unsupported product/version combinations are omitted rather than treated as failures or zero-cost results.

Untimed correctness, preflight, and peak-memory processes run in an operating-system process group or job and are terminated with their descendants after `TOMLSMITH_BENCH_PROCESS_TIMEOUT_SECS`. The default is 120 seconds. The standard cross-product gate checks every product that participates in the four canonical medium-fixture publication lanes. Every benchmark run also verifies its exact selected fixture before timing, so a large or stress lane cannot inherit correctness from the medium gate. Increase the timeout explicitly for a known slow lane and record the effective value with its result bundle. A timeout is recorded as a failed case with the product, operation, fixture, limit, and captured stderr rather than an implicit skip.

## Latency and throughput

Stable product IDs have the shape `e2e/<operation>/cold-stdin/<toml-version>/<fixture-id>`. Every Criterion iteration starts a fresh product process, writes the complete fixture through stdin, waits for stdout and stderr, and consumes only a constant-time output fingerprint in the measured closure. Fixture generation, semantic digests, correctness checks, configuration creation, and peak-RSS collection stay outside the timed loop.

Canonical runs use `scripts/run-bench.sh`, one exact lane per run ID, a 3-second warm-up, a 5-second Criterion measurement window, and 30 samples unless the published bundle records different settings. Timed iterations use the direct product spawn path without process-group setup so timeout containment does not become benchmarked product latency; the bounded correctness and preflight call must pass immediately before the timed loop. Throughput is calculated from exact input bytes.

Shared CI runners are smoke and regression signals, not headline ranking hardware. The scheduled and manually dispatched full benchmark validates the selected canonical lanes, runs every lane in a separate matrix job, and aggregates their complete result bundles without merging files from different runners. This lane isolation reduces workflow wall time and prevents one lane from contaminating another, but it does not turn cross-runner measurements into a controlled concurrent comparison. Publishable tables require dedicated hardware, repeated sessions, and uncertainty reporting. Differences inside observed session drift must not be described as a stable ordering.

## Peak memory

Peak RSS is collected in separate fresh processes after latency measurement so the platform resource meter does not affect Criterion samples. The report stores every sample plus the median and maximum. GNU `time -v` reports KiB on Linux; BSD `/usr/bin/time -l` reports bytes on macOS, and the harness normalizes both to bytes.

## Identity and environment

Every enabled executable is selected by an explicit absolute path, checked against an exact version, and hashed with SHA-256. Go products additionally verify embedded module identity; the Prettier lane records the plugin package and entry hashes. Per-product configuration is isolated and ambient project configuration is disabled.

Each run bundle records hardware, operating system, power information when available, Cargo and Rustc identities, required Go/Node runtimes, the effective benchmark settings, product catalog/status and detected versions, corpus and lockfile hashes, and the benchmark repository revision. Published package versions and `Cargo.lock` replace source-checkout revisions for TomlSmith dependencies. Publishable runs require a clean benchmark repository; a dirty-diff hash without the corresponding patch is not reconstructable evidence.

## Publication contract

A README or website performance table may be published only from a complete passing run directory containing Criterion `raw.csv`, `environment.json`, `verification.json`, `peak-rss.json`, the benchmark source revision, exact package/product versions, and the generated summary. Git stores the generated tables, charts, and compact machine-readable summary, not raw result directories or archives. Local ignored directories and expiring GitHub Actions artifacts retain the full capture; when long-lived public evidence is needed, publish an immutable checksummed archive as a GitHub Release asset outside the source tree and link it only after it exists.

Tables, SVG charts, and website JSON must be derived and cross-checked against the same raw run directories. English and Simplified Chinese values must remain identical. Historical data without this evidence remains explicitly labeled as a frozen, non-current baseline.

Current limitations are that the formatter digest still needs an independent non-TomlSmith judge, and stable public rankings require multi-session uncertainty and regression budgets.
