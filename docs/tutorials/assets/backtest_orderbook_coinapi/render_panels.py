"""
Render book, liquidity, and position panels from the CoinAPI tutorial program.

Run from the repository root with the tutorial dependencies and a Chrome-compatible browser:

    uv run --project python --no-sync --with pandas --with plotly --with kaleido \
        python docs/tutorials/assets/backtest_orderbook_coinapi/render_panels.py

The program reuses its cached catalog or performs its documented first download.

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
    / "coinapi_lighter_book_imbalance.py"
)
OUT = Path(__file__).resolve().parent
NEGATIVE_FILL = _hex_to_rgba(COLORS["negative"], 0.14)
LASTING_STATE = pd.Timedelta(milliseconds=1)
SHORT_INTERVAL = pd.Timedelta(microseconds=10)


def main() -> None:
    """
    Run the tutorial program and export panels from its native reports.
    """
    run = runpy.run_path(str(PROGRAM), run_name="__main__")
    book = _top_of_book(run["deltas"], run["INSTRUMENT_ID"], run["BOOK_TYPE"])
    executions = run["executions"].sort_values("ts_event", kind="stable")
    account = run["account"]

    durations = book["duration"].dropna()
    lasting = book.loc[book["duration"] >= LASTING_STATE, "spread"]
    print(f"Spread range: {book['spread'].min()} to {book['spread'].max()} USDC")
    print(f"Spread range for states lasting 1 ms or more: {lasting.min()} to {lasting.max()} USDC")
    print(f"Intervals under 10 us: {(durations < SHORT_INTERVAL).sum():,} of {len(durations):,}")

    panels = {
        "panel_a_top_book.png": _book_figure(book, executions),
        "panel_b_liquidity.png": _liquidity_figure(book),
        "panel_c_position_pnl.png": _position_pnl_figure(book, executions, account),
    }

    for filename, figure in panels.items():
        figure.update_xaxes(
            tickformat="%H:%M",
            dtick=600_000,
            range=[
                book.index[0].floor("h") - pd.Timedelta(seconds=45),
                book.index[-1].ceil("min") + pd.Timedelta(seconds=45),
            ],
        )
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

        state = {"time": pd.Timestamp(delta.ts_init, tz="UTC")}
        rows.append(state)

        bid, ask = book.best_bid_price(), book.best_ask_price()
        bid_size, ask_size = book.best_bid_size(), book.best_ask_size()
        if bid is None or ask is None or bid_size is None or ask_size is None:
            continue  # One side of the book is empty

        bid_px, ask_px = bid.as_decimal(), ask.as_decimal()
        if ask_px <= bid_px:
            continue  # Locked or crossed book: no spread

        state.update(
            {
                "bid": bid_px,
                "ask": ask_px,
                "spread": ask_px - bid_px,
                "bid_size": bid_size.as_decimal(),
                "ask_size": ask_size.as_decimal(),
            },
        )

    states = pd.DataFrame(rows)
    states["duration"] = (
        states["time"].shift(-1) - states["time"]
    )  # Until the next batch of any kind
    return states.dropna(subset=["spread"]).set_index("time")


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

    apply_layout(figure)
    figure.update_layout(
        title={
            "text": "ANTHROPIC pre-IPO perpetual: book and executions",
            "subtitle": {
                "text": f"Lighter | {book.index[0]:%Y-%m-%d} | All book events and execution fills",
            },
        },
    )
    figure.update_xaxes(title_text="Time (UTC)")
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

    ticks = [0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10]
    apply_layout(figure, height=680)
    figure.update_layout(
        title={
            "text": "Spread and best-quote depth",
            "subtitle": {"text": "Lighter ANTHROPIC | Full hour | Depth uses a logarithmic scale"},
        },
        legend={"y": 0.47},
    )
    figure.update_xaxes(title_text="Time (UTC)", row=2, col=1)
    figure.update_yaxes(title_text="Spread (USDC)", rangemode="tozero", row=1, col=1)
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


def _position_pnl_figure(
    book: pd.DataFrame,
    executions: pd.DataFrame,
    account: pd.DataFrame,
) -> go.Figure:
    position, marked_pnl = _position_and_marked_pnl(book, executions)
    totals = account["total"].map(Decimal).groupby(level=0, sort=False).last()
    realized_pnl = totals - totals.iloc[0]
    if marked_pnl.iloc[-1] != realized_pnl.iloc[-1]:
        msg = (
            f"Marked PnL {marked_pnl.iloc[-1]} does not match the balance change "
            f"{realized_pnl.iloc[-1]} after the close"
        )
        raise ValueError(msg)

    figure = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.14,
        row_heights=[0.45, 0.55],
        subplot_titles=(
            "Net position",
            "PnL with the open position marked at the midpoint",
        ),
    )
    figure.add_trace(
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
            x=realized_pnl.index[-1:],
            y=realized_pnl.iloc[-1:].map(float),
            mode="markers",
            marker={"color": COLORS["negative"], "size": 8},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    figure.add_annotation(
        x=realized_pnl.index[-1],
        y=float(realized_pnl.iloc[-1]),
        text=f"Flat: {realized_pnl.iloc[-1]} USDC realized",
        ax=-130,
        ay=-90,
        arrowcolor=COLORS["muted"],
        font={"color": COLORS["text"]},
        row=2,
        col=1,
    )

    apply_layout(figure, height=640)
    figure.update_layout(
        title={
            "text": "Short position and mark-to-market PnL",
            "subtitle": {
                "text": (
                    f"Peak short {-position.min()} units | Realized PnL {realized_pnl.iloc[-1]} "
                    "USDC with zero fees"
                ),
            },
        },
    )
    figure.update_xaxes(title_text="Time (UTC)", row=2, col=1)
    figure.update_yaxes(title_text="Contract units", zeroline=True, row=1, col=1)
    figure.update_yaxes(title_text="USDC", zeroline=True, row=2, col=1)
    return figure


def _position_and_marked_pnl(
    book: pd.DataFrame,
    executions: pd.DataFrame,
) -> tuple[pd.Series, pd.Series]:
    times = pd.to_datetime(executions["ts_event"], utc=True)
    quantities = [
        Decimal(row.last_qty) * (1 if row.order_side == "BUY" else -1)
        for row in executions.itertuples()
    ]
    cash_flows = [
        -quantity * Decimal(price)
        for quantity, price in zip(quantities, executions["last_px"], strict=True)
    ]
    position = pd.Series(quantities, index=times).cumsum().groupby(level=0, sort=False).last()
    cash = pd.Series(cash_flows, index=times).cumsum().groupby(level=0, sort=False).last()
    midpoint = ((book["bid"] + book["ask"]) / 2).groupby(level=0, sort=False).last()

    index = midpoint.index.union(position.index).sort_values()
    held = position.reindex(index).ffill().fillna(Decimal(0))
    flows = cash.reindex(index).ffill().fillna(Decimal(0))
    marks = midpoint.reindex(index).ffill()
    return held, flows + held * marks


if __name__ == "__main__":
    main()
