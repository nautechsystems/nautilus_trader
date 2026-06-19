"""
Render the Lighter NVDA composite market maker tutorial panels.

Usage:

    uv sync --extra visualization
    python3 docs/tutorials/assets/lighter_rwa_composite_mm/render_panels.py

The renderer uses deterministic replay data to show the quoting equations and
cash-session operating constraint without shipping a Databento capture or a
live Lighter account trace.

"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme


OUT = Path(__file__).resolve().parent

SYMBOL = "NVDA"
HALF_SPREAD_BPS = 25
SIGNAL_SKEW_FACTOR = 55.0
INVENTORY_SKEW_FACTOR = 2.0
MAX_POSITION = 0.20
TRADE_SIZE = 0.05

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]
BACKGROUND = COLORS["background"]


def apply_layout(fig: go.Figure, title: str, height: int = 480) -> None:
    fig.update_layout(
        template=TEMPLATE,
        title={"text": title, "x": 0.02, "xanchor": "left"},
        paper_bgcolor=BACKGROUND,
        plot_bgcolor=BACKGROUND,
        font={"family": "Inter, system-ui, sans-serif", "size": 13},
        margin={"l": 60, "r": 30, "t": 70, "b": 50},
        height=height,
        width=1200,
        legend={"orientation": "h", "yanchor": "bottom", "y": 1.02, "xanchor": "right", "x": 1.0},
    )
    fig.update_xaxes(gridcolor=GRID, zeroline=False)
    fig.update_yaxes(gridcolor=GRID, zeroline=False)


def build_replay() -> pd.DataFrame:
    ts = pd.date_range("2026-06-17 13:30:00+00:00", periods=390, freq="1min")
    x = np.linspace(0.0, 1.0, len(ts))

    databento_mid = (
        207.20
        + 2.10 * x
        + 0.85 * np.sin(2 * np.pi * x * 2.2)
        + 0.45 * np.exp(-(((x - 0.62) / 0.08) ** 2))
        - 0.35 * np.exp(-(((x - 0.22) / 0.05) ** 2))
    )
    basis_bps = 5.5 * np.sin(2 * np.pi * x * 3.0 + 0.4) + 10.0 * np.exp(
        -(((x - 0.66) / 0.055) ** 2),
    )
    lighter_mid = databento_mid * (1 + basis_bps / 10_000.0)

    position = np.zeros(len(ts))
    position[55:125] = TRADE_SIZE
    position[125:205] = TRADE_SIZE * 2
    position[205:260] = TRADE_SIZE
    position[260:335] = -TRADE_SIZE

    baseline = databento_mid[0]
    signal_residual = databento_mid / baseline - 1.0
    signal_shift = SIGNAL_SKEW_FACTOR * signal_residual
    inventory_shift = INVENTORY_SKEW_FACTOR * position
    total_shift = signal_shift - inventory_shift
    quote_center = lighter_mid + total_shift
    half_spread = lighter_mid * (HALF_SPREAD_BPS / 10_000.0)

    return pd.DataFrame(
        {
            "ts": ts,
            "databento_mid": databento_mid,
            "lighter_mid": lighter_mid,
            "basis_bps": basis_bps,
            "position": position,
            "signal_residual_bps": signal_residual * 10_000.0,
            "signal_shift": signal_shift,
            "inventory_shift": inventory_shift,
            "total_shift": total_shift,
            "quote_center": quote_center,
            "bid": quote_center - half_spread,
            "ask": quote_center + half_spread,
            "quote_shift_bps": total_shift / lighter_mid * 10_000.0,
        },
    )


def build_session_frame() -> pd.DataFrame:
    ts = pd.date_range("2026-06-17 12:00:00+00:00", periods=337, freq="5min")
    cash_open = pd.Timestamp("2026-06-17 13:30:00+00:00")
    cash_close = pd.Timestamp("2026-06-17 20:00:00+00:00")
    is_cash_session = (ts >= cash_open) & (ts <= cash_close)

    stale_minutes = np.zeros(len(ts))
    last_databento_ts = cash_open

    for i, current in enumerate(ts):
        if is_cash_session[i]:
            last_databento_ts = current
        stale_minutes[i] = max((current - last_databento_ts).total_seconds() / 60.0, 0.0)

    return pd.DataFrame(
        {
            "ts": ts,
            "lighter_active": 1.0,
            "databento_active": is_cash_session.astype(float),
            "stale_minutes": stale_minutes,
        },
    )


def panel_a_reference_overlay(df: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["databento_mid"],
            mode="lines",
            name=f"Databento {SYMBOL}.EQUS mid",
            line={"color": PRIMARY, "width": 1.7},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["lighter_mid"],
            mode="lines",
            name=f"Lighter {SYMBOL}-PERP mid",
            line={"color": NEUTRAL, "width": 1.2},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["ask"],
            mode="lines",
            name="Composite ask",
            line={"color": NEGATIVE, "width": 0.9, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["bid"],
            mode="lines",
            name="Composite bid",
            line={"color": POSITIVE, "width": 0.9, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["quote_center"],
            mode="lines",
            name="Quote center",
            line={"color": "#eeeeee", "width": 1.2},
        ),
    )
    apply_layout(
        fig,
        f"{SYMBOL} composite quote center against Databento signal and Lighter anchor",
        height=540,
    )
    fig.update_yaxes(title_text="USD")
    return fig


def panel_b_signal_basis(df: pd.DataFrame) -> go.Figure:
    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.52, 0.48],
        subplot_titles=("Signal residual and Lighter basis", "Resulting quote-center shift"),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["signal_residual_bps"],
            mode="lines",
            name="Databento residual",
            line={"color": PRIMARY, "width": 1.4},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["basis_bps"],
            mode="lines",
            name="Lighter basis",
            line={"color": NEUTRAL, "width": 1.0},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["quote_shift_bps"],
            mode="lines",
            name="Quote-center shift",
            line={"color": POSITIVE, "width": 1.4},
        ),
        row=2,
        col=1,
    )
    fig.add_hline(y=0, line={"color": GRID, "width": 1}, row=1, col=1)
    fig.add_hline(y=0, line={"color": GRID, "width": 1}, row=2, col=1)
    apply_layout(
        fig,
        "Databento residual, Lighter basis, and composite quote-center shift",
        height=620,
    )
    fig.update_yaxes(title_text="bps", row=1, col=1)
    fig.update_yaxes(title_text="bps", row=2, col=1)
    return fig


def panel_c_inventory_skew(df: pd.DataFrame) -> go.Figure:
    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.45, 0.55],
        subplot_titles=("Net position", "Price-unit skew terms"),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["position"],
            mode="lines",
            name="Net position",
            line={"color": PRIMARY, "width": 1.7, "shape": "hv"},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["signal_shift"],
            mode="lines",
            name="Signal shift",
            line={"color": POSITIVE, "width": 1.3},
        ),
        row=2,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=-df["inventory_shift"],
            mode="lines",
            name="Inventory adjustment",
            line={"color": NEGATIVE, "width": 1.3},
        ),
        row=2,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["total_shift"],
            mode="lines",
            name="Total shift",
            line={"color": "#eeeeee", "width": 1.4},
        ),
        row=2,
        col=1,
    )
    fig.add_hline(
        y=MAX_POSITION,
        line={"color": NEUTRAL, "dash": "dash", "width": 1},
        row=1,
        col=1,
    )
    fig.add_hline(
        y=-MAX_POSITION,
        line={"color": NEUTRAL, "dash": "dash", "width": 1},
        row=1,
        col=1,
    )
    fig.add_hline(y=0, line={"color": GRID, "width": 1}, row=2, col=1)
    apply_layout(
        fig,
        f"Inventory skew with {TRADE_SIZE:.2f} {SYMBOL} trade size and {MAX_POSITION:.2f} cap",
        height=620,
    )
    fig.update_yaxes(title_text=f"{SYMBOL}", row=1, col=1)
    fig.update_yaxes(title_text="USD", row=2, col=1)
    return fig


def panel_d_session_clock(df: pd.DataFrame) -> go.Figure:
    fig = make_subplots(specs=[[{"secondary_y": True}]])
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["lighter_active"],
            mode="lines",
            name="Lighter RWA market",
            line={"color": PRIMARY, "width": 1.5, "shape": "hv"},
        ),
        secondary_y=False,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["databento_active"],
            mode="lines",
            name="Databento EQUS.PLUS feed",
            line={"color": POSITIVE, "width": 1.5, "shape": "hv"},
        ),
        secondary_y=False,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["stale_minutes"],
            mode="lines",
            name="Signal age",
            line={"color": NEGATIVE, "width": 1.3},
        ),
        secondary_y=True,
    )
    fig.add_vrect(
        x0="2026-06-17T13:30:00+00:00",
        x1="2026-06-17T20:00:00+00:00",
        fillcolor=PRIMARY,
        opacity=0.10,
        line_width=0,
        annotation_text="regular session",
        annotation_position="top left",
    )
    apply_layout(
        fig,
        "Lighter trades continuously while the Databento equity signal has a cash-session clock",
        height=500,
    )
    fig.update_yaxes(title_text="active flag", range=[-0.05, 1.15], secondary_y=False)
    fig.update_yaxes(title_text="minutes", secondary_y=True)
    return fig


def main() -> None:
    replay = build_replay()
    session = build_session_frame()
    panels = {
        "panel_a_reference_overlay.png": panel_a_reference_overlay(replay),
        "panel_b_signal_basis.png": panel_b_signal_basis(replay),
        "panel_c_inventory_skew.png": panel_c_inventory_skew(replay),
        "panel_d_session_clock.png": panel_d_session_clock(session),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
