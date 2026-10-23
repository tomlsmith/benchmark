# TomlSmith Benchmark

[English](README.md) | **简体中文**

[![CI](https://github.com/tomlsmith/benchmark/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/benchmark/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

TomlSmith Benchmark 测量 TOML 检查与格式化 CLI 的端到端冷启动延迟、吞吐量和峰值内存。每个样本都会新起一个进程运行产品命令，不直接调用解析器库。

<!-- BENCHMARK_RESULTS_START -->
## Check

检查场景使用覆盖 TOML 1.0.0 与 TOML 1.1.0 已发布语法的确定性合成文档，只展示能在所选版本下接受文档的产品与版本组合。

**TOML 1.0 · `v1_0_medium`**

| 产品 | 中位数 (ms) | 三次会话范围 (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| TomlSmith native CLI | 7.65 | 7.65–8.51 | 16.35 | 7.53 |
| BurntSushi/toml `tomlv` | 9.85 | 9.85–10.35 | 12.71 | 9.30 |
| Taplo CLI | 18.55 | 18.54–18.96 | 6.74 | 19.02 |
| Tombi | 173.20 | 173.20–183.09 | 0.72 | 26.88 |

**TOML 1.1 · `v1_1_medium`**

| 产品 | 中位数 (ms) | 三次会话范围 (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| TomlSmith native CLI | 8.69 | 8.51–8.69 | 14.42 | 7.56 |
| BurntSushi/toml `tomlv` | 10.22 | 9.96–10.40 | 12.25 | 9.27 |
| Tombi | 171.63 | 171.63–184.62 | 0.73 | 26.86 |

![Check: median latency and peak RSS](assets/check-results-20261023.svg)

## Format

格式化场景使用相同文档，并刻意调整初始布局。结果要求输出能按所选版本重新解析、保持解码内容不变，且第二次格式化不再变化。

**TOML 1.0 · `v1_0_medium`**

| 产品 | 中位数 (ms) | 三次会话范围 (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| pelletier/go-toml `tomll`* | 7.62 | 7.27–7.62 | 16.42 | 6.77 |
| TomlSmith native CLI | 8.16 | 8.15–9.68 | 15.33 | 7.56 |
| dprint + TOML plugin | 14.83 | 14.83–15.39 | 8.44 | 19.77 |
| Taplo CLI | 21.82 | 21.82–26.17 | 5.73 | 19.58 |
| Tombi | 619.48 | 613.23–619.48 | 0.20 | 31.31 |
| Prettier + prettier-plugin-toml | 687.96 | 682.12–703.69 | 0.18 | 564.73 |

**TOML 1.1 · `v1_1_medium`**

| 产品 | 中位数 (ms) | 三次会话范围 (ms) | MiB/s | Peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| pelletier/go-toml `tomll`* | 7.74 | 6.58–7.74 | 16.17 | 6.84 |
| TomlSmith native CLI | 8.04 | 7.81–8.78 | 15.57 | 7.63 |
| dprint + TOML plugin | 15.64 | 14.58–15.64 | 8.00 | 19.75 |
| Tombi | 626.94 | 626.94–689.98 | 0.20 | 31.63 |

![Format: median latency and peak RSS](assets/format-results-20261023.svg)

\* `tomll` 会丢弃注释并改写字面量，完成的工作少于保留注释的格式化器。

本次数据于 2026-10-23 在 Apple M4 Pro、macOS 27.2 上顺序测得，使用正式发布的 TomlSmith 0.4.1 和 Rust 1.92.0。每条赛道执行三次独立会话，每次预热 3 秒、测量窗口 5 秒、30 个样本，并另取 3 次新进程 Peak RSS。主表与图表采用测量前指定的第二次会话；范围列为三次会话中位数的最小值至最大值，并非置信区间。范围重叠或差异处于会话波动内时，不应解读为稳定排名。

双语表格、图表和[官网](https://tomlsmith.github.io/)共用这些数据。[机器可读摘要](assets/benchmark-data-20261023.json)记录全部会话、产品版本和原始文件校验值；完整结果目录保存在本地 `results/`，不加入 Git。
<!-- BENCHMARK_RESULTS_END -->

## 本地运行

安装正式发布的精确版本 TomlSmith 原生 CLI 与固定版本的参测 CLI（前置依赖见 [CONTRIBUTING.md](CONTRIBUTING.md)）：

```bash
./scripts/setup-products.sh
```

执行任意一条赛道，例如：

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

每个计时样本都会启动一个新进程，并通过 stdin 读取文档；Peak RSS 为三个独立新进程结果的中位数。任意结果目录都可以用 `scripts/summarize-results.py` 生成汇总表，并用 `scripts/generate-result-charts.py` 重新生成图表。

正确性、preflight 与资源采样进程树受 `TOMLSMITH_BENCH_PROCESS_TIMEOUT_SECS` 限制（默认 120 秒）。跨产品门禁覆盖四条标准的 medium fixture 发布赛道；任何其他选定赛道（包括 large 与 stress fixture）都必须在计时前独立通过有界 preflight。计时迭代随后使用直接 spawn 路径，避免把 containment 开销算进产品延迟。完整契约见[测量方法](docs/methodology.md)。

在已安装 Node.js 24 与 pnpm 11.22.0 的环境中执行 Pull Request 门禁：

```bash
pnpm install --frozen-lockfile
./scripts/check.sh
```

发布测量结果前，还要用 `./scripts/check-products.sh` 对四条标准发布赛道运行跨产品正确性门禁；任何额外赛道都会由 benchmark runner 针对其精确 fixture 再次验证。定时和手动 benchmark 工作流会把完整结果目录上传为有保留期的 GitHub Actions artifact。原始目录和归档包不进入 Git；需要长期公开证据时，维护者可将归档包及其校验和提升为 GitHub Release 资产。

## 参与贡献

提交 Pull Request 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。参与项目需要遵守组织统一的 [行为准则](https://github.com/tomlsmith/tomlsmith/blob/main/CODE_OF_CONDUCT.md)。

## 许可

项目采用 MIT 许可，详见 [LICENSE](LICENSE)。
