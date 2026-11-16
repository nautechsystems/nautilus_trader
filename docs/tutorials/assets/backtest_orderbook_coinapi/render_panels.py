"""
Render the CoinAPI tutorial's panels from its pinned Lighter ANTHROPIC backtest.

Run from the repository root with the tutorial and visualization dependencies installed:

    uv run --project python --no-sync --with pandas --with plotly --with kaleido \
        python docs/tutorials/assets/backtest_orderbook_coinapi/render_panels.py

The tutorial reuses its cached export or performs its documented first download.
The figures use the same native replay and reports as the tutorial.

"""

from __future__ import annotations

import runpy
import sys
from decimal import Decimal
from pathlib import Path

import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _hex_to_rgba
from nautilus_trader.analysis.tearsheet import _padded_range
from nautilus_trader.analysis.tearsheet import _write_figure


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from orderbook_panels import COLORS
from orderbook_panels import apply_layout


TUTORIAL_DIR = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
NEGATIVE_FILL = _hex_to_rgba(COLORS["negative"], 0.14)


def main() -> None:
    """
    Run the tutorial and write panels from its book and execution reports.
    """
    sys.path.insert(0, str(TUTORIAL_DIR))
    run = runpy.run_path(str(TUTORIAL_DIR / "backtest_orderbook_coinapi.py"))
    book = run["top_of_book"]
    executions = run["executions"]
    account = run["account"]

    panels = {
        "panel_a_top_book.png": _book_figure(book, executions),
        "panel_b_liquidity.png": _liquidity_figure(book),
        "panel_c_position.png": _position_figure(book, executions),
        "panel_d_pnl.png": _pnl_figure(book, account),
    }

    for filename, figure in panels.items():
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
                line={"color": color, "width": 1, "shape": "hv"},
            ),
        )

    limits = executions[executions["order_type"] == "LIMIT"]
    closes = executions[executions["order_type"] == "MARKET"]

    for fills, label, color, symbol, size in [
        (
            limits[limits["order_side"] == "SELL"],
            "FOK sell fills",
            COLORS["negative"],
            "triangle-down",
            6,
        ),
        (
            limits[limits["order_side"] == "BUY"],
            "FOK buy fill",
            COLORS["positive"],
            "triangle-up",
            8,
        ),
        (closes, "Final market buy fills", "#fbbf24", "diamond", 8),
    ]:
        figure.add_trace(
            go.Scatter(
                x=pd.to_datetime(fills["ts_event"], utc=True),
                y=fills["last_px"].map(float),
                mode="markers",
                name=label,
                marker={
                    "color": color,
                    "symbol": symbol,
                    "size": size,
                    "line": {"color": COLORS["background"], "width": 0.7},
                },
            ),
        )

    _apply_layout(
        figure,
        "ANTHROPIC pre-IPO perpetual: book and executions",
        f"Lighter | {book.index[0]:%Y-%m-%d} | All book events and execution fills",
        book,
    )
    figure.update_yaxes(title_text="Price (USDC)", tickformat=".1f")
    return figure


def _liquidity_figure(book: pd.DataFrame) -> go.Figure:
    figure = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.14,
        subplot_titles=("Bid-ask spread", "Liquidity at the best bid and ask"),
    )
    figure.add_trace(
        go.Scatter(
            x=book.index,
            y=book["spread"].map(float),
            mode="lines",
            name="Spread",
            line={"color": COLORS["primary"], "width": 1, "shape": "hv"},
            showlegend=False,
        ),
        row=1,
        col=1,
    )

    for column, label, color in [
        ("bid_size", "Best bid size", COLORS["positive"]),
        ("ask_size", "Best ask size", COLORS["negative"]),
    ]:
        figure.add_trace(
            go.Scatter(
                x=book.index,
                y=book[column].map(float),
                mode="lines",
                name=label,
                line={"color": color, "width": 1, "shape": "hv"},
            ),
            row=2,
            col=1,
        )

    _apply_layout(
        figure,
        "Spread and best-quote depth",
        "Lighter ANTHROPIC | Full hour | Depth uses a logarithmic scale",
        book,
        height=680,
    )
    figure.update_xaxes(title_text=None, row=1, col=1)
    figure.update_layout(legend={"y": 0.47})
    figure.update_yaxes(title_text="Spread (USDC)", rangemode="tozero", row=1, col=1)
    ticks = [0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10]
    figure.update_yaxes(
        title_text="Contract units (log)",
        type="log",
        tickmode="array",
        tickvals=ticks,
        ticktext=[f"{value:g}" for value in ticks],
        row=2,
        col=1,
    )
    return figure


