"""
Render book, liquidity, and position panels from the FinFeed tutorial program.

Run from the repository root with the tutorial dependencies and a Chrome-compatible browser:

    uv run --project python --no-sync --with pandas --with plotly --with kaleido \
        python docs/tutorials/assets/backtest_orderbook_finfeed/render_panels.py

If Kaleido reports that Chrome is missing, install its browser once:

    uv run --project python --no-sync --with plotly --with kaleido plotly_get_chrome -y

The program reuses its cached catalog or performs its documented first collection.

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
from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import RecordFlag


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from orderbook_panels import COLORS
from orderbook_panels import apply_layout


PROGRAM = (
    Path(__file__).resolve().parents[4]
    / "examples"
    / "backtest"
    / "finfeed_polymarket_book_imbalance.py"
)
OUT = Path(__file__).resolve().parent
POSITIVE_FILL = _hex_to_rgba(COLORS["positive"], 0.14)


def main() -> None:
    """
    Run the tutorial program and export panels from its native reports.
    """
    run = runpy.run_path(str(PROGRAM), run_name="__main__")
    book = _top_of_book(run["deltas"], run["INSTRUMENT_ID"], run["BOOK_TYPE"])
    print(f"Spread range: {book['spread'].min()} to {book['spread'].max()} pUSD")
    executions = run["executions"].sort_values("ts_event", kind="stable")
    account = run["account"]
    window = [run["START"], run["END"] + pd.Timedelta(seconds=1)]

    panels = [
        ("panel_a_top_book.png", _book_figure(book, executions), 520),
        ("panel_b_liquidity.png", _liquidity_figure(book), 720),
        ("panel_c_position_pnl.png", _position_pnl_figure(book, executions, account), 640),
    ]

    for filename, figure, height in panels:
        apply_layout(figure, height=height)
        figure.update_xaxes(tickformat="%H:%M:%S", range=window)
        _write_figure(figure, str(OUT / filename))


def _top_of_book(
    deltas: list[OrderBookDelta],
    instrument_id: InstrumentId,
    book_type: BookType,
) -> pd.DataFrame:
    book = OrderBook(instrument_id, book_type)
    rows = []

    for delta in deltas:
        book.apply_delta(delta)
        if not RecordFlag.F_LAST.matches(delta.flags):
            continue  # The book is incomplete until the batch ends

        bid, ask = book.best_bid_price(), book.best_ask_price()
        bid_size, ask_size = book.best_bid_size(), book.best_ask_size()
        if bid is None or ask is None or bid_size is None or ask_size is None:
            continue  # One side of the book is empty

        bid_px, ask_px = bid.as_decimal(), ask.as_decimal()
        bid_qty, ask_qty = bid_size.as_decimal(), ask_size.as_decimal()

        if ask_px <= bid_px:
            continue  # Locked book: no spread

        rows.append(
            {
                "time": pd.Timestamp(delta.ts_init, tz="UTC"),
                "bid": bid_px,
                "ask": ask_px,
                "spread": ask_px - bid_px,
                "bid_size": bid_qty,
                "ask_size": ask_qty,
                "imbalance": (bid_qty - ask_qty) / (bid_qty + ask_qty),
            },
        )

    return pd.DataFrame(rows).set_index("time")


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

    limits = executions[executions["order_type"] == "LIMIT"]
    closes = executions[executions["order_type"] == "MARKET"]

    for fills, label, color, symbol, size in [
        (
            limits[limits["order_side"] == "BUY"],
            "FOK buy fills",
            COLORS["positive"],
            "triangle-up",
            6,
        ),
        (
            limits[limits["order_side"] == "SELL"],
            "FOK sell fill",
            COLORS["negative"],
            "triangle-down",
            8,
        ),
        (closes, "Final market sell fills", "#fbbf24", "diamond", 8),
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

    figure.update_layout(
        title={
            "text": "Bitcoin daily threshold: No-share book and executions",
            "subtitle": {
                "text": (
                    f"Polymarket | {book.index[0]:%Y-%m-%d} | All book events and execution fills"
                ),
            },
        },
    )
    figure.update_xaxes(title_text="Time (UTC)")
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
    figure.update_xaxes(title_text="Time (UTC)", row=3, col=1)
    figure.update_yaxes(title_text="pUSD", row=1, col=1)
    figure.update_yaxes(title_text="Shares", row=2, col=1)
    figure.update_yaxes(title_text="Imbalance", range=[-1, 1], row=3, col=1)
    figure.update_layout(
        title={
            "text": "Historical spread, depth, and imbalance",
            "subtitle": {
                "text": f"Polymarket Bitcoin No shares | {book.index[0]:%Y-%m-%d}",
            },
        },
    )
    return figure


def _position_pnl_figure(
    book: pd.DataFrame,
    executions: pd.DataFrame,
    account: pd.DataFrame,
) -> go.Figure:
    position = _position_series(book, executions)
    marked_pnl = _equity_series(book, executions, account)

    figure = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.14,
        row_heights=[0.45, 0.55],
        subplot_titles=(
            "Shares held",
            "PnL with the open position marked at the midpoint",
        ),
    )
    figure.add_trace(
        go.Scatter(
            x=position.index,
            y=position.map(float),
            mode="lines",
            name="Shares held",
            line={"color": COLORS["positive"], "width": 2, "shape": "hv"},
            fill="tozeroy",
            fillcolor=POSITIVE_FILL,
            showlegend=False,
        ),
        row=1,
        col=1,
    )
    figure.add_trace(
        go.Scatter(
            x=marked_pnl.index,
            y=marked_pnl.map(float),
            mode="lines",
            name="PnL marked at the midpoint",
            line={"color": COLORS["primary"], "width": 1.5, "shape": "hv"},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    figure.add_trace(
        go.Scatter(
            x=marked_pnl.index[-1:],
            y=marked_pnl.iloc[-1:].map(float),
            mode="markers",
            marker={"color": COLORS["negative"], "size": 8},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    figure.add_annotation(
        x=marked_pnl.index[-1],
        y=float(marked_pnl.iloc[-1]),
        text=f"Flat: {marked_pnl.iloc[-1]:.6f} pUSD realized",
        ax=-150,
        ay=-60,
        arrowcolor=COLORS["muted"],
        font={"color": COLORS["text"]},
        row=2,
        col=1,
    )
    figure.update_layout(
        title={
            "text": "Long position and mark-to-market PnL",
            "subtitle": {
                "text": (
                    f"Peak long {position.max()} shares | Realized PnL {marked_pnl.iloc[-1]:.6f} "
                    "pUSD with zero fees"
                ),
            },
        },
    )
    figure.update_xaxes(title_text="Time (UTC)", row=2, col=1)
    figure.update_yaxes(title_text="Shares", zeroline=True, row=1, col=1)
    figure.update_yaxes(title_text="pUSD", zeroline=True, row=2, col=1)
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


def _equity_series(
    book: pd.DataFrame,
    executions: pd.DataFrame,
    account: pd.DataFrame,
) -> pd.Series:
    position = _position_series(book, executions)
    cash = account["total"].map(Decimal).groupby(level=0, sort=False).last()
    midpoint = ((book["bid"] + book["ask"]) / 2).groupby(level=0, sort=False).last()
    index = midpoint.index.union(position.index).union(cash.index).sort_values()
    shares = position.reindex(index).ffill().fillna(Decimal(0))
    balance = cash.reindex(index).ffill()
    marks = midpoint.reindex(index).ffill()
    return balance + shares * marks - cash.iloc[0]


if __name__ == "__main__":
    main()
