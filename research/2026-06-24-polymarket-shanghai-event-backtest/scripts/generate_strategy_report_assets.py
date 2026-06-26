#!/usr/bin/env python3
"""Generate SVG assets for the Polymarket Shanghai smoke report.

The repo environment does not require matplotlib/plotly for this research note;
these charts are intentionally dependency-light SVGs generated from the CSV
artifacts emitted by the current harness.
"""

from __future__ import annotations

import csv
import html
from datetime import datetime
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parents[3]
RESEARCH = ROOT / "research" / "2026-06-24-polymarket-shanghai-event-backtest"
DATA = RESEARCH / "data"
ASSETS = RESEARCH / "report_assets"
SUMMARY = DATA / "strategy_suite_summary.csv"

EVENT_NAMES = {
    "highest-temperature-in-shanghai-on-june-9-2026": "Jun 9 / 25C YES",
    "highest-temperature-in-shanghai-on-june-10-2026": "Jun 10 / 28C YES",
}
STRATEGY_ORDER = ["buy_hold_first_ask", "momentum_taker", "contrarian_taker", "maker_bbo"]
COLORS = {
    "buy_hold_first_ask": "#2ca02c",
    "momentum_taker": "#1f77b4",
    "contrarian_taker": "#ff7f0e",
    "maker_bbo": "#d62728",
    "mid": "#1f77b4",
    "bid": "#2ca02c",
    "ask": "#d62728",
}


def read_csv(path: Path) -> list[dict[str, str]]:
    with path.open("r", encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def parse_dt(value: str) -> datetime:
    value = value.strip().replace("Z", "+00:00")
    return datetime.fromisoformat(value)


def f(value: str | float | int) -> float:
    try:
        return float(value)
    except Exception:
        return 0.0


def esc(s: object) -> str:
    return html.escape(str(s), quote=True)


def svg_frame(width: int, height: int, body: str, title: str = "") -> str:
    title_el = f"<title>{esc(title)}</title>" if title else ""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" role="img">\n'
        f"{title_el}\n"
        '<rect width="100%" height="100%" fill="white"/>\n'
        '<style>text{font-family:Arial,Helvetica,sans-serif;font-size:12px;fill:#222}.small{font-size:10px;fill:#555}.title{font-size:16px;font-weight:700}.axis{stroke:#999;stroke-width:1}.grid{stroke:#e6e6e6;stroke-width:1}.legend{font-size:11px}</style>\n'
        f"{body}\n</svg>\n"
    )


def scale(vals: Iterable[float], lo_px: float, hi_px: float, pad_frac: float = 0.08):
    vals = list(vals)
    lo = min(vals) if vals else 0.0
    hi = max(vals) if vals else 1.0
    if lo == hi:
        lo -= 1.0
        hi += 1.0
    pad = (hi - lo) * pad_frac
    lo -= pad
    hi += pad

    def mapv(v: float) -> float:
        return lo_px + (v - lo) / (hi - lo) * (hi_px - lo_px)

    return mapv, lo, hi


def polyline(points: list[tuple[float, float]], color: str, width: float = 2.0) -> str:
    if not points:
        return ""
    pts = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
    return f'<polyline fill="none" stroke="{color}" stroke-width="{width}" points="{pts}"/>'


def bar_chart(summary: list[dict[str, str]]) -> str:
    width, height = 980, 430
    m = dict(left=75, right=25, top=55, bottom=90)
    plot_w = width - m["left"] - m["right"]
    plot_h = height - m["top"] - m["bottom"]
    vals = [f(r["settlement_pnl"]) for r in summary]
    ymap, ymin, ymax = scale(vals + [0.0], m["top"] + plot_h, m["top"], 0.15)
    zero = ymap(0.0)
    group_w = plot_w / 2
    bar_gap = 8
    bar_w = (group_w - 75) / 4
    body = [f'<text class="title" x="{m["left"]}" y="28">Settlement PnL by strategy</text>']
    # grid
    for t in [ymin, (ymin + 0) / 2, 0, ymax / 2, ymax]:
        if ymin <= t <= ymax:
            y = ymap(t)
            body.append(f'<line class="grid" x1="{m["left"]}" y1="{y:.1f}" x2="{width-m["right"]}" y2="{y:.1f}"/>')
            body.append(f'<text class="small" x="10" y="{y+4:.1f}">{t:.0f}</text>')
    body.append(f'<line class="axis" x1="{m["left"]}" y1="{zero:.1f}" x2="{width-m["right"]}" y2="{zero:.1f}"/>')
    by_event = {}
    for r in summary:
        by_event.setdefault(r["event_slug"], {})[r["strategy"]] = r
    for gi, event in enumerate(EVENT_NAMES):
        x0 = m["left"] + gi * group_w + 35
        body.append(f'<text x="{x0+group_w/2-75:.1f}" y="{height-35}">{esc(EVENT_NAMES[event])}</text>')
        for si, strat in enumerate(STRATEGY_ORDER):
            r = by_event[event][strat]
            val = f(r["settlement_pnl"])
            x = x0 + si * (bar_w + bar_gap)
            y = ymap(max(val, 0))
            y2 = ymap(min(val, 0))
            h = abs(y2 - y)
            color = COLORS[strat]
            body.append(f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w:.1f}" height="{h:.1f}" fill="{color}" opacity="0.85"><title>{esc(strat)} {val:.2f}</title></rect>')
            body.append(f'<text class="small" transform="translate({x+bar_w/2:.1f},{height-48}) rotate(-35)" text-anchor="end">{esc(strat.replace("_", " "))}</text>')
            body.append(f'<text class="small" x="{x+bar_w/2:.1f}" y="{(y-5 if val>=0 else y2+13):.1f}" text-anchor="middle">{val:.1f}</text>')
    return svg_frame(width, height, "\n".join(body), "Settlement PnL by strategy")


