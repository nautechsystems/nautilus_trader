"""
Render the Betfair book imbalance tutorial panels from a backtest log.

After building NautilusTrader from source, run these commands from the repository root:

    CARGO_BUILD_JOBS=8 cargo run -p nautilus-betfair --features examples --profile nextest \
        --example betfair-backtest -- test_data/betfair/1-166811431.bz2 > /tmp/betfair.log 2>&1

    BETFAIR_LOG=/tmp/betfair.log \
        uv run --project python --no-sync \
            python docs/tutorials/assets/backtest_book_imbalance_betfair/render_panels.py

The strategy logs current best sizes as ``[runner] update #N: bid=B ask=A ratio=R``.
The renderer plots current signed imbalance, smaller/larger ratios, and best sizes.

"""

from __future__ import annotations

import os
import re
from pathlib import Path

import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme


OUT = Path(__file__).resolve().parent
LOG_PATH = Path(os.environ.get("BETFAIR_LOG", "/tmp/betfair.log"))  # noqa: S108

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]
PALETTE = [PRIMARY, NEGATIVE, POSITIVE, NEUTRAL]

ANSI = re.compile(r"\x1b\[[0-9;]*m")
SNAPSHOT = re.compile(
    r"\[(?P<inst>[^\]]+)\] update #(?P<n>\d+): "
    r"bid=(?P<bid>[0-9.]+) ask=(?P<ask>[0-9.]+) ratio=(?P<ratio>[0-9.]+)",
)


def parse_log(path: Path) -> pd.DataFrame:
    rows = []

    for line in path.read_text(encoding="utf-8").splitlines():
        match = SNAPSHOT.search(ANSI.sub("", line))
        if match:
            bid = float(match.group("bid"))
            ask = float(match.group("ask"))
            rows.append(
                {
                    "instrument": match.group("inst"),
                    "n": int(match.group("n")),
                    "bid": bid,
                    "ask": ask,
                    "ratio": float(match.group("ratio")),
                    "imbalance": (bid - ask) / (bid + ask),
                },
            )
    if not rows:
        raise ValueError(f"No current-book snapshots found in {path}")
    return pd.DataFrame(rows)


def apply_layout(fig: go.Figure, title: str, height: int = 480) -> None:
    fig.update_layout(
        template=TEMPLATE,
        title={"text": title, "x": 0.02, "xanchor": "left"},
        paper_bgcolor=COLORS["background"],
        plot_bgcolor=COLORS["background"],
        font={"family": "Inter, system-ui, sans-serif", "size": 13},
        margin={"l": 60, "r": 30, "t": 70, "b": 50},
        height=height,
        width=1200,
        legend={"orientation": "h", "yanchor": "bottom", "y": 1.08, "xanchor": "right", "x": 1.0},
    )
    fig.update_xaxes(gridcolor=GRID, zeroline=False)
    fig.update_yaxes(gridcolor=GRID, zeroline=False)


def panel_a_imbalance_lines(batches: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if batches.empty:
        apply_layout(fig, "Current best-size imbalance per runner (no data)", height=480)
        return fig
    runners = sorted(batches["instrument"].unique())
    for color, inst in zip(PALETTE, runners, strict=False):
        sel = batches[batches["instrument"] == inst].sort_values("n")
        fig.add_trace(
            go.Scatter(
                x=sel["n"],
                y=sel["imbalance"],
                mode="lines+markers",
                line_shape="hv",
                name=inst,
                line={"color": color, "width": 1.4},
                marker={"size": 4, "color": color},
            ),
        )
    fig.add_hline(y=0.0, line={"color": GRID, "width": 1})
    apply_layout(
        fig,
        "Current best-size imbalance per runner",
        height=520,
    )
    fig.update_xaxes(title_text="book update count")
    fig.update_yaxes(title_text="imbalance = (bid - ask) / (bid + ask)", range=[-1.05, 1.05])
    return fig


def panel_b_ratio_distribution(batches: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if batches.empty:
        apply_layout(fig, "Current smaller/larger ratio distribution (no data)", height=420)
        return fig
    df = batches
    runners = sorted(df["instrument"].unique())
    for color, inst in zip(PALETTE, runners, strict=False):
        sel = df[df["instrument"] == inst]
        fig.add_trace(
            go.Histogram(
                x=sel["ratio"],
                nbinsx=30,
                name=inst,
                marker={"color": color, "line": {"color": COLORS["background"], "width": 0.4}},
                opacity=0.7,
            ),
        )
    fig.update_layout(barmode="overlay")
    fig.add_vline(
        x=0.20,
        line={"color": NEUTRAL, "width": 1, "dash": "dash"},
        annotation_text="trigger 0.20",
        annotation_position="top right",
    )
    apply_layout(fig, "Current smaller/larger best-size ratio per runner", height=420)
    fig.update_xaxes(title_text="smaller / larger best size", range=[0, 1])
    fig.update_yaxes(title_text="sampled books")
    return fig


def panel_c_best_sizes(batches: pd.DataFrame) -> go.Figure:
    fig = make_subplots(
        rows=1,
        cols=2,
        subplot_titles=("Current best bid size", "Current best ask size"),
        shared_yaxes=False,
    )

    if batches.empty:
        apply_layout(fig, "Current best sizes per runner (no data)", height=480)
        return fig
    df = batches.copy().sort_values(["instrument", "n"])
    for color, inst in zip(PALETTE, sorted(df["instrument"].unique()), strict=False):
        sel = df[df["instrument"] == inst]
        fig.add_trace(
            go.Scatter(
                x=sel["n"],
                y=sel["bid"],
                mode="lines",
                line_shape="hv",
                name=inst,
                line={"color": color, "width": 1.4},
                showlegend=True,
            ),
            row=1,
            col=1,
        )
        fig.add_trace(
            go.Scatter(
                x=sel["n"],
                y=sel["ask"],
                mode="lines",
                line_shape="hv",
                line={"color": color, "width": 1.4},
                showlegend=False,
            ),
            row=1,
            col=2,
        )
    apply_layout(fig, "Current best bid (back) and ask (lay) sizes per runner", height=480)
    fig.update_xaxes(title_text="book update count", row=1, col=1)
    fig.update_xaxes(title_text="book update count", row=1, col=2)
    fig.update_yaxes(title_text="GBP", row=1, col=1)
    fig.update_yaxes(title_text="GBP", row=1, col=2)
    return fig


def main() -> None:
    batches = parse_log(LOG_PATH)
    print(f"snapshots={len(batches)} runners={batches['instrument'].nunique()}")
    panels = {
        "panel_a_imbalance_lines.png": panel_a_imbalance_lines(batches),
        "panel_b_ratio_distribution.png": panel_b_ratio_distribution(batches),
        "panel_c_best_sizes.png": panel_c_best_sizes(batches),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
