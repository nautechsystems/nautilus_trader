"""
Render the Bybit order book imbalance tutorial panels from a backtest run.

Usage:

    uv sync --extra visualization
    NAUTILUS_DATA_DIR=tests/test_data/local \
        python3 docs/tutorials/assets/backtest_orderbook_bybit/render_panels.py

Replays the same Bybit ob500 XRPUSDT 2024-12-01 deltas as the tutorial, runs
the shipped ``OrderBookImbalance`` strategy alongside a sampling actor that
records top-of-book once per second, and writes four PNG panels to the same
directory using the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

import os
from decimal import Decimal
from pathlib import Path

import numpy as np
import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.adapters.bybit.loaders import BybitOrderBookDeltaDataLoader
from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.currencies import XRP
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


OUT = Path(__file__).resolve().parent
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "Bybit"

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]


class TopBookSamplerConfig(ActorConfig, frozen=True):
    instrument_id: InstrumentId
    sample_every_secs: int = 1


class TopBookSampler(Actor):
    def __init__(self, config: TopBookSamplerConfig) -> None:
        super().__init__(config)
        self.samples: list[dict] = []
        self._last_sample_ns = 0
        self._interval_ns = config.sample_every_secs * 1_000_000_000

    def on_start(self) -> None:
        self.subscribe_order_book_deltas(self.config.instrument_id, BookType.L2_MBP)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        ts = deltas.ts_event
        if ts - self._last_sample_ns < self._interval_ns:
            return
        book = self.cache.order_book(self.config.instrument_id)
        if book is None or not book.spread():
            return
        bid = book.best_bid_price()
        ask = book.best_ask_price()
        bid_size = book.best_bid_size()
        ask_size = book.best_ask_size()

        if bid is None or ask is None:
            return
        self.samples.append(
            {
                "ts": pd.Timestamp(ts, unit="ns", tz="UTC"),
                "bid": float(bid),
                "ask": float(ask),
                "bid_size": float(bid_size) if bid_size is not None else 0.0,
                "ask_size": float(ask_size) if ask_size is not None else 0.0,
            },
        )
        self._last_sample_ns = ts


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


def run_backtest(nrows: int = 1_000_000):
    update_path = DATA_DIR / "2024-12-01_XRPUSDT_ob500.data.zip"
    df_raw = BybitOrderBookDeltaDataLoader.load(update_path, nrows=nrows)

    XRPUSDT_BYBIT = TestInstrumentProvider.xrpusdt_linear_bybit()
    wrangler = OrderBookDeltaDataWrangler(XRPUSDT_BYBIT)
    deltas = wrangler.process(df_raw)
    deltas.sort(key=lambda x: x.ts_init)

    config = BacktestEngineConfig(
        trader_id="BACKTESTER-001",
        logging=LoggingConfig(log_level="ERROR"),
    )
    engine = BacktestEngine(config=config)

    BYBIT = Venue("BYBIT")
    engine.add_venue(
        venue=BYBIT,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=None,
        starting_balances=[Money(200_000, XRP), Money(100_000, USDT)],
        book_type=BookType.L2_MBP,
    )
    engine.add_instrument(XRPUSDT_BYBIT)
    engine.add_data(deltas)

    sampler = TopBookSampler(
        TopBookSamplerConfig(instrument_id=XRPUSDT_BYBIT.id, sample_every_secs=1),
    )
    engine.add_actor(sampler)

    strategy = OrderBookImbalance(
        OrderBookImbalanceConfig(
            instrument_id=XRPUSDT_BYBIT.id,
            book_type="L2_MBP",
            max_trade_size=Decimal("1.000"),
            min_seconds_between_triggers=1.0,
        ),
    )
    engine.add_strategy(strategy)
    engine.run()

    samples_df = pd.DataFrame(sampler.samples)
    fills = engine.trader.generate_fills_report()

    if not samples_df.empty:
        median_mid = (samples_df["bid"].median() + samples_df["ask"].median()) / 2.0
        ok = (samples_df["bid"] - median_mid).abs() < median_mid * 0.05
        samples_df = samples_df.loc[ok].reset_index(drop=True)
        if len(samples_df) > 1:
            diffs = samples_df["ts"].diff().dt.total_seconds().fillna(0)
            big_gap = diffs[diffs > 300]
            if not big_gap.empty:
                cutoff_idx = big_gap.index[0]
                samples_df = samples_df.iloc[:cutoff_idx].reset_index(drop=True)
    return samples_df, fills


def fills_to_records(fills: pd.DataFrame) -> list[dict]:
    if fills.empty:
        return []
    df = fills.copy()
    df["ts_event"] = pd.to_datetime(df["ts_event"], utc=True)
    return [
        {
            "ts": row["ts_event"],
            "side": row["order_side"],
            "qty": float(row["last_qty"]),
            "price": float(row["last_px"]),
        }
        for _, row in df.iterrows()
    ]


def walk_fills_netting(fills: list[dict]) -> tuple[list[dict], list[dict]]:
    entries: list[dict] = []
    closes: list[dict] = []
    pos = 0.0
    open_side = 0
    EPS = 1e-9

    for f in fills:
        delta = f["qty"] if f["side"] == "BUY" else -f["qty"]
        new_pos = pos + delta
        if abs(pos) < EPS and abs(new_pos) >= EPS:
            open_side = 1 if new_pos > 0 else -1
            entries.append({**f, "side_sign": open_side})
        elif abs(new_pos) < EPS and abs(pos) >= EPS:
            closes.append({**f, "side_sign": -open_side})
            open_side = 0
        pos = new_pos
    return entries, closes


def panel_a_top_book(samples: pd.DataFrame, fills: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if samples.empty:
        apply_layout(fig, "No samples", height=520)
        return fig
    if fills.empty:
        zoom_lo = samples["ts"].min()
        zoom_hi = samples["ts"].min() + pd.Timedelta(minutes=10)
    else:
        ts_e = pd.to_datetime(fills["ts_event"], utc=True).reset_index(drop=True)
        first_burst = ts_e.iloc[: max(1, len(ts_e) // 4)]
        zoom_lo = first_burst.min() - pd.Timedelta(seconds=60)
        zoom_hi = first_burst.max() + pd.Timedelta(seconds=60)
    zoom = samples[(samples["ts"] >= zoom_lo) & (samples["ts"] <= zoom_hi)]

    fill_records = fills_to_records(fills)
    entries, closes = walk_fills_netting(fill_records)
    e_in = [e for e in entries if zoom_lo <= e["ts"] <= zoom_hi]
    c_in = [c for c in closes if zoom_lo <= c["ts"] <= zoom_hi]

    fig.add_trace(
        go.Scatter(
            x=zoom["ts"],
            y=(zoom["bid"] + zoom["ask"]) / 2,
            mode="lines",
            name="Mid",
            line={"color": PRIMARY, "width": 1.4},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=zoom["ts"],
            y=zoom["bid"],
            mode="lines",
            name="Best bid",
            line={"color": POSITIVE, "width": 0.8, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=zoom["ts"],
            y=zoom["ask"],
            mode="lines",
            name="Best ask",
            line={"color": NEGATIVE, "width": 0.8, "dash": "dot"},
        ),
    )
    longs = [r for r in e_in if r["side_sign"] == 1]
    shorts = [r for r in e_in if r["side_sign"] == -1]

    if longs:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in longs],
                y=[r["price"] for r in longs],
                mode="markers",
                name="Long entry",
                marker={
                    "symbol": "triangle-up",
                    "size": 10,
                    "color": POSITIVE,
                    "line": {"color": "white", "width": 1},
                },
            ),
        )
    if shorts:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in shorts],
                y=[r["price"] for r in shorts],
                mode="markers",
                name="Short entry",
                marker={
                    "symbol": "triangle-down",
                    "size": 10,
                    "color": NEGATIVE,
                    "line": {"color": "white", "width": 1},
                },
            ),
        )
    if c_in:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in c_in],
                y=[r["price"] for r in c_in],
                mode="markers",
                name="Close",
                marker={"symbol": "x", "size": 9, "color": "#eeeeee"},
            ),
        )

    title = (
        f"XRPUSDT top of book {zoom_lo.strftime('%Y-%m-%d %H:%M:%S')} to "
        f"{zoom_hi.strftime('%H:%M:%S')} UTC with FOK fills"
    )
    apply_layout(fig, title, height=520)
    fig.update_yaxes(title_text="USDT")
    return fig


def panel_b_imbalance_dist(samples: pd.DataFrame, threshold: float = 0.20) -> go.Figure:
    fig = go.Figure()
    if samples.empty:
        apply_layout(fig, "Imbalance ratio distribution (empty)", height=420)
        return fig
    df = samples.copy()
    df["smaller"] = np.minimum(df["bid_size"], df["ask_size"])
    df["larger"] = np.maximum(df["bid_size"], df["ask_size"])
    df["ratio"] = df["smaller"] / df["larger"].replace(0, np.nan)
    df = df.dropna(subset=["ratio"])
    fig.add_trace(
        go.Histogram(
            x=df["ratio"],
            nbinsx=40,
            marker={"color": PRIMARY, "line": {"color": COLORS["background"], "width": 0.5}},
            showlegend=False,
        ),
    )
    fig.add_vline(
        x=threshold,
        line={"color": POSITIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"trigger ratio {threshold}",
        annotation_position="top right",
        annotation={"font": {"size": 12, "color": POSITIVE}},
    )
    apply_layout(
        fig,
        "Top-of-book imbalance ratio (smaller / larger) distribution across samples",
        height=420,
    )
    fig.update_xaxes(title_text="ratio")
    fig.update_yaxes(title_text="count")
    return fig


def panel_c_size_landscape(samples: pd.DataFrame) -> go.Figure:
    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.07,
        subplot_titles=("Mid (USDT)", "Top-of-book size (XRP)"),
        row_heights=[0.55, 0.45],
    )

    if samples.empty:
        apply_layout(fig, "No samples", height=540)
        return fig
    df = samples.copy()
    df["mid"] = (df["bid"] + df["ask"]) / 2.0
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["mid"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.0},
            showlegend=False,
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["bid_size"],
            mode="lines",
            name="Best bid size",
            line={"color": POSITIVE, "width": 0.8},
        ),
        row=2,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["ask_size"],
            mode="lines",
            name="Best ask size",
            line={"color": NEGATIVE, "width": 0.8},
        ),
        row=2,
        col=1,
    )
    apply_layout(fig, "Top-of-book mid and per-side size across the run", height=600)
    fig.update_yaxes(title_text="USDT", row=1, col=1)
    fig.update_yaxes(title_text="XRP", row=2, col=1)
    return fig


def panel_d_position(fills: pd.DataFrame, samples: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if fills.empty:
        apply_layout(fig, "Net position trajectory (no fills)", height=420)
        return fig
    df = fills.copy()
    df["ts_event"] = pd.to_datetime(df["ts_event"], utc=True)
    df = df.sort_values("ts_event").reset_index(drop=True)
    if not samples.empty:
        df = df[df["ts_event"] <= samples["ts"].max()]
    if df.empty:
        apply_layout(fig, "Net position trajectory (no fills in active window)", height=420)
        return fig
    df["delta"] = df["last_qty"].astype(float) * df["order_side"].map({"BUY": 1.0, "SELL": -1.0})
    df["net_position"] = df["delta"].cumsum()
    fig.add_trace(
        go.Scatter(
            x=df["ts_event"],
            y=df["net_position"],
            mode="lines+markers",
            line={"color": PRIMARY, "width": 1.4},
            marker={
                "size": 6,
                "color": [POSITIVE if d > 0 else NEGATIVE for d in df["delta"]],
                "line": {"color": COLORS["background"], "width": 0.5},
            },
            showlegend=False,
        ),
    )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1})
    apply_layout(fig, "Net XRP position across the FOK fill sequence", height=420)
    fig.update_xaxes(title_text="fill time")
    fig.update_yaxes(title_text="XRP (signed)")
    return fig


def main() -> None:
    samples, fills = run_backtest()
    print(f"samples={len(samples)} fill_rows={len(fills)}")
    panels = {
        "panel_a_top_book.png": panel_a_top_book(samples, fills),
        "panel_b_imbalance_dist.png": panel_b_imbalance_dist(samples),
        "panel_c_size_landscape.png": panel_c_size_landscape(samples),
        "panel_d_position.png": panel_d_position(fills, samples),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
