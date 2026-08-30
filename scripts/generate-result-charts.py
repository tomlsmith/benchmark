#!/usr/bin/env python3
"""Regenerate the README result charts from run-bench.sh result directories.

Usage:
  scripts/generate-result-charts.py check  V1_0_DIR V1_1_DIR OUTPUT_SVG
  scripts/generate-result-charts.py format V1_0_DIR V1_1_DIR OUTPUT_SVG

Reads the same Criterion raw.csv and peak-rss.json data as
summarize-results.py and emits a two-panel vertical bar chart. Latency spans
two orders of magnitude across products, so bars use an explicitly labeled
logarithmic axis and every bar carries its exact median; the README table
next to the chart remains the full-precision data view.

Color: the TomlSmith bar wears the panel accent; competitor bars use a muted
slate that deliberately reads as context, not identity — identity comes from
the product name printed under every bar (validated with the dataviz
six-check palette script: lightness, CVD separation, normal-vision floor,
and >= 3:1 surface contrast all pass).
"""

import csv
import json
import math
import statistics
import sys
from pathlib import Path

DISPLAY_NAMES = {
    "burntsushi-toml": "BurntSushi",
    "tomlsmith": "TomlSmith",
    "taplo": "Taplo",
    "tombi": "Tombi",
    "go-toml-tomll": "tomll",
    "dprint": "dprint",
    "prettier": "Prettier",
}

PANELS = [
    {"panel_x": 28, "accent": "#2563eb", "title": "TOML 1.0"},
    {"panel_x": 618, "accent": "#e7652e", "title": "TOML 1.1"},
]

NEUTRAL_BAR = "#64748b"
INK = "#17324d"
INK_MUTED = "#60758a"
INK_FAINT = "#8796a5"
GRID = "#e5ebf1"
BASELINE_STROKE = "#d8e1ea"

PANEL_WIDTH = 554
PLOT_TOP = 210
PLOT_BASELINE = 470
PANEL_HEIGHT = 440
CANVAS_HEIGHT = 610
AXIS_MIN_MS = 1.0
AXIS_MAX_MS = 1000.0
AXIS_TICKS = [(1, "1"), (10, "10"), (100, "100"), (1000, "1k ms")]


def load_rows(result_dir: Path):
    rss_path = result_dir / "peak-rss.json"
    rss = {}
    if rss_path.exists():
        data = json.loads(rss_path.read_text())
        rss = {
            case["product_id"]: case["median_peak_rss_bytes"] / (1024 * 1024)
            for case in data.get("cases", [])
        }
    fixture_bytes = None
    fixture_id = None
    rows = []
    for raw_csv in sorted(result_dir.glob("criterion/*/*/new/raw.csv")):
        samples = []
        with raw_csv.open(newline="") as handle:
            for record in csv.DictReader(handle):
                fixture_id = record["group"].rsplit("/", 1)[-1]
                if record["throughput_type"] == "bytes":
                    fixture_bytes = int(record["throughput_num"])
                samples.append(
                    float(record["sample_measured_value"]) / int(record["iteration_count"])
                )
        product = raw_csv.parent.parent.name
        rows.append((statistics.median(samples) / 1e6, product))
    rows.sort()
    return fixture_id, fixture_bytes, rows, rss


def y_for(value_ms: float) -> float:
    span = math.log10(AXIS_MAX_MS / AXIS_MIN_MS)
    fraction = math.log10(max(value_ms, AXIS_MIN_MS) / AXIS_MIN_MS) / span
    return PLOT_BASELINE - fraction * (PLOT_BASELINE - PLOT_TOP)


def bar_path(x: float, width: float, top: float) -> str:
    """A bar with a 4px rounded data end and a square baseline anchor."""
    radius = min(4.0, width / 2)
    return (
        f"M{x:.1f},{PLOT_BASELINE} "
        f"L{x:.1f},{top + radius:.1f} "
        f"Q{x:.1f},{top:.1f} {x + radius:.1f},{top:.1f} "
        f"L{x + width - radius:.1f},{top:.1f} "
        f"Q{x + width:.1f},{top:.1f} {x + width:.1f},{top + radius:.1f} "
        f"L{x + width:.1f},{PLOT_BASELINE} Z"
    )


def median_label(value_ms: float) -> str:
    return f"{value_ms:.0f}" if value_ms >= 100 else f"{value_ms:.1f}"


