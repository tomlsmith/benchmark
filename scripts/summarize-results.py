#!/usr/bin/env python3
"""Summarize run-bench.sh result directories as a Markdown table.

Usage: scripts/summarize-results.py RESULT_DIR [RESULT_DIR ...]

Reads the Criterion raw.csv samples and peak-rss.json captured by
run-bench.sh and prints one Markdown table per benchmark lane with the
median latency, derived throughput, and median peak RSS for every product.
The three best values in each metric column are marked with medals.
The script only depends on the Python standard library so it can run in CI
and locally without extra setup.
"""

import csv
import json
import statistics
import sys
from pathlib import Path

MEDALS = ("🥇", "🥈", "🥉")


def rank_medals(values, *, reverse=False):
    """Return the medal for each of the three best distinct values."""
    ranked_values = sorted(set(values), reverse=reverse)[: len(MEDALS)]
    return dict(zip(ranked_values, MEDALS))


def format_ranked(value, decimal_places, medals):
    if value is None:
        return ""
    return f"{value:.{decimal_places}f}{medals.get(value, '')}"


def lane_samples(raw_csv: Path):
    """Return (lane, input_bytes, per-iteration nanosecond samples)."""
    lane = None
    input_bytes = None
    samples = []
    with raw_csv.open(newline="") as handle:
        for row in csv.DictReader(handle):
            lane = row["group"]
            if row["throughput_type"] == "bytes":
                input_bytes = int(row["throughput_num"])
            samples.append(
                float(row["sample_measured_value"]) / int(row["iteration_count"])
            )
    return lane, input_bytes, samples


def peak_rss_mib(result_dir: Path):
    """Return {product_id: median peak RSS in MiB} when captured."""
    path = result_dir / "peak-rss.json"
    if not path.exists():
        return {}
    data = json.loads(path.read_text())
    return {
        case["product_id"]: case["median_peak_rss_bytes"] / (1024 * 1024)
        for case in data.get("cases", [])
    }


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    for argument in argv[1:]:
        result_dir = Path(argument)
        raw_files = sorted(result_dir.glob("criterion/*/*/new/raw.csv"))
        if not raw_files:
            print(f"no Criterion raw.csv found under {result_dir}", file=sys.stderr)
            return 1

        rss = peak_rss_mib(result_dir)
        rows = []
        lane_name = None
        for raw_csv in raw_files:
            lane, input_bytes, samples = lane_samples(raw_csv)
            lane_name = lane_name or lane
            product = raw_csv.parent.parent.name
            median_ms = statistics.median(samples) / 1e6
            throughput = None
            if input_bytes:
                throughput = input_bytes / (1024 * 1024) / (median_ms / 1000)
            peak = rss.get(product)
            rows.append((median_ms, product, throughput, peak))

        rows.sort()
        median_medals = rank_medals(row[0] for row in rows)
        throughput_medals = rank_medals(
            (row[2] for row in rows if row[2] is not None), reverse=True
        )
        peak_medals = rank_medals(row[3] for row in rows if row[3] is not None)
        print(f"### {result_dir.name} — `{lane_name}`\n")
        print("| Product | Median (ms) | Throughput (MiB/s) | Peak RSS (MiB) |")
        print("| --- | ---: | ---: | ---: |")
        for median_ms, product, throughput, peak in rows:
            median_text = format_ranked(median_ms, 3, median_medals)
            throughput_text = format_ranked(throughput, 2, throughput_medals)
            peak_text = format_ranked(peak, 2, peak_medals)
            print(
                f"| {product} | {median_text} | {throughput_text} | {peak_text} |"
            )
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
