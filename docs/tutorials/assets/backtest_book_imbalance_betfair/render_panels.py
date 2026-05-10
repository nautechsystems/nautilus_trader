"""
Render the Betfair book imbalance tutorial panels from a backtest log.

Usage:

    # Run the example with a fine log interval so the panels have detail.
    IMBALANCE_LOG_INTERVAL=200 \
        cargo run -p nautilus-betfair --features examples --release \
        --example betfair-backtest > /tmp/betfair.log 2>&1

    uv sync --extra visualization
    BETFAIR_LOG=/tmp/betfair.log \
        python3 docs/tutorials/assets/backtest_book_imbalance_betfair/render_panels.py

The actor logs ``[runner] update #N: batch bid=B ask=A cumulative imbalance=I``
on every Nth update. The renderer parses those lines and writes three PNG
panels using the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

import os
import re
from pathlib import Path

import numpy as np
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

ANSI = re.compile(r"\x1b\[[0-9;]*m")
BATCH = re.compile(
    r"\[(?P<inst>[^\]]+)\] update #(?P<n>\d+): "
    r"batch bid=(?P<bid>[\-0-9.]+) ask=(?P<ask>[\-0-9.]+)\s+cumulative imbalance=(?P<imb>[\-0-9.]+)",
)
SUMMARY = re.compile(
    r"\s+(?P<inst>[^\s]+\.BETFAIR)\s+updates:\s*(?P<u>\d+)\s+bid_vol:\s*(?P<b>[\-0-9.]+)\s+ask_vol:\s*(?P<a>[\-0-9.]+)\s+imbalance:\s*(?P<i>[\-0-9.]+)",
)


def parse_log(path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    batches: list[dict] = []
    summary: list[dict] = []

    for line in path.read_text().splitlines():
        line = ANSI.sub("", line)
        m = BATCH.search(line)
        if m:
            batches.append(
                {
                    "instrument": m.group("inst"),
                    "n": int(m.group("n")),
                    "batch_bid": float(m.group("bid")),
                    "batch_ask": float(m.group("ask")),
                    "imbalance": float(m.group("imb")),
                },
            )
            continue
        s = SUMMARY.search(line)
        if s:
            summary.append(
                {
                    "instrument": s.group("inst"),
                    "updates": int(s.group("u")),
                    "bid_vol": float(s.group("b")),
                    "ask_vol": float(s.group("a")),
                    "imbalance": float(s.group("i")),
                },
            )
    return pd.DataFrame(batches), pd.DataFrame(summary)


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
        legend={"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1.0},
    )
    fig.update_xaxes(gridcolor=GRID, zeroline=False)
    fig.update_yaxes(gridcolor=GRID, zeroline=False)


def panel_a_imbalance_lines(batches: pd.DataFrame, summary: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if batches.empty:
        apply_layout(fig, "Cumulative imbalance per runner (no data)", height=480)
        return fig
    palette = [PRIMARY, POSITIVE, NEGATIVE, NEUTRAL]
    runners = sorted(batches["instrument"].unique())
    for color, inst in zip(palette, runners, strict=False):
        sel = batches[batches["instrument"] == inst].sort_values("n")
        fig.add_trace(
            go.Scatter(
                x=sel["n"],
                y=sel["imbalance"],
                mode="lines+markers",
                name=inst,
                line={"color": color, "width": 1.4},
                marker={"size": 4, "color": color},
            ),
        )
    fig.add_hline(y=0.0, line={"color": GRID, "width": 1})
    if not summary.empty:
        for color, (_, row) in zip(
            palette,
            summary.sort_values("instrument").iterrows(),
            strict=False,
        ):
            fig.add_hline(
                y=row["imbalance"],
                line={"color": color, "dash": "dash", "width": 1},
                annotation_text=f"final {row['imbalance']:+.3f}",
                annotation_position="top right",
                annotation={"font": {"size": 11, "color": color}},
            )
    apply_layout(
        fig,
        "Cumulative quoted-volume imbalance per runner across all updates",
        height=520,
    )
    fig.update_xaxes(title_text="cumulative update count")
    fig.update_yaxes(title_text="imbalance = (bid - ask) / (bid + ask)", range=[-1.05, 1.05])
    return fig


def panel_b_batch_distribution(batches: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if batches.empty:
        apply_layout(fig, "Per-batch signed flow distribution (no data)", height=420)
        return fig
    df = batches.copy()
    df["batch_signed"] = df["batch_bid"] - df["batch_ask"]
    df["batch_total"] = df["batch_bid"] + df["batch_ask"]
    df["batch_ratio"] = np.where(
        df["batch_total"] > 0,
        df["batch_signed"] / df["batch_total"],
        np.nan,
    )
    runners = sorted(df["instrument"].unique())
    palette = [PRIMARY, POSITIVE, NEGATIVE, NEUTRAL]
    for color, inst in zip(palette, runners, strict=False):
        sel = df[df["instrument"] == inst].dropna(subset=["batch_ratio"])
        fig.add_trace(
            go.Histogram(
                x=sel["batch_ratio"],
                nbinsx=30,
                name=inst,
                marker={"color": color, "line": {"color": COLORS["background"], "width": 0.4}},
                opacity=0.7,
            ),
        )
    fig.update_layout(barmode="overlay")
    fig.add_vline(x=0.0, line={"color": GRID, "width": 1})
    apply_layout(fig, "Per-batch signed-flow ratio per runner", height=420)
    fig.update_xaxes(title_text="(bid - ask) / (bid + ask)")
    fig.update_yaxes(title_text="batches")
    return fig


def panel_c_cumulative_volume(batches: pd.DataFrame) -> go.Figure:
    fig = make_subplots(
        rows=1,
        cols=2,
        subplot_titles=("Cumulative bid volume", "Cumulative ask volume"),
        shared_yaxes=False,
    )

    if batches.empty:
        apply_layout(fig, "Cumulative volume per runner (no data)", height=480)
        return fig
    df = batches.copy().sort_values(["instrument", "n"])
    df["cum_bid"] = df.groupby("instrument")["batch_bid"].cumsum()
    df["cum_ask"] = df.groupby("instrument")["batch_ask"].cumsum()
    palette = [PRIMARY, POSITIVE, NEGATIVE, NEUTRAL]
    for color, inst in zip(palette, sorted(df["instrument"].unique()), strict=False):
        sel = df[df["instrument"] == inst]
        fig.add_trace(
            go.Scatter(
                x=sel["n"],
                y=sel["cum_bid"],
                mode="lines",
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
                y=sel["cum_ask"],
                mode="lines",
                line={"color": color, "width": 1.4},
                showlegend=False,
            ),
            row=1,
            col=2,
        )
    apply_layout(fig, "Cumulative bid (back) and ask (lay) volume per runner", height=480)
    fig.update_xaxes(title_text="cumulative update count", row=1, col=1)
    fig.update_xaxes(title_text="cumulative update count", row=1, col=2)
    fig.update_yaxes(title_text="GBP", row=1, col=1)
    fig.update_yaxes(title_text="GBP", row=1, col=2)
    return fig


def main() -> None:
    batches, summary = parse_log(LOG_PATH)
    print(f"batches={len(batches)} summary_rows={len(summary)}")
    if not summary.empty:
        print(summary.to_string(index=False))
    panels = {
        "panel_a_imbalance_lines.png": panel_a_imbalance_lines(batches, summary),
        "panel_b_batch_distribution.png": panel_b_batch_distribution(batches),
        "panel_c_cumulative_volume.png": panel_c_cumulative_volume(batches),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