def panel_svg(panel: dict, result_dir: Path) -> str:
    panel_x = panel["panel_x"]
    label_x = panel_x + 20
    plot_left = panel_x + 62
    plot_right = panel_x + PANEL_WIDTH - 22
    fixture_id, fixture_bytes, rows, rss = load_rows(result_dir)

    grid = "\n".join(
        f'    <line x1="{plot_left}" y1="{y_for(value):.1f}" x2="{plot_right}" '
        f'y2="{y_for(value):.1f}" stroke="{GRID if value > AXIS_MIN_MS else BASELINE_STROKE}"/>'
        for value, _ in AXIS_TICKS
    )
    ticks = "\n".join(
        f'    <text x="{plot_left - 8}" y="{y_for(value) + 4:.1f}">{label}</text>'
        for value, label in AXIS_TICKS
    )

    slot = (plot_right - plot_left) / len(rows)
    width = min(56.0, slot * 0.6)
    bars = []
    values = []
    names = []
    rss_labels = []
    for index, (median, product) in enumerate(rows):
        x = plot_left + slot * index + (slot - width) / 2
        center = x + width / 2
        top = y_for(median)
        color = panel["accent"] if product == "tomlsmith" else NEUTRAL_BAR
        bars.append(f'    <path d="{bar_path(x, width, top)}" fill="{color}"/>')
        values.append(
            f'    <text x="{center:.1f}" y="{top - 8:.1f}">{median_label(median)}</text>'
        )
        emphasis = ' font-weight="700"' if product == "tomlsmith" else ""
        names.append(
            f'    <text x="{center:.1f}" y="{PLOT_BASELINE + 22}"{emphasis}>'
            f"{DISPLAY_NAMES.get(product, product)}</text>"
        )
        rss_value = rss.get(product)
        rss_text = f"{rss_value:.1f} MiB" if rss_value is not None else ""
        rss_labels.append(
            f'    <text x="{center:.1f}" y="{PLOT_BASELINE + 40}">{rss_text}</text>'
        )

    newline = "\n"
    return f"""  <rect x="{panel_x}" y="106" width="{PANEL_WIDTH}" height="{PANEL_HEIGHT}" rx="16" fill="#ffffff" stroke="{BASELINE_STROKE}"/>
  <text x="{label_x}" y="145" fill="{panel["accent"]}" font-family="ui-sans-serif, system-ui, sans-serif" font-size="20" font-weight="700">{panel["title"]}</text>
  <text x="{label_x}" y="171" fill="{INK_MUTED}" font-family="ui-sans-serif, system-ui, sans-serif" font-size="13">{fixture_id}</text>
  <g stroke-width="1">
{grid}
  </g>
  <g fill="{INK_FAINT}" font-family="ui-monospace, SFMono-Regular, monospace" font-size="10" text-anchor="end">
{ticks}
  </g>
  <g>
{newline.join(bars)}
  </g>
  <g fill="{INK}" font-family="ui-monospace, SFMono-Regular, monospace" font-size="12" text-anchor="middle">
{newline.join(values)}
  </g>
  <g fill="{INK}" font-family="ui-sans-serif, system-ui, sans-serif" font-size="13" text-anchor="middle">
{newline.join(names)}
  </g>
  <g fill="{INK_FAINT}" font-family="ui-monospace, SFMono-Regular, monospace" font-size="11" text-anchor="middle">
{newline.join(rss_labels)}
  </g>"""


def main(argv):
    if len(argv) != 5 or argv[1] not in {"check", "format"}:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    operation = argv[1]
    panels = [
        panel_svg(PANELS[0], Path(argv[2])),
        panel_svg(PANELS[1], Path(argv[3])),
    ]
    title = operation.capitalize()
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="{CANVAS_HEIGHT}" viewBox="0 0 1200 {CANVAS_HEIGHT}" role="img" aria-labelledby="title desc">
  <title id="title">TomlSmith benchmark {operation} results</title>
  <desc id="desc">Bar chart of cold CLI {operation} median latency (logarithmic axis) with peak RSS for separate synthetic TOML 1.0 and TOML 1.1 documents.</desc>
  <rect width="1200" height="{CANVAS_HEIGHT}" fill="#f5f7fa"/>
  <text x="30" y="46" fill="{INK}" font-family="ui-sans-serif, system-ui, sans-serif" font-size="28" font-weight="700">{title}</text>
  <text x="30" y="76" fill="{INK_MUTED}" font-family="ui-sans-serif, system-ui, sans-serif" font-size="15">Cold CLI · median latency on a logarithmic axis · lower is better</text>
  <text x="1170" y="76" fill="{INK_MUTED}" font-family="ui-monospace, SFMono-Regular, monospace" font-size="13" text-anchor="end">bar top: median ms · below name: Peak RSS</text>

{panels[0]}

{panels[1]}
</svg>
"""
    Path(argv[4]).write_text(svg)
    print(f"wrote {argv[4]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