def _position_figure(book: pd.DataFrame, executions: pd.DataFrame) -> go.Figure:
    ordered = executions.sort_values("ts_event", kind="stable")
    quantities = [
        Decimal(row.last_qty) * (1 if row.order_side == "BUY" else -1)
        for row in ordered.itertuples()
    ]
    position = pd.Series(quantities).cumsum()
    position.index = pd.to_datetime(ordered["ts_event"], utc=True)
    position = pd.concat([pd.Series([Decimal(0)], index=book.index[:1]), position])
    position = position.groupby(level=0, sort=False).last()

    figure = go.Figure(
        go.Scatter(
            x=position.index,
            y=position.map(float),
            mode="lines",
            name="Net position",
            line={"color": COLORS["negative"], "width": 2, "shape": "hv"},
            fill="tozeroy",
            fillcolor=NEGATIVE_FILL,
            showlegend=False,
        ),
    )

    _apply_layout(
        figure,
        "Position accumulation and final close",
        f"Peak short: {-position.min():.5f} units | Final position: {position.iloc[-1]:.5f} units",
        book,
    )
    figure.update_yaxes(
        title_text="Net position (contract units)",
        zeroline=True,
        tickformat=".1f",
        range=_padded_range(float(position.min()), float(position.max())),
    )
    figure.add_trace(
        go.Scatter(
            x=position.index[-1:],
            y=position.iloc[-1:].map(float),
            mode="markers+text",
            text=["Flat"],
            textposition="bottom left",
            marker={"color": COLORS["positive"], "size": 8},
            showlegend=False,
        ),
    )
    return figure


def _pnl_figure(book: pd.DataFrame, account: pd.DataFrame) -> go.Figure:
    totals = account["total"].map(Decimal)
    pnl = (totals - totals.iloc[0]).groupby(level=0, sort=False).last()

    figure = go.Figure(
        go.Scatter(
            x=pnl.index,
            y=pnl.map(float),
            mode="lines",
            name="Realized PnL",
            line={"color": COLORS["negative"], "width": 2, "shape": "hv"},
            fill="tozeroy",
            fillcolor=NEGATIVE_FILL,
            showlegend=False,
        ),
    )

    _apply_layout(
        figure,
        "Realized PnL and final account balance",
        f"Realized PnL: {pnl.iloc[-1]:.8f} USDC | Final balance: {totals.iloc[-1]:,.8f} USDC",
        book,
    )
    figure.update_yaxes(
        title_text="Realized PnL (USDC)",
        zeroline=True,
        tickformat=".2f",
        range=_padded_range(float(pnl.min()), float(pnl.max())),
    )
    figure.add_trace(
        go.Scatter(
            x=pnl.index[-1:],
            y=pnl.iloc[-1:].map(float),
            mode="markers+text",
            text=["Final close"],
            textposition="top left",
            marker={"color": COLORS["negative"], "size": 8},
            showlegend=False,
        ),
    )
    return figure


def _apply_layout(
    figure: go.Figure,
    title: str,
    subtitle: str,
    book: pd.DataFrame,
    *,
    height: int = 520,
) -> None:
    figure.update_layout(
        title={
            "text": title,
            "subtitle": {"text": subtitle},
        },
    )
    apply_layout(figure, height=height)
    figure.update_xaxes(
        title_text="Time (UTC)",
        tickformat="%H:%M",
        dtick=600_000,
        range=[
            book.index[0].floor("h") - pd.Timedelta(seconds=45),
            book.index[-1].ceil("min") + pd.Timedelta(seconds=45),
        ],
    )


if __name__ == "__main__":
    main()
