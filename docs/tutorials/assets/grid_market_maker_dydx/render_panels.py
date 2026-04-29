"""
Render the dYdX grid market maker tutorial panels from a captured live run.

Usage:

    # Capture a live run (mainnet by default; pass DYDX_NETWORK=testnet for testnet
    # if you have an API trading key configured for the testnet wallet).
    timeout 35 ./target/release/examples/dydx-grid-mm > /tmp/dydx_main.log 2>&1

    uv sync --extra visualization
    DYDX_LOG=/tmp/dydx_main.log \
        python3 docs/tutorials/assets/grid_market_maker_dydx/render_panels.py

The renderer parses ``Requoting`` lines for the mid trajectory and
``[SUBMIT_ORDER]`` / ``OrderAccepted`` / ``OrderCanceled`` events for the
order lifecycle, then writes four PNG panels using the ``nautilus_dark``
tearsheet theme.

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
LOG_PATH = Path(os.environ.get("DYDX_LOG", "/tmp/dydx_main.log"))  # noqa: S108

GRID_STEP_BPS = 100
NUM_LEVELS = 3

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TS = r"(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)"
REQUOTE = re.compile(rf"{TS}.*Requoting grid: mid=(?P<mid>[\-0-9.]+)")
SUBMIT = re.compile(
    rf"{TS}.*\[SUBMIT_ORDER\] Nautilus '(?P<id>[^']+)' .*side=(?P<side>Buy|Sell) qty=(?P<qty>[\-0-9.]+)",
)
ACCEPTED = re.compile(rf"{TS}.*OrderAccepted\(.*?client_order_id=(?P<id>[^,]+),")
CANCELED = re.compile(rf"{TS}.*OrderCanceled\(.*?client_order_id=(?P<id>[^,]+),")


def parse_log(path: Path):
    requotes: list[dict] = []
    submits: dict[str, dict] = {}
    accepts: dict[str, pd.Timestamp] = {}
    cancels: dict[str, pd.Timestamp] = {}

    for raw in path.read_text().splitlines():
        line = ANSI.sub("", raw)
        m = REQUOTE.search(line)
        if m:
            requotes.append(
                {"ts": pd.Timestamp(m.group("ts")), "mid": float(m.group("mid"))},
            )
            continue
        s = SUBMIT.search(line)
        if s:
            submits[s.group("id")] = {
                "ts": pd.Timestamp(s.group("ts")),
                "side": s.group("side").upper(),
                "qty": float(s.group("qty")),
            }
            continue
        a = ACCEPTED.search(line)
        if a:
            accepts[a.group("id")] = pd.Timestamp(a.group("ts"))
            continue
        c = CANCELED.search(line)
        if c:
            cancels[c.group("id")] = pd.Timestamp(c.group("ts"))
            continue
    return (
        pd.DataFrame(requotes),
        submits,
        accepts,
        cancels,
    )


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


def panel_a_grid_overlay(requotes: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if requotes.empty:
        apply_layout(fig, "No requote events captured", height=520)
        return fig
    df = requotes.copy().drop_duplicates(subset=["ts", "mid"]).reset_index(drop=True)
    pct = GRID_STEP_BPS / 10_000.0
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["mid"],
            mode="lines+markers",
            name="Mid at requote",
            line={"color": PRIMARY, "width": 1.6},
            marker={"size": 6, "color": PRIMARY},
        ),
    )

    for level in range(1, NUM_LEVELS + 1):
        fig.add_trace(
            go.Scatter(
                x=df["ts"],
                y=df["mid"] * (1 - pct) ** level,
                mode="lines",
                name=f"Buy L{level}",
                line={"color": POSITIVE, "width": 0.8, "dash": "dot"},
                opacity=0.7,
            ),
        )
        fig.add_trace(
            go.Scatter(
                x=df["ts"],
                y=df["mid"] * (1 + pct) ** level,
                mode="lines",
                name=f"Sell L{level}",
                line={"color": NEGATIVE, "width": 0.8, "dash": "dot"},
                opacity=0.7,
            ),
        )
    apply_layout(
        fig,
        f"ETH-USD-PERP mid at every requote with theoretical grid bands "
        f"({GRID_STEP_BPS} bps step, {NUM_LEVELS} levels)",
        height=520,
    )
    fig.update_yaxes(title_text="USD")
    return fig


def panel_b_order_lifetime(submits, accepts, cancels) -> go.Figure:
    fig = go.Figure()
    rows: list[dict] = []

    for cid, sub in submits.items():
        if cid in accepts and cid in cancels:
            rows.append(
                {
                    "ts": accepts[cid],
                    "side": sub["side"],
                    "lifetime_secs": (cancels[cid] - accepts[cid]).total_seconds(),
                },
            )
    if not rows:
        apply_layout(fig, "Order lifetime distribution (no completed orders)", height=420)
        return fig
    df = pd.DataFrame(rows)
    fig.add_trace(
        go.Histogram(
            x=df["lifetime_secs"],
            nbinsx=30,
            marker={"color": PRIMARY, "line": {"color": COLORS["background"], "width": 0.5}},
            showlegend=False,
        ),
    )
    fig.add_vline(
        x=8.0,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.2},
        annotation_text="expire_time_secs = 8",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
    )
    apply_layout(
        fig,
        "Time from `OrderAccepted` to `OrderCanceled` per short-term order",
        height=420,
    )
    fig.update_xaxes(title_text="seconds")
    fig.update_yaxes(title_text="orders")
    return fig


def panel_c_orders_per_cycle(submits, accepts) -> go.Figure:
    fig = go.Figure()
    if not submits:
        apply_layout(fig, "Orders per requote cycle (no orders)", height=420)
        return fig
    rows = pd.DataFrame(
        [
            {
                "ts": v["ts"],
                "side": v["side"],
                "accepted": k in accepts,
            }
            for k, v in submits.items()
        ],
    )
    rows["bucket"] = rows["ts"].dt.floor("250ms")
    counts = (
        rows.groupby("bucket")
        .agg(
            buys=("side", lambda s: (s == "BUY").sum()),
            sells=("side", lambda s: (s == "SELL").sum()),
            accepted=("accepted", "sum"),
        )
        .reset_index()
    )
    fig.add_trace(
        go.Bar(
            x=counts["bucket"],
            y=counts["buys"],
            name="Buy submissions",
            marker={"color": POSITIVE},
        ),
    )
    fig.add_trace(
        go.Bar(
            x=counts["bucket"],
            y=counts["sells"],
            name="Sell submissions",
            marker={"color": NEGATIVE},
        ),
    )
    fig.update_layout(barmode="stack")
    apply_layout(fig, "Orders submitted per 250-ms bucket", height=420)
    fig.update_xaxes(title_text="bucket")
    fig.update_yaxes(title_text="orders")
    return fig


def panel_d_short_term_timeline() -> go.Figure:
    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.10,
        row_heights=[0.6, 0.4],
        subplot_titles=(
            "Short-term order time-to-live (expire_time_secs=8)",
            "Block height growth (~0.5s per block)",
        ),
    )

    # Three orders submitted at t=0, 8, 16 (one per requote cycle).
    for start in (0, 8, 16):
        fig.add_trace(
            go.Scatter(
                x=[start, start + 8],
                y=[8, 0],
                mode="lines",
                line={"color": PRIMARY, "width": 2.0},
                showlegend=False,
            ),
            row=1,
            col=1,
        )
        fig.add_vline(x=start, line={"color": POSITIVE, "dash": "dot", "width": 1}, row=1, col=1)
        fig.add_vline(
            x=start + 8,
            line={"color": NEGATIVE, "dash": "dot", "width": 1},
            row=1,
            col=1,
        )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1}, row=1, col=1)

    # GoodTilBlock target moves with chain height. With 0.5s blocks, +16 blocks ~ 8s.
    blocks = list(range(0, 51, 1))
    fig.add_trace(
        go.Scatter(
            x=[b * 0.5 for b in blocks],
            y=blocks,
            mode="lines",
            line={"color": PRIMARY, "width": 1.6},
            showlegend=False,
        ),
        row=2,
        col=1,
    )

    apply_layout(
        fig,
        "Short-term order lifecycle: submit at t=0, 8, 16, expire 8 s later",
        height=520,
    )
    fig.update_xaxes(title_text="seconds since first submission", row=2, col=1, range=[0, 25])
    fig.update_yaxes(title_text="seconds remaining", range=[0, 9], row=1, col=1)
    fig.update_yaxes(title_text="block height", row=2, col=1)
    return fig


def main() -> None:
    requotes, submits, accepts, cancels = parse_log(LOG_PATH)
    print(
        f"requotes={len(requotes)} submits={len(submits)} "
        f"accepts={len(accepts)} cancels={len(cancels)}",
    )
    panels = {
        "panel_a_grid_overlay.png": panel_a_grid_overlay(requotes),
        "panel_b_order_lifetime.png": panel_b_order_lifetime(submits, accepts, cancels),
        "panel_c_orders_per_cycle.png": panel_c_orders_per_cycle(submits, accepts),
        "panel_d_short_term_timeline.png": panel_d_short_term_timeline(),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
