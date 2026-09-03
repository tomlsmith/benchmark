# TomlSmith Benchmark

**English** | [简体中文](README.zh-Hans.md)

[![CI](https://github.com/tomlsmith/benchmark/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/benchmark/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

TomlSmith Benchmark measures end-to-end cold-start latency, throughput, and peak memory for TOML checking and formatting CLIs. Each sample runs the product command in a new process instead of calling a parser library directly.

<!-- BENCHMARK_RESULTS_START -->
## Check

The check scenario uses deterministic synthetic documents covering the published TOML 1.0.0 and TOML 1.1.0 grammars. Only product/version pairs that accept the document under the selected version are shown.

<table>
<tr>
<td valign="top">

**TOML 1.0 · `v1_0_medium`**

| Product | Median (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: |
| TomlSmith native CLI | 10.21🥇 | 12.26🥇 | 6.98🥇 |
| BurntSushi/toml `tomlv` | 12.07🥈 | 10.37🥈 | 8.97🥈 |
| Taplo CLI | 23.34🥉 | 5.36🥉 | 18.59🥉 |
| Tombi | 206.80 | 0.61 | 27.25 |

</td>
<td valign="top">

**TOML 1.1 · `v1_1_medium`**

| Product | Median (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: |
| TomlSmith native CLI | 8.17🥇 | 15.33🥇 | 6.95🥇 |
| BurntSushi/toml `tomlv` | 10.07🥈 | 12.43🥈 | 8.89🥈 |
| Tombi | 193.31🥉 | 0.65🥉 | 27.28🥉 |

</td>
</tr>
</table>

![Check latency and peak RSS for TOML 1.0 and 1.1](assets/check-results-20260903.svg)

## Format

The format scenario uses the same documents in an intentionally edited layout. A result is included only when the output reparses under the selected version, preserves decoded content, and is unchanged by a second format pass.

<table>
<tr>
<td valign="top">

**TOML 1.0 · `v1_0_medium`**

| Product | Median (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: |
| TomlSmith native CLI | 8.35🥇 | 14.99🥇 | 6.95🥈 |
| pelletier/go-toml `tomll`* | 10.91🥈 | 11.47🥈 | 6.45🥇 |
| dprint + TOML plugin | 20.12🥉 | 6.22🥉 | 19.94 |
| Taplo CLI | 22.07 | 5.67 | 18.77🥉 |
| Tombi | 655.14 | 0.19 | 31.86 |
| Prettier + prettier-plugin-toml | 678.41 | 0.18 | 594.09 |

</td>
<td valign="top">

**TOML 1.1 · `v1_1_medium`**

| Product | Median (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: |
| TomlSmith native CLI | 8.36🥇 | 14.97🥇 | 7.02🥈 |
| pelletier/go-toml `tomll`* | 9.87🥈 | 12.68🥈 | 6.56🥇 |
| dprint + TOML plugin | 18.99🥉 | 6.59🥉 | 19.94🥉 |
| Tombi | 721.46 | 0.17 | 31.55 |

</td>
</tr>
</table>

![Format latency and peak RSS for TOML 1.0 and 1.1](assets/format-results-20260903.svg)

\* The correctness check compares decoded TOML content, not comments or layout. `tomll` reparses and writes the data, discarding comments and changing literal styles. The other formatters preserve comments, so these rows do not represent identical work.

These are the same four canonical results published on [tomlsmith.github.io](https://tomlsmith.github.io/). Git stores only the generated tables and charts. Raw run directories and archives are intentionally ignored; if they need to be distributed, publish them as checksummed GitHub Release assets rather than source files.
<!-- BENCHMARK_RESULTS_END -->

## Run locally

Install the exact published TomlSmith native CLI and the pinned competitor CLIs (prerequisites are in [CONTRIBUTING.md](CONTRIBUTING.md)):

```bash
./scripts/setup-products.sh
```

Run any exact lane, for example:

```bash
TOMLSMITH_BENCH_FILTER=e2e/check/cold-stdin/1.0/v1_0_medium \
  ./scripts/run-bench.sh local-check-v1_0

TOMLSMITH_BENCH_FILTER=e2e/check/cold-stdin/1.1/v1_1_medium \
  ./scripts/run-bench.sh local-check-v1_1

TOMLSMITH_BENCH_FILTER=e2e/format/cold-stdin/1.0/v1_0_medium \
  ./scripts/run-bench.sh local-format-v1_0

TOMLSMITH_BENCH_FILTER=e2e/format/cold-stdin/1.1/v1_1_medium \
  ./scripts/run-bench.sh local-format-v1_1
```

The manually dispatched `Benchmark` workflow accepts a `tomlsmith_ref` input to build the TomlSmith CLI from any git ref of the core repository before running the selected lanes. A locally built TomlSmith executable can be measured by exporting `TOMLSMITH_BIN` with `TOMLSMITH_BIN_EXPECTED_VERSION=any` (or an explicit version string) to relax the exact release pin, and `TOMLSMITH_BENCH_SKIP_PEAK_RSS=1` skips the peak-RSS pass on hosts without GNU `time`; published lanes leave both unset. Every timed sample starts a fresh process and reads the document from stdin; Peak RSS is the median of three separate fresh runs. Summarize any result directory with `scripts/summarize-results.py` and regenerate the charts with `scripts/generate-result-charts.py`.

Correctness, preflight, and resource-sampling process trees are bounded by `TOMLSMITH_BENCH_PROCESS_TIMEOUT_SECS` (120 seconds by default). The cross-product gate covers the four canonical medium-fixture publication lanes, while every other selected lane—including large and stress fixtures—must pass its own bounded preflight before timing. Timed iterations use the direct spawn path after that preflight so containment overhead is not counted as product latency. See the complete [measurement methodology](docs/methodology.md).

Run the pull-request gate with:

```bash
./scripts/check.sh
```

Before publishing measurements, run the cross-product correctness gate for all four canonical publication lanes with `./scripts/check-products.sh`; any additional lane is verified again against its exact fixture by the benchmark runner. Scheduled and on-demand benchmark workflows upload complete result directories as temporary GitHub Actions artifacts. Raw directories and archives stay out of Git; a maintainer can promote an archive and its checksum to a GitHub Release when long-lived public evidence is needed.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Participation is governed by the organization-wide [Code of Conduct](https://github.com/tomlsmith/tomlsmith/blob/main/CODE_OF_CONDUCT.md).

## License

MIT. See [LICENSE](LICENSE).
