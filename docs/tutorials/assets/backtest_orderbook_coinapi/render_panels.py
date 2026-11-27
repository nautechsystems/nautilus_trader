"""
Render book, liquidity, and position panels from the CoinAPI tutorial program.

Run from the repository root with the tutorial dependencies and a Chrome-compatible browser:

    uv run --project python --no-sync --with pandas --with plotly --with kaleido \
        python docs/tutorials/assets/backtest_orderbook_coinapi/render_panels.py

If Kaleido reports that Chrome is missing, install its browser once:

    uv run --project python --no-sync --with plotly --with kaleido plotly_get_chrome -y

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
            selector=lambda axis: axis.range is None,
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
            continue

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
    end = book.index[-1]
    start = end - pd.Timedelta(seconds=30)
    window = book.loc[book.index >= start]
    figure = make_subplots(
        rows=2,
        cols=1,
        vertical_spacing=0.22,
        subplot_titles=("Best ask over the hour", "Final 30 seconds and closing fills"),
    )

    for quotes, label, color, row in [
        (book["ask"], "Best ask", COLORS["negative"], 1),
        (window["bid"], "Best bid", COLORS["positive"], 2),
        (window["ask"], "Best ask", COLORS["negative"], 2),
    ]:
        prices = _step_changes(quotes)
        figure.add_trace(
            go.Scatter(
                x=prices.index,
                y=prices.map(float),
                mode="lines",
                name=label,
                line={"color": color, "width": 1, "shape": "hv"},
                showlegend=row == 2,
            ),
            row=row,
            col=1,
        )

    executions = executions.loc[pd.to_datetime(executions["ts_event"], utc=True) >= start]
    limits = executions[executions["order_type"] == "LIMIT"]
    closes = executions[executions["order_type"] == "MARKET"]

    for fills, label, color, symbol, size in [
        (
            limits[limits["order_side"] == "SELL"],
            "FOK sell fills",
            COLORS["negative"],
            "triangle-down",
            7,
        ),
        (
            limits[limits["order_side"] == "BUY"],
            "FOK buy fills",
            COLORS["positive"],
            "triangle-up",
            7,
        ),
        (closes, "Closing market fills", "#fbbf24", "diamond", 8),
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
            row=2,
            col=1,
        )

    apply_layout(figure, height=730)
    figure.update_layout(
        title={
            "text": "ANT equity perpetual order book and fills",
            "subtitle": {
                "text": f"Lighter | {book.index[0]:%Y-%m-%d}",
            },
        },
    )
    figure.update_xaxes(
        title_text="Time (UTC)",
        tickformat="%H:%M:%S",
        dtick=5000,
        range=[start, end + pd.Timedelta(seconds=1)],
        row=2,
        col=1,
    )
    figure.update_yaxes(title_text="Price (USDC)", tickformat=".2f")
    return figure


def _liquidity_figure(book: pd.DataFrame) -> go.Figure:
    figure = make_subplots(
        rows=3,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.14,
        subplot_titles=("Bid-ask spread", "Best bid size", "Best ask size"),
    )
    spreads = _step_changes(book["spread"])
    figure.add_trace(
        go.Scatter(
            x=spreads.index,
            y=spreads.map(float),
            mode="lines",
            name="Spread",
            line={"color": COLORS["primary"], "width": 1, "shape": "hv"},
            showlegend=False,
        ),
        row=1,
        col=1,
    )

    min_depth = min(book["bid_size"].min(), book["ask_size"].min())
    max_depth = max(book["bid_size"].max(), book["ask_size"].max())
    depth_range = [min_depth.adjusted() - 0.1, max_depth.adjusted() + 1]

    for row, column, label, color in [
        (2, "bid_size", "Best bid size", COLORS["positive"]),
        (3, "ask_size", "Best ask size", COLORS["negative"]),
    ]:
        sizes = _step_changes(book[column])
        figure.add_trace(
            go.Scatter(
                x=sizes.index,
                y=sizes.map(float),
                mode="lines",
                name=label,
                line={"color": color, "width": 1, "shape": "hv"},
                showlegend=False,
            ),
            row=row,
            col=1,
        )
        figure.update_yaxes(
            title_text="Contract units (log)",
            type="log",
            dtick=1,
            tickformat=".4~f",
            range=depth_range,
            row=row,
            col=1,
        )

    apply_layout(figure, height=870)
    figure.update_layout(
        title={
            "text": "Spread and best-quote depth",
            "subtitle": {"text": "Lighter ANT | Full hour | Depth uses a logarithmic scale"},
        },
    )
    figure.update_xaxes(title_text="Time (UTC)", row=3, col=1)
    figure.update_yaxes(title_text="Spread (USDC)", rangemode="tozero", row=1, col=1)
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

    position = _step_changes(position)
    marked_pnl = _step_changes(marked_pnl)
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
        ay=-30,
        arrowcolor=COLORS["muted"],
        font={"color": COLORS["text"]},
        row=2,
        col=1,
    )

    apply_layout(figure, height=640)
    figure.update_layout(
        title={
            "text": "Net position and mark-to-market PnL",
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


def _step_changes(series: pd.Series) -> pd.Series:
    changed = series.ne(series.shift())
    changed.iloc[-1] = True  # Extend the final step to the end of the series
    return series.loc[changed]


if __name__ == "__main__":
    main()
