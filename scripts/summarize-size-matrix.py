#!/usr/bin/env python3
"""Build the by-document-size Markdown matrix from result directories.

Usage: scripts/summarize-size-matrix.py RESULT_DIR [RESULT_DIR ...]

Each result directory holds one lane (one operation x TOML version x
fixture). The script groups them into one table per operation and TOML
version: product rows, one median-latency column per document size, with
the three fastest values per column marked. Missing cells (a product that
does not support a lane or failed its correctness gate) render as an em dash.
"""

import csv
import statistics
import sys
from pathlib import Path

DISPLAY_NAMES = {
    "burntsushi-toml": "BurntSushi/toml `tomlv`",
    "tomlsmith": "TomlSmith",
    "taplo": "Taplo CLI",
    "tombi": "Tombi",
    "go-toml-tomll": "pelletier/go-toml `tomll`",
    "dprint": "dprint + TOML plugin",
    "prettier": "Prettier + prettier-plugin-toml",
}

SIZE_LABELS = {
    "small": "4 KiB",
    "medium": "128 KiB",
    "large": "1 MiB",
    "stress": "10 MiB",
}
SIZE_ORDER = ["small", "medium", "large", "stress"]
MEDALS = ("🥇", "🥈", "🥉")


def lane_medians(result_dir: Path):
    """Return (operation, version, size, {product: median_ms})."""
    operation = version = size = None
    medians = {}
    for raw_csv in sorted(result_dir.glob("criterion/*/*/new/raw.csv")):
        samples = []
        with raw_csv.open(newline="") as handle:
            for record in csv.DictReader(handle):
                group = record["group"]
                samples.append(
                    float(record["sample_measured_value"]) / int(record["iteration_count"])
                )
        _, operation, _, version, fixture = group.split("/")
        size = fixture.rsplit("_", 1)[-1]
        medians[raw_csv.parent.parent.name] = statistics.median(samples) / 1e6
    return operation, version, size, medians


def rank_medals(values, *, reverse=False):
    """Return the medal for each of the three best distinct values."""
    ranked_values = sorted(set(values), reverse=reverse)[: len(MEDALS)]
    return dict(zip(ranked_values, MEDALS))


def format_cell(value, medal):
    if value is None:
        return "—"
    text = f"{value:.2f}" if value < 100 else f"{value:,.0f}"
    return f"{text}{medal}"


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    # tables[(operation, version)][size][product] = median_ms
    tables = {}
    for argument in argv[1:]:
        operation, version, size, medians = lane_medians(Path(argument))
        if not medians:
            print(f"no Criterion data under {argument}", file=sys.stderr)
            return 1
        tables.setdefault((operation, version), {})[size] = medians

    for (operation, version), by_size in sorted(tables.items()):
        sizes = [size for size in SIZE_ORDER if size in by_size]
        products = {product for medians in by_size.values() for product in medians}
        # Sort rows by the largest shared size so the headline ordering is
        # stable; products missing there sort by their smallest available lane.
        anchor = sizes[-1]

        def sort_key(product):
            for size in [anchor, *reversed(sizes)]:
                value = by_size.get(size, {}).get(product)
                if value is not None:
                    return value
            return float("inf")

        medals = {
            size: rank_medals(by_size[size].values())
            for size in sizes
        }

        print(f"### {operation} · TOML {version} (median ms)\n")
        header = " | ".join(SIZE_LABELS[size] for size in sizes)
        print(f"| Product | {header} |")
        print("| --- |" + " ---: |" * len(sizes))
        for product in sorted(products, key=sort_key):
            cells = " | ".join(
                format_cell(
                    by_size[size].get(product),
                    medals[size].get(by_size[size].get(product), ""),
                )
                for size in sizes
            )
            print(f"| {DISPLAY_NAMES.get(product, product)} | {cells} |")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
