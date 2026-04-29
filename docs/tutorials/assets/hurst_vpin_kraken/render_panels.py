"""
Render the Hurst/VPIN tutorial's diagnostic panels from real backtest output.

Usage:

    # Capture a backtest log against multi-day Tardis PF_XBTUSD data.
    RUST_LOG=info cargo run -p nautilus-kraken --features examples \\
        --example kraken-hurst-vpin-backtest --release > /tmp/backtest.log 2>&1

    # Parse the log and regenerate the five PNG panels next to this script.
    BACKTEST_LOG=/tmp/backtest.log \\
        python3 docs/tutorials/assets/hurst_vpin_kraken/render_panels.py

Requires the ``visualization`` extra for Kaleido/Plotly:
    uv sync --extra visualization

Parses per-bar signal snapshots and ``OrderFilled`` events from the strategy's
info-level log, then writes five PNGs using the ``nautilus_dark`` tearsheet
theme.

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

LOG_PATH = Path(os.environ.get("BACKTEST_LOG", "/tmp/backtest.log"))  # noqa: S108
OUT = Path(__file__).resolve().parent

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

HURST_ENTER = 0.55
HURST_EXIT = 0.50
VPIN_THRESHOLD = 0.30

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TS = r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)"
BAR = re.compile(
    rf"{TS}.*Hurst=([\-0-9.]+) VPIN=([\-0-9.]+) signed=([+\-0-9.]+) bar_close=([\-0-9.]+)",
)
FILL = re.compile(
    rf"{TS}.*OrderFilled\(.*?order_side=([A-Z]+).*?last_qty=([0-9.]+),\s*last_px=([0-9_.]+)",
)


def parse_log(path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    bars: list[dict] = []
    fills: list[dict] = []

    for raw in path.read_text().splitlines():
        line = ANSI.sub("", raw)
        m = BAR.search(line)
        if m:
            bars.append(
                {
                    "ts": pd.Timestamp(m.group(1)),
                    "hurst": float(m.group(2)),
                    "vpin": float(m.group(3)),
                    "signed_vpin": float(m.group(4)),
                    "close": float(m.group(5)),
                },
            )
            continue
        f = FILL.search(line)
        if f:
            fills.append(
                {
                    "ts": pd.Timestamp(f.group(1)),
                    "side": f.group(2),
                    "qty": float(f.group(3)),
                    "price": float(f.group(4).replace("_", "")),
                },
            )
    return pd.DataFrame(bars), pd.DataFrame(fills)


def walk_fills(
    fills: pd.DataFrame,
) -> tuple[list[dict], list[dict], list[dict], list[tuple[pd.Timestamp, pd.Timestamp | None]]]:
    """
    Walk the fill sequence tracking running net position on a Netting OMS.

    Returns entries, partial reductions, full closes, and (open_ts, close_ts)
    intervals per position cycle. ``close_ts`` is None for a position that is
    still open at the end of the fill series.

    """
    entries: list[dict] = []
    partials: list[dict] = []
    closes: list[dict] = []
    intervals: list[tuple[pd.Timestamp, pd.Timestamp | None]] = []

    pos = 0.0
    open_ts: pd.Timestamp | None = None
    open_side = 0
    EPS = 1e-9

    for _, row in fills.iterrows():
        delta = row["qty"] if row["side"] == "BUY" else -row["qty"]
        new_pos = pos + delta

        if abs(pos) < EPS and abs(new_pos) >= EPS:
            open_side = 1 if new_pos > 0 else -1
            open_ts = row["ts"]
            entries.append(
                {
                    "ts": row["ts"],
                    "price": row["price"],
                    "side": open_side,
                    "qty": abs(delta),
                },
            )
        elif abs(pos) >= EPS and abs(new_pos) < abs(pos):
            record = {
                "ts": row["ts"],
                "price": row["price"],
                "side": -open_side,
                "qty": abs(delta),
            }

            if abs(new_pos) < EPS:
                closes.append(record)
                intervals.append((open_ts, row["ts"]))
                open_ts = None
                open_side = 0
            else:
                partials.append(record)

        pos = new_pos

    if open_ts is not None:
        intervals.append((open_ts, None))

    return entries, partials, closes, intervals


def in_position_mask(
    ts: pd.DatetimeIndex,
    intervals: list[tuple[pd.Timestamp, pd.Timestamp | None]],
) -> np.ndarray:
    mask = np.zeros(len(ts), dtype=bool)
    for start, end in intervals:
        stop = end if end is not None else ts[-1]
        mask |= (ts >= start) & (ts <= stop)
    return mask


def apply_layout(fig: go.Figure, title: str, height: int = 500) -> None:
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


def _contiguous_runs(mask: np.ndarray) -> list[tuple[int, int]]:
    runs: list[tuple[int, int]] = []
    start = None
    for i, v in enumerate(mask):
        if v and start is None:
            start = i
        elif not v and start is not None:
            runs.append((start, i - 1))
            start = None
    if start is not None:
        runs.append((start, len(mask) - 1))
    return runs


def _filter_fills(records, lo, hi):
    return [e for e in records if lo <= e["ts"] <= hi]


def _draw_fill_connectors(fig, records, bars, row=None, col=None):
    # Thin dotted segment from the bar-close polyline (interpolated at the
    # fill timestamp) to the actual execution price. Makes the slip between
    # fill price and reference line visually obvious without moving markers
    # off their true y-value.
    if not records or bars.empty:
        return
    kw = {}
    if row is not None:
        kw["row"] = row
        kw["col"] = col
    xs = bars["ts"].astype("int64").to_numpy()
    ys = bars["close"].to_numpy()
    for r in records:
        line_y = float(np.interp(pd.Timestamp(r["ts"]).value, xs, ys))
        fig.add_trace(
            go.Scatter(
                x=[r["ts"], r["ts"]],
                y=[line_y, r["price"]],
                mode="lines",
                line={"color": "#eeeeee", "width": 1, "dash": "dot"},
                showlegend=False,
                hoverinfo="skip",
            ),
            **kw,
        )


def _draw_fill_markers(fig, entries, partials, closes, row=None, col=None, show_legend=True):
    kw = {}
    if row is not None:
        kw["row"] = row
        kw["col"] = col
    long_e = [e for e in entries if e["side"] == 1]
    short_e = [e for e in entries if e["side"] == -1]

    if long_e:
        fig.add_trace(
            go.Scatter(
                x=[e["ts"] for e in long_e],
                y=[e["price"] for e in long_e],
                mode="markers",
                name="Long entry" if show_legend else None,
                marker={
                    "symbol": "triangle-up",
                    "size": 13,
                    "color": POSITIVE,
                    "line": {"color": "white", "width": 1.2},
                },
                showlegend=show_legend,
            ),
            **kw,
        )
    if short_e:
        fig.add_trace(
            go.Scatter(
                x=[e["ts"] for e in short_e],
                y=[e["price"] for e in short_e],
                mode="markers",
                name="Short entry" if show_legend else None,
                marker={
                    "symbol": "triangle-down",
                    "size": 13,
                    "color": NEGATIVE,
                    "line": {"color": "white", "width": 1.2},
                },
                showlegend=show_legend,
            ),
            **kw,
        )
    if closes:
        fig.add_trace(
            go.Scatter(
                x=[e["ts"] for e in closes],
                y=[e["price"] for e in closes],
                mode="markers",
                name="Close" if show_legend else None,
                marker={
                    "symbol": "x",
                    "size": 11,
                    "color": "#eeeeee",
                    "line": {"color": "black", "width": 1},
                },
                showlegend=show_legend,
            ),
            **kw,
        )
    if partials:
        fig.add_trace(
            go.Scatter(
                x=[e["ts"] for e in partials],
                y=[e["price"] for e in partials],
                mode="markers",
                name="Partial cover" if show_legend else None,
                marker={
                    "symbol": "circle-open",
                    "size": 12,
                    "color": "#eeeeee",
                    "line": {"color": "#eeeeee", "width": 2},
                },
                showlegend=show_legend,
            ),
            **kw,
        )


def panel_a_price_regime(
    bars: pd.DataFrame,
    entries,
    partials,
    closes,
    in_pos,
    zoom: tuple[pd.Timestamp, pd.Timestamp] | None = None,
) -> go.Figure:
    if zoom is not None:
        lo, hi = zoom
        sel = (bars["ts"] >= lo) & (bars["ts"] <= hi)
        bars = bars.loc[sel].reset_index(drop=True)
        in_pos = in_pos[sel.to_numpy()]
        entries = _filter_fills(entries, lo, hi)
        partials = _filter_fills(partials, lo, hi)
        closes = _filter_fills(closes, lo, hi)
    fig = go.Figure()
    ts = bars["ts"]
    for a, b in _contiguous_runs(bars["hurst"].to_numpy() >= HURST_ENTER):
        fig.add_vrect(
            x0=ts.iloc[a],
            x1=ts.iloc[b],
            fillcolor=POSITIVE,
            opacity=0.10,
            line_width=0,
            layer="below",
        )
    for a, b in _contiguous_runs(in_pos):
        fig.add_vrect(
            x0=ts.iloc[a],
            x1=ts.iloc[b],
            fillcolor="#f5b700",
            opacity=0.28,
            line_width=0,
            layer="below",
        )
    fig.add_trace(
        go.Scatter(
            x=ts,
            y=bars["close"],
            mode="lines",
            name="Close",
            line={"color": PRIMARY, "width": 1.6},
        ),
    )
    _draw_fill_connectors(fig, entries + partials + closes, bars)
    _draw_fill_markers(fig, entries, partials, closes)
    fig.add_trace(
        go.Scatter(
            x=[None],
            y=[None],
            mode="markers",
            marker={"size": 14, "color": POSITIVE, "opacity": 0.35, "symbol": "square"},
            name="Hurst >= 0.55",
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=[None],
            y=[None],
            mode="markers",
            marker={"size": 14, "color": "#f5b700", "opacity": 0.45, "symbol": "square"},
            name="In position",
        ),
    )
    title = "PF_XBTUSD close with trending-regime bands and trades"
    if zoom is not None:
        title += f" ({zoom[0].strftime('%Y-%m-%d %H:%M')} - {zoom[1].strftime('%H:%M')} UTC)"
    apply_layout(fig, title)
    fig.update_yaxes(title_text="USD")
    return fig


def panel_b_dashboard(
    bars: pd.DataFrame,
    entries,
    partials,
    closes,
    zoom: tuple[pd.Timestamp, pd.Timestamp] | None = None,
) -> go.Figure:
    if zoom is not None:
        lo, hi = zoom
        bars = bars.loc[(bars["ts"] >= lo) & (bars["ts"] <= hi)].reset_index(drop=True)
        entries = _filter_fills(entries, lo, hi)
        partials = _filter_fills(partials, lo, hi)
        closes = _filter_fills(closes, lo, hi)
    fig = make_subplots(
        rows=3,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.05,
        row_heights=[0.5, 0.25, 0.25],
    )
    ts = bars["ts"]
    fig.add_trace(
        go.Scatter(
            x=ts,
            y=bars["close"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.6},
            showlegend=False,
        ),
        row=1,
        col=1,
    )
    _draw_fill_connectors(fig, entries + partials + closes, bars, row=1, col=1)
    _draw_fill_markers(fig, entries, partials, closes, row=1, col=1, show_legend=False)

    fig.add_trace(
        go.Scatter(
            x=ts,
            y=bars["hurst"],
            mode="lines",
            line={"color": POSITIVE, "width": 1.3},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    fig.add_hline(
        y=HURST_ENTER,
        line={"color": POSITIVE, "dash": "dash", "width": 1},
        annotation_text="enter 0.55",
        annotation_position="top left",
        annotation={"font": {"size": 11, "color": POSITIVE}},
        row=2,
        col=1,
    )
    fig.add_hline(
        y=HURST_EXIT,
        line={"color": NEGATIVE, "dash": "dash", "width": 1},
        annotation_text="exit 0.50",
        annotation_position="bottom left",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
        row=2,
        col=1,
    )

    fig.add_trace(
        go.Scatter(
            x=ts,
            y=bars["vpin"],
            mode="lines",
            name="VPIN |",
            line={"color": NEUTRAL, "width": 1.3},
        ),
        row=3,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=ts,
            y=bars["signed_vpin"],
            mode="lines",
            name="VPIN signed",
            line={"color": PRIMARY, "width": 1.2, "dash": "dot"},
        ),
        row=3,
        col=1,
    )
    fig.add_hline(
        y=VPIN_THRESHOLD,
        line={"color": POSITIVE, "dash": "dash", "width": 1},
        annotation_text="threshold 0.30",
        annotation_position="top left",
        annotation={"font": {"size": 11, "color": POSITIVE}},
        row=3,
        col=1,
    )
    fig.add_hline(
        y=-VPIN_THRESHOLD,
        line={"color": NEGATIVE, "dash": "dash", "width": 1},
        row=3,
        col=1,
    )
    fig.add_hline(y=0.0, line={"color": GRID, "width": 1}, row=3, col=1)

    title = "Signal dashboard: price, Hurst, VPIN"
    if zoom is not None:
        title += f" ({zoom[0].strftime('%Y-%m-%d %H:%M')} - {zoom[1].strftime('%H:%M')} UTC)"
    apply_layout(fig, title, height=780)
    fig.update_yaxes(title_text="USD", row=1, col=1)
    fig.update_yaxes(title_text="Hurst", row=2, col=1)
    fig.update_yaxes(title_text="VPIN", row=3, col=1)
    return fig


def panel_c_decision_scatter(bars: pd.DataFrame) -> go.Figure:
    h = bars["hurst"].to_numpy()
    v = bars["vpin"].to_numpy()
    s = bars["signed_vpin"].to_numpy()

    x_lo = float(min(h.min(), HURST_EXIT - 0.02))
    x_hi = float(max(h.max(), HURST_ENTER + 0.02))
    y_lo = 0.0
    y_hi = float(max(v.max(), VPIN_THRESHOLD + 0.05))
    x_pad = (x_hi - x_lo) * 0.04
    y_pad = (y_hi - y_lo) * 0.05

    fig = go.Figure()
    fig.add_shape(
        type="rect",
        x0=HURST_ENTER,
        x1=x_hi + x_pad,
        y0=VPIN_THRESHOLD,
        y1=y_hi + y_pad,
        fillcolor=POSITIVE,
        opacity=0.12,
        line_width=0,
        layer="below",
    )

    eligible = (h >= HURST_ENTER) & (v >= VPIN_THRESHOLD)
    for subset, size in ((~eligible, 6), (eligible, 11)):
        if subset.any():
            fig.add_trace(
                go.Scatter(
                    x=h[subset],
                    y=v[subset],
                    mode="markers",
                    marker={
                        "size": size,
                        "color": s[subset],
                        "colorscale": [[0, NEGATIVE], [0.5, NEUTRAL], [1, POSITIVE]],
                        "cmin": -max(0.001, float(np.nanmax(np.abs(s)))),
                        "cmax": max(0.001, float(np.nanmax(np.abs(s)))),
                        "showscale": bool((subset == eligible).all()),
                        "colorbar": {"title": "signed VPIN"},
                        "line": {"width": 0.5, "color": COLORS["background"]},
                    },
                    showlegend=False,
                ),
            )

    fig.add_vline(x=HURST_ENTER, line={"color": POSITIVE, "dash": "dash", "width": 1})
    fig.add_vline(x=HURST_EXIT, line={"color": NEGATIVE, "dash": "dash", "width": 1})
    fig.add_hline(y=VPIN_THRESHOLD, line={"color": POSITIVE, "dash": "dash", "width": 1})

    # Threshold labels placed inside plot area above data.
    label_y = y_hi + y_pad * 0.15
    fig.add_annotation(
        x=HURST_ENTER,
        y=label_y,
        xanchor="left",
        text=f"Hurst enter {HURST_ENTER}",
        showarrow=False,
        font={"size": 12, "color": POSITIVE},
    )
    fig.add_annotation(
        x=HURST_EXIT,
        y=label_y,
        xanchor="right",
        text=f"Hurst exit {HURST_EXIT}",
        showarrow=False,
        font={"size": 12, "color": NEGATIVE},
    )
    fig.add_annotation(
        x=x_hi + x_pad * 0.5,
        y=VPIN_THRESHOLD,
        xanchor="right",
        yanchor="bottom",
        text=f"VPIN threshold {VPIN_THRESHOLD}",
        showarrow=False,
        font={"size": 12, "color": POSITIVE},
    )
    apply_layout(fig, "Decision space: Hurst vs VPIN per bar (shaded = entry-eligible)", height=600)
    fig.update_xaxes(title_text="Hurst", range=[x_lo - x_pad, x_hi + x_pad])
    fig.update_yaxes(title_text="VPIN", range=[y_lo, y_hi + y_pad * 2])
    return fig


def panel_d_vpin_hist(bars: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    fig.add_trace(
        go.Histogram(
            x=bars["vpin"],
            nbinsx=40,
            marker={
                "color": PRIMARY,
                "line": {"color": COLORS["background"], "width": 0.5},
            },
        ),
    )
    fig.add_vline(
        x=VPIN_THRESHOLD,
        line={"color": POSITIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"threshold {VPIN_THRESHOLD}",
        annotation_position="top right",
        annotation={"font": {"size": 12, "color": POSITIVE}},
    )
    apply_layout(fig, "VPIN distribution across bars", height=420)
    fig.update_xaxes(title_text="VPIN")
    fig.update_yaxes(title_text="count")
    return fig


def panel_e_hurst_only(bars: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=bars["hurst"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.6},
            showlegend=False,
        ),
    )
    fig.add_hline(
        y=HURST_ENTER,
        line={"color": POSITIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"enter {HURST_ENTER}",
        annotation_position="top right",
        annotation={"font": {"size": 12, "color": POSITIVE}},
    )
    fig.add_hline(
        y=HURST_EXIT,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"exit {HURST_EXIT}",
        annotation_position="bottom right",
        annotation={"font": {"size": 12, "color": NEGATIVE}},
    )
    fig.add_hline(y=0.5, line={"color": GRID, "width": 1})
    apply_layout(fig, "Hurst exponent (R/S regression) over the backtest", height=420)
    fig.update_yaxes(title_text="Hurst")
    return fig


def main() -> None:
    bars, fills = parse_log(LOG_PATH)
    bars = bars.sort_values("ts").reset_index(drop=True)
    entries, partials, closes, intervals = walk_fills(fills)
    in_pos = in_position_mask(pd.DatetimeIndex(bars["ts"]), intervals)
    print(
        f"bars={len(bars)} fills={len(fills)} entries={len(entries)} "
        f"partials={len(partials)} closes={len(closes)} intervals={len(intervals)}",
    )
    print(f"bar span: {bars['ts'].min()} -> {bars['ts'].max()}")

    # Zoom to the active trading window (first day with fills).
    fill_events = entries + partials + closes
    zoom_start = min(e["ts"] for e in entries) - pd.Timedelta(minutes=30)
    last_event = (
        max(e["ts"] for e in fill_events) if fill_events else zoom_start + pd.Timedelta(hours=2)
    )
    zoom_end = last_event + pd.Timedelta(minutes=30)

    panels = {
        "panel_a_price_regime.png": panel_a_price_regime(
            bars,
            entries,
            partials,
            closes,
            in_pos,
            zoom=(zoom_start, zoom_end),
        ),
        "panel_b_dashboard.png": panel_b_dashboard(
            bars,
            entries,
            partials,
            closes,
            zoom=(zoom_start, zoom_end),
        ),
        "panel_c_decision_scatter.png": panel_c_decision_scatter(bars),
        "panel_d_vpin_hist.png": panel_d_vpin_hist(bars),
        "panel_e_hurst_only.png": panel_e_hurst_only(bars),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