def quality_chart(summary: list[dict[str, str]]) -> str:
    width, height = 900, 360
    m = dict(left=80, right=25, top=55, bottom=70)
    metrics = [
        ("price_change_batch_mismatch_rate", "Batch BBO"),
        ("snapshot_bbo_mismatch_rate", "Snapshot BBO"),
        ("trade_off_book_rate", "Trade off-book"),
    ]
    # one row per event
    rows = []
    seen = set()
    for r in summary:
        if r["event_slug"] not in seen:
            seen.add(r["event_slug"])
            rows.append(r)
    maxv = max(f(r[k]) for r in rows for k, _ in metrics) * 100
    xmap, _, xmax = scale([0, maxv], m["left"], width - m["right"], 0.05)
    body = [f'<text class="title" x="{m["left"]}" y="28">Replay diagnostics carried into results</text>']
    for pct in [0, 5, 10, 15, 20, 25]:
        if pct <= xmax:
            x = xmap(pct)
            body.append(f'<line class="grid" x1="{x:.1f}" y1="{m["top"]}" x2="{x:.1f}" y2="{height-m["bottom"]}"/>')
            body.append(f'<text class="small" x="{x:.1f}" y="{height-45}" text-anchor="middle">{pct}%</text>')
    colors = ["#1f77b4", "#ff7f0e", "#2ca02c"]
    for ri, r in enumerate(rows):
        base_y = m["top"] + 35 + ri * 115
        body.append(f'<text x="8" y="{base_y+18}">{esc(EVENT_NAMES[r["event_slug"]])}</text>')
        for mi, (key, label) in enumerate(metrics):
            y = base_y + mi * 24
            val = f(r[key]) * 100
            body.append(f'<rect x="{m["left"]}" y="{y:.1f}" width="{xmap(val)-m["left"]:.1f}" height="16" fill="{colors[mi]}" opacity="0.85"><title>{esc(label)} {val:.2f}%</title></rect>')
            body.append(f'<text class="small" x="{m["left"]+5}" y="{y+12:.1f}" fill="white">{esc(label)}</text>')
            body.append(f'<text class="small" x="{xmap(val)+5:.1f}" y="{y+12:.1f}">{val:.2f}%</text>')
    return svg_frame(width, height, "\n".join(body), "Replay diagnostics")


def find_case_dir(summary_row: dict[str, str], strategy: str | None = None) -> Path:
    path = Path(summary_row["summary_json"])
    if not path.is_absolute():
        path = ROOT / path
    if strategy and summary_row["strategy"] != strategy:
        raise ValueError("summary_row strategy mismatch")
    return path.parent


