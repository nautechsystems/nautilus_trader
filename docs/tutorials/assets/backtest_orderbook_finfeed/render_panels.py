"""
Render book, liquidity, and round-trip panels from the FinFeed tutorial program.

Run from the repository root with the tutorial dependencies and a Chrome-compatible browser:

    uv run --project python --no-sync --with pandas --with plotly --with kaleido \
        python docs/tutorials/assets/backtest_orderbook_finfeed/render_panels.py

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
ROUND_TRIP_MARGIN = pd.Timedelta(seconds=4)


def main() -> None:
    """
    Run the tutorial program and export panels from its native reports.
    """
    run = runpy.run_path(str(PROGRAM), run_name="__main__")
    book = _top_of_book(run["deltas"], run["INSTRUMENT_ID"], run["BOOK_TYPE"])
    print(f"Spread range: {book['spread'].min()} to {book['spread'].max()} pUSD")
    executions = run["executions"].sort_values("ts_event", kind="stable")
    account = run["account"]
    fills = pd.to_datetime(executions["ts_event"], utc=True)
    window = [run["START"], run["END"]]
    round_trip_window = [fills.min() - ROUND_TRIP_MARGIN, fills.max() + ROUND_TRIP_MARGIN]

    panels = [
        ("panel_a_top_book.png", _book_figure(book, executions), window, 520),
        ("panel_b_liquidity.png", _liquidity_figure(book), window, 720),
        (
            "panel_c_round_trip.png",
            _round_trip_figure(book, executions, account),
            round_trip_window,
            640,
        ),
    ]

    for filename, figure, x_range, height in panels:
        apply_layout(figure, height=height)
        figure.update_xaxes(tickformat="%H:%M:%S", range=x_range)
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
    _add_book_traces(figure, book, executions)
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
                "text": f"Polymarket Bitcoin Yes shares | {book.index[0]:%Y-%m-%d}",
            },
        },
    )
    return figure


def _round_trip_figure(
    book: pd.DataFrame,
    executions: pd.DataFrame,
    account: pd.DataFrame,
) -> go.Figure:
    sides = executions["order_side"].tolist()
    if sides != ["BUY", "SELL"]:
        msg = f"Round-trip panel expects one buy then one sell, was {sides}"
        raise ValueError(msg)

    figure = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.14,
        row_heights=[0.6, 0.4],
        subplot_titles=(
            "Best bid and ask with the fills",
            "Equity change: cash plus the held share at the midpoint",
        ),
    )
    _add_book_traces(figure, book, executions, row=1, labels=True)

    equity = _equity_series(book, executions, account)
    figure.add_trace(
        go.Scatter(
            x=equity.index,
            y=equity.map(float),
            mode="lines",
            name="Equity change",
            line={"color": COLORS["primary"], "shape": "hv"},
            showlegend=False,
        ),
        row=2,
        col=1,
    )

    fills = pd.to_datetime(executions["ts_event"], utc=True)
    figure.add_vrect(
        x0=fills.min(),
        x1=fills.max(),
        fillcolor=COLORS["primary"],
        opacity=0.12,
        line_width=0,
        row="all",
        col=1,
    )

    buy = executions[executions["order_side"] == "BUY"].iloc[0]
    sell = executions[executions["order_side"] == "SELL"].iloc[-1]
    held = (fills.max() - fills.min()).total_seconds()
    figure.update_layout(
        title={
            "text": "One round trip: buy at the ask, sell at the bid",
            "subtitle": {
                "text": (
                    f"Bought at {buy.last_px}, sold {held:.1f} s later at {sell.last_px} "
                    f"(shaded) | Net PnL {equity.iloc[-1]:.6f} pUSD with zero fees"
                ),
            },
        },
    )
    figure.update_xaxes(title_text="Time (UTC)", row=2, col=1)
    figure.update_yaxes(title_text="pUSD", row=1, col=1)
    figure.update_yaxes(title_text="pUSD", zeroline=True, row=2, col=1)
    return figure


def _add_book_traces(
    figure: go.Figure,
    book: pd.DataFrame,
    executions: pd.DataFrame,
    *,
    row: int | None = None,
    labels: bool = False,
) -> None:
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
            row=row,
            col=None if row is None else 1,
        )

    for side, symbol, color, position in [
        ("BUY", "triangle-up", COLORS["positive"], "top center"),
        ("SELL", "triangle-down", COLORS["negative"], "bottom center"),
    ]:
        fills = executions[executions["order_side"] == side]
        figure.add_trace(
            go.Scatter(
                x=pd.to_datetime(fills["ts_event"], utc=True),
                y=fills["last_px"].map(float),
                mode="markers+text" if labels else "markers",
                text=[f"{side.title()} {price}" for price in fills["last_px"]] if labels else None,
                textposition=position,
                name=f"{side.title()} fills",
                marker={"symbol": symbol, "color": color, "size": 10 if labels else 7},
            ),
            row=row,
            col=None if row is None else 1,
        )


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
