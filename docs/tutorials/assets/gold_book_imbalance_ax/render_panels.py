"""
Render the AX XAU-PERP book imbalance tutorial panels from a backtest run.

Usage:

    uv sync --extra visualization
    GC_DBN=tests/test_data/local/Databento/gc_gold_quotes.dbn.zst \
        python3 docs/tutorials/assets/gold_book_imbalance_ax/render_panels.py

Replays a Databento ``GC.v.0`` mbp-1 file through the shipped
``OrderBookImbalance`` strategy with `use_quote_ticks=True`. Quote ticks are
sampled once per second by an actor for the panels, then four PNGs are
written using the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

import os
from decimal import Decimal
from pathlib import Path

import numpy as np
import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


OUT = Path(__file__).resolve().parent
GC_DBN = Path(
    os.environ.get(
        "GC_DBN",
        "tests/test_data/local/Databento/gc_gold_quotes.dbn.zst",
    ),
)

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

TRIGGER_RATIO = 0.10
MIN_TRIGGER_SIZE = 1.0


class QuoteSamplerConfig(ActorConfig, frozen=True):
    instrument_id: InstrumentId
    sample_every_secs: int = 1


class QuoteSampler(Actor):
    def __init__(self, config: QuoteSamplerConfig) -> None:
        super().__init__(config)
        self.samples: list[dict] = []
        self._last_sample_ns = 0
        self._interval_ns = config.sample_every_secs * 1_000_000_000

    def on_start(self) -> None:
        self.subscribe_quote_ticks(self.config.instrument_id)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        ts = tick.ts_event
        if ts - self._last_sample_ns < self._interval_ns:
            return
        self.samples.append(
            {
                "ts": pd.Timestamp(ts, unit="ns", tz="UTC"),
                "bid": float(tick.bid_price),
                "ask": float(tick.ask_price),
                "bid_size": float(tick.bid_size),
                "ask_size": float(tick.ask_size),
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


def run_backtest():
    instrument_id = InstrumentId.from_str("XAU-PERP.AX")
    XAU_PERP = PerpetualContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol("XAU-PERP"),
        underlying="XAU",
        asset_class=AssetClass.COMMODITY,
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        margin_init=Decimal("0.08"),
        margin_maint=Decimal("0.04"),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0005"),
        ts_event=0,
        ts_init=0,
    )

    loader = DatabentoDataLoader()
    quotes = loader.from_dbn_file(path=str(GC_DBN), instrument_id=instrument_id)

    engine = BacktestEngine(
        BacktestEngineConfig(
            trader_id=TraderId("BACKTESTER-001"),
            logging=LoggingConfig(log_level="ERROR"),
            cache=CacheConfig(bar_capacity=10_000, tick_capacity=10_000),
        ),
    )

    AX = Venue("AX")
    engine.add_venue(
        venue=AX,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(100_000, USD)],
    )
    engine.add_instrument(XAU_PERP)
    engine.add_data(quotes)

    sampler = QuoteSampler(QuoteSamplerConfig(instrument_id=instrument_id))
    engine.add_actor(sampler)

    strategy = OrderBookImbalance(
        OrderBookImbalanceConfig(
            instrument_id=instrument_id,
            max_trade_size=Decimal(10),
            trigger_min_size=MIN_TRIGGER_SIZE,
            trigger_imbalance_ratio=TRIGGER_RATIO,
            min_seconds_between_triggers=5.0,
            book_type="L1_MBP",
            use_quote_ticks=True,
        ),
    )
    engine.add_strategy(strategy)
    engine.run()

    samples = pd.DataFrame(sampler.samples)
    fills = engine.trader.generate_fills_report()
    positions = engine.trader.generate_positions_report()
    return samples, fills, positions


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


def walk_fills_netting(fills: list[dict]):
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


def panel_a_top_book(
    samples: pd.DataFrame,
    fills: pd.DataFrame,
    positions: pd.DataFrame,
) -> go.Figure:
    fig = go.Figure()
    if samples.empty:
        apply_layout(fig, "No quote samples", height=520)
        return fig

    fill_records = fills_to_records(fills)
    entries, closes = walk_fills_netting(fill_records)

    if entries:
        cycles_df = pd.DataFrame(
            {
                "open": [pd.Timestamp(e["ts"]) for e in entries[: len(closes)]],
                "close": [pd.Timestamp(c["ts"]) for c in closes],
            },
        )
        cycles_df["dur"] = cycles_df["close"] - cycles_df["open"]
        cycles_df = cycles_df[cycles_df["dur"] < pd.Timedelta(hours=2)]
        if not cycles_df.empty:
            row = cycles_df.iloc[
                (cycles_df["dur"] - pd.Timedelta(minutes=20)).abs().argsort().iloc[0]
            ]
            zoom_lo = row["open"] - pd.Timedelta(minutes=10)
            zoom_hi = row["close"] + pd.Timedelta(minutes=10)
        else:
            zoom_lo = entries[0]["ts"] - pd.Timedelta(minutes=15)
            zoom_hi = entries[0]["ts"] + pd.Timedelta(minutes=45)
    else:
        zoom_lo = samples["ts"].min()
        zoom_hi = samples["ts"].min() + pd.Timedelta(hours=1)

    z = samples[(samples["ts"] >= zoom_lo) & (samples["ts"] <= zoom_hi)]
    e_in = [e for e in entries if zoom_lo <= e["ts"] <= zoom_hi]
    c_in = [c for c in closes if zoom_lo <= c["ts"] <= zoom_hi]
    fills_in = [f for f in fill_records if zoom_lo <= f["ts"] <= zoom_hi]
    incremental = [
        f
        for f in fills_in
        if f["ts"] not in {e["ts"] for e in e_in} and f["ts"] not in {c["ts"] for c in c_in}
    ]

    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=(z["bid"] + z["ask"]) / 2,
            mode="lines",
            name="Mid",
            line={"color": PRIMARY, "width": 1.4},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["bid"],
            mode="lines",
            name="Best bid",
            line={"color": POSITIVE, "width": 0.8, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["ask"],
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
                marker={"symbol": "x", "size": 11, "color": "#eeeeee"},
            ),
        )
    if incremental:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in incremental],
                y=[r["price"] for r in incremental],
                mode="markers",
                name="Increment fill",
                marker={
                    "symbol": "circle-open",
                    "size": 7,
                    "color": NEUTRAL,
                    "line": {"color": NEUTRAL, "width": 1},
                },
            ),
        )

    title = (
        f"GC.v.0 top of book {zoom_lo.strftime('%Y-%m-%d %H:%M')} to "
        f"{zoom_hi.strftime('%H:%M')} UTC with FOK fills"
    )
    apply_layout(fig, title, height=520)
    fig.update_yaxes(title_text="USD")
    return fig


def panel_b_imbalance_dist(samples: pd.DataFrame) -> go.Figure:
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
        x=TRIGGER_RATIO,
        line={"color": POSITIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"trigger {TRIGGER_RATIO}",
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
        vertical_spacing=0.06,
        row_heights=[0.55, 0.45],
        subplot_titles=("Mid (USD)", "Top-of-book size (contracts)"),
    )

    if samples.empty:
        apply_layout(fig, "No samples", height=600)
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
    apply_layout(fig, "GC.v.0 mid and best bid/ask size across the sampled day", height=620)
    fig.update_yaxes(title_text="USD", row=1, col=1)
    fig.update_yaxes(title_text="contracts", row=2, col=1)
    return fig


def panel_d_pnl(positions: pd.DataFrame, fills: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if positions.empty or fills.empty:
        apply_layout(fig, "Cumulative realised pnl per closed position (no positions)", height=420)
        return fig
    df = positions.copy()
    df = df[df["ts_closed"].notna()]
    if df.empty:
        apply_layout(fig, "No closed positions", height=420)
        return fig
    df["pnl_usd"] = (
        df["realized_pnl"].astype(str).str.replace(" USD", "", regex=False).astype(float)
    )
    df = df.sort_values("ts_closed").reset_index(drop=True)
    df["cum_pnl"] = df["pnl_usd"].cumsum()
    colors = [POSITIVE if p >= 0 else NEGATIVE for p in df["pnl_usd"]]
    fig.add_trace(
        go.Scatter(
            x=df["ts_closed"],
            y=df["cum_pnl"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.6},
            showlegend=False,
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=df["ts_closed"],
            y=df["cum_pnl"],
            mode="markers",
            marker={
                "size": 6,
                "color": colors,
                "line": {"color": COLORS["background"], "width": 0.5},
            },
            showlegend=False,
        ),
    )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1})
    apply_layout(fig, "Cumulative realised pnl per closed position (USD)", height=420)
    fig.update_xaxes(title_text="position close time")
    fig.update_yaxes(title_text="USD")
    return fig


def main() -> None:
    samples, fills, positions = run_backtest()
    print(f"samples={len(samples)} fill_rows={len(fills)} positions={len(positions)}")
    panels = {
        "panel_a_top_book.png": panel_a_top_book(samples, fills, positions),
        "panel_b_imbalance_dist.png": panel_b_imbalance_dist(samples),
        "panel_c_size_landscape.png": panel_c_size_landscape(samples),
        "panel_d_pnl.png": panel_d_pnl(positions, fills),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