def price_paths(summary: list[dict[str, str]]) -> str:
    width, height = 980, 520
    m = dict(left=55, right=25, top=55, bottom=55)
    panel_h = (height - m["top"] - m["bottom"] - 35) / 2
    body = [f'<text class="title" x="{m["left"]}" y="28">BBO / mid price path (5-minute samples)</text>']
    by_event = {}
    for r in summary:
        if r["strategy"] == "buy_hold_first_ask":
            by_event[r["event_slug"]] = r
    for pi, event in enumerate(EVENT_NAMES):
        rows = read_csv(find_case_dir(by_event[event]) / "bbo_5min.csv")
        times = [parse_dt(r["timestamp"]).timestamp() for r in rows]
        bids = [f(r["best_bid"]) for r in rows]
        asks = [f(r["best_ask"]) for r in rows]
        mids = [f(r["mid"]) for r in rows]
        top = m["top"] + pi * (panel_h + 35)
        bottom = top + panel_h
        xmap, _, _ = scale(times, m["left"], width - m["right"], 0.0)
        ymap, _, _ = scale(bids + asks + mids + [0, 1], bottom, top, 0.03)
        for yv in [0, .25, .5, .75, 1.0]:
            y = ymap(yv)
            body.append(f'<line class="grid" x1="{m["left"]}" y1="{y:.1f}" x2="{width-m["right"]}" y2="{y:.1f}"/>')
            body.append(f'<text class="small" x="18" y="{y+4:.1f}">{yv:.2f}</text>')
        body.append(f'<text x="{m["left"]}" y="{top-8:.1f}">{esc(EVENT_NAMES[event])}</text>')
        body.append(polyline([(xmap(t), ymap(v)) for t, v in zip(times, bids)], COLORS["bid"], 1.2))
        body.append(polyline([(xmap(t), ymap(v)) for t, v in zip(times, asks)], COLORS["ask"], 1.2))
        body.append(polyline([(xmap(t), ymap(v)) for t, v in zip(times, mids)], COLORS["mid"], 2.0))
        body.append(f'<text class="small" x="{width-240}" y="{top+15:.1f}" fill="{COLORS["mid"]}">mid</text>')
        body.append(f'<text class="small" x="{width-200}" y="{top+15:.1f}" fill="{COLORS["bid"]}">bid</text>')
        body.append(f'<text class="small" x="{width-160}" y="{top+15:.1f}" fill="{COLORS["ask"]}">ask</text>')
    return svg_frame(width, height, "\n".join(body), "BBO mid price paths")


def equity_curves(summary: list[dict[str, str]]) -> str:
    width, height = 980, 560
    m = dict(left=65, right=25, top=55, bottom=55)
    panel_h = (height - m["top"] - m["bottom"] - 35) / 2
    body = [f'<text class="title" x="{m["left"]}" y="28">Mark-to-market equity curves</text>']
    by_event_strategy = {}
    for r in summary:
        by_event_strategy[(r["event_slug"], r["strategy"])] = r
    for pi, event in enumerate(EVENT_NAMES):
        series = {}
        all_times, all_vals = [], []
        for strat in STRATEGY_ORDER:
            rows = read_csv(find_case_dir(by_event_strategy[(event, strat)]) / "bbo_5min.csv")
            pts = [(parse_dt(r["timestamp"]).timestamp(), f(r["mtm_equity"])) for r in rows]
            series[strat] = pts
            all_times.extend(t for t, _ in pts)
            all_vals.extend(v for _, v in pts)
        top = m["top"] + pi * (panel_h + 35)
        bottom = top + panel_h
        xmap, _, _ = scale(all_times, m["left"], width - m["right"], 0.0)
        ymap, ymin, ymax = scale(all_vals + [0], bottom, top, 0.12)
        for yv in [ymin, 0, ymax]:
            y = ymap(yv)
            body.append(f'<line class="grid" x1="{m["left"]}" y1="{y:.1f}" x2="{width-m["right"]}" y2="{y:.1f}"/>')
            body.append(f'<text class="small" x="8" y="{y+4:.1f}">{yv:.0f}</text>')
        body.append(f'<text x="{m["left"]}" y="{top-8:.1f}">{esc(EVENT_NAMES[event])}</text>')
        for strat in STRATEGY_ORDER:
            body.append(polyline([(xmap(t), ymap(v)) for t, v in series[strat]], COLORS[strat], 1.8))
        lx = width - 245
        for li, strat in enumerate(STRATEGY_ORDER):
            y = top + 15 + li * 16
            body.append(f'<line x1="{lx}" y1="{y-4}" x2="{lx+25}" y2="{y-4}" stroke="{COLORS[strat]}" stroke-width="2"/>')
            body.append(f'<text class="small" x="{lx+32}" y="{y}">{esc(strat.replace("_", " "))}</text>')
    return svg_frame(width, height, "\n".join(body), "MTM equity curves")


def main() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    summary = read_csv(SUMMARY)
    outputs = {
        "strategy_pnl.svg": bar_chart(summary),
        "replay_quality.svg": quality_chart(summary),
        "price_paths.svg": price_paths(summary),
        "equity_curves.svg": equity_curves(summary),
    }
    for name, content in outputs.items():
        (ASSETS / name).write_text(content, encoding="utf-8")
    print(f"wrote {len(outputs)} assets to {ASSETS.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
