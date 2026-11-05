"""
Render book, liquidity, position, and equity panels from the FinFeed tutorial.

Run from the repository root with the tutorial dependencies and a Chrome-compatible browser:

    uv run --project python --no-sync --with pandas --with plotly --with kaleido \
        python docs/tutorials/render_finfeed_panels.py

The tutorial reuses its cached catalog or performs its documented first collection.

"""

from __future__ import annotations

import runpy
import sys
from decimal import Decimal
from pathlib import Path

import pandas as pd
import plotly.graph_objects as go
from orderbook_panels import COLORS
from orderbook_panels import apply_layout
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _write_figure


TUTORIAL_DIR = Path(__file__).resolve().parent
OUT = TUTORIAL_DIR / "assets" / "backtest_orderbook_finfeed"


def main() -> None:
    """
    Replay the cached series and export panels from the native reports.
    """
    sys.path.insert(0, str(TUTORIAL_DIR))
    run = runpy.run_path(str(TUTORIAL_DIR / "backtest_orderbook_finfeed.py"))
    book = run["top_of_book"]
    executions = run["executions"].sort_values("ts_event", kind="stable")
    account = run["account"]
    panels = {
        "panel_a_top_book.png": _book_figure(book, executions),
        "panel_b_liquidity.png": _liquidity_figure(book),
        "panel_c_position.png": _position_figure(book, executions),
        "panel_d_pnl.png": _equity_figure(book, executions, account),
    }

    for filename, figure in panels.items():
        apply_layout(figure, height=720 if "liquidity" in filename else 520)
        figure.update_xaxes(
            title_text="Time (UTC)",
            tickformat="%H:%M:%S",
            range=[run["START"], run["END"]],
        )

        if "liquidity" in filename:
            figure.update_xaxes(title_text=None, row=1, col=1)
            figure.update_xaxes(title_text=None, row=2, col=1)
        _write_figure(figure, str(OUT / filename))


def _book_figure(book: pd.DataFrame, executions: pd.DataFrame) -> go.Figure:
    figure = go.Figure()

    for column, label, color in [
        ("bid", "Best bid", COLORS["positive"]),
        ("ask", "Best ask", COLORS["negative"]),
    ]:
        figure.add_trace(
            go.Scatter(
                x=book.index,
                y=book[column].map(float),
                mode="lines",
                name=label,
                line={"color": color, "shape": "hv", "width": 1},
            ),
        )
    for side, symbol, color in [
        ("BUY", "triangle-up", COLORS["positive"]),
        ("SELL", "triangle-down", COLORS["negative"]),
    ]:
        fills = executions[executions["order_side"] == side]
        figure.add_trace(
            go.Scatter(
                x=pd.to_datetime(fills["ts_event"], utc=True),
                y=fills["last_px"].map(float),
                mode="markers",
                name=f"{side.title()} fills",
                marker={"symbol": symbol, "color": color, "size": 7},
            ),
        )
    figure.update_layout(
        title={
            "text": "Bitcoin daily threshold: Yes-share book and executions",
            "subtitle": {
                "text": (
                    f"Polymarket | {book.index[0]:%Y-%m-%d} | All book events and execution fills"
                ),
            },
        },
    )
    figure.update_yaxes(title_text="Share price (pUSD)")
    return figure


def _liquidity_figure(book: pd.DataFrame) -> go.Figure:
    figure = make_subplots(
        rows=3,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.12,
        subplot_titles=("Bid-ask spread", "Best-quote sizes", "Best-quote size imbalance"),
    )

    for column, label, color, row in [
        ("spread", "Spread", COLORS["primary"], 1),
        ("bid_size", "Bid shares", COLORS["positive"], 2),
        ("ask_size", "Ask shares", COLORS["negative"], 2),
        ("imbalance", "Size imbalance", COLORS["primary"], 3),
    ]:
        figure.add_trace(
            go.Scatter(
                x=book.index,
                y=book[column].map(float),
                mode="lines",
                name=label,
                line={"color": color, "shape": "hv", "width": 1},
                showlegend=column in ("bid_size", "ask_size"),
            ),
            row=row,
            col=1,
        )
    figure.update_yaxes(title_text="pUSD", row=1, col=1)
    figure.update_yaxes(title_text="Shares", row=2, col=1)
    figure.update_yaxes(title_text="Imbalance", range=[-1, 1], row=3, col=1)
    figure.update_layout(
        title={
            "text": "Historical spread, depth, and imbalance",
            "subtitle": {
                "text": f"Polymarket Bitcoin Yes shares | {book.index[0]:%Y-%m-%d}",
            },
        },
    )
    return figure


def _position_series(book: pd.DataFrame, executions: pd.DataFrame) -> pd.Series:
    quantities = [
        Decimal(row.last_qty) * (1 if row.order_side == "BUY" else -1)
        for row in executions.itertuples()
    ]
    position = pd.Series(quantities).cumsum()
    position.index = pd.to_datetime(executions["ts_event"], utc=True)
    position = pd.concat([pd.Series([Decimal(0)], index=book.index[:1]), position])
    position = position.groupby(level=0, sort=False).last()
    return position.reindex(position.index.union(book.index[-1:])).ffill()


def _position_figure(book: pd.DataFrame, executions: pd.DataFrame) -> go.Figure:
    position = _position_series(book, executions)
    figure = go.Figure(
        go.Scatter(
            x=position.index,
            y=position.map(float),
            mode="lines",
            name="Yes shares held",
            line={"color": COLORS["positive"], "shape": "hv"},
            fill="tozeroy",
        ),
    )
    figure.update_layout(
        title={
            "text": "Yes-share inventory and final close",
            "subtitle": {
                "text": (
                    f"Peak long: {position.max():.6f} shares | "
                    f"Final position: {position.iloc[-1]:.6f} shares"
                ),
            },
        },
    )
    figure.update_yaxes(title_text="Shares held", rangemode="tozero")
    return figure


def _equity_figure(
    book: pd.DataFrame,
    executions: pd.DataFrame,
    account: pd.DataFrame,
) -> go.Figure:
    position = _position_series(book, executions)
    cash = account["total"].map(Decimal).groupby(level=0, sort=False).last()
    midpoint = ((book["bid"] + book["ask"]) / 2).groupby(level=0, sort=False).last()
    index = midpoint.index.union(position.index).union(cash.index).sort_values()
    shares = position.reindex(index).ffill().fillna(Decimal(0))
    balance = cash.reindex(index).ffill()
    marks = midpoint.reindex(index).ffill()
    pnl = balance + shares * marks - cash.iloc[0]
    figure = go.Figure(
        go.Scatter(
            x=pnl.index,
            y=pnl.map(float),
            mode="lines",
            name="Equity change",
            line={"color": COLORS["primary"], "shape": "hv"},
        ),
    )
    figure.update_layout(
        title={
            "text": "Cash plus Yes shares marked at the midpoint",
            "subtitle": {
                "text": (
                    f"Equity change: {pnl.iloc[-1]:.6f} pUSD | "
                    f"Final cash balance: {cash.iloc[-1]:,.6f} pUSD"
                ),
            },
        },
    )
    figure.update_yaxes(title_text="Equity change (pUSD)", zeroline=True)
    return figure


if __name__ == "__main__":
    main()
