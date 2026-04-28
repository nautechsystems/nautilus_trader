"""
Render the BitMEX grid market maker tutorial panels from a backtest run.

Usage:

    # Download the free Tardis sample once:
    curl -L -o XBTUSD.csv.gz \
        https://datasets.tardis.dev/v1/bitmex/quotes/2024/01/01/XBTUSD.csv.gz
    curl -L -o XBTUSD-trades.csv.gz \
        https://datasets.tardis.dev/v1/bitmex/trades/2024/01/01/XBTUSD.csv.gz

    uv sync --extra visualization
    XBTUSD_QUOTES=XBTUSD.csv.gz XBTUSD_TRADES=XBTUSD-trades.csv.gz \
        python3 docs/tutorials/assets/grid_market_maker_bitmex/render_panels.py

Replays Tardis 2024-01-01 quotes (and trades when available) through the
shipped ``GridMarketMaker`` strategy with a tighter grid than the live
example uses, so the panels show maker fills on a calm holiday session.
Writes four PNGs to the same directory using the ``nautilus_dark`` theme.

"""

from __future__ import annotations

import os
from decimal import Decimal
from pathlib import Path

import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMaker
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMakerConfig
from nautilus_trader.model.currencies import BTC
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
QUOTES_PATH = Path(os.environ.get("XBTUSD_QUOTES", "XBTUSD.csv.gz"))
TRADES_PATH = Path(os.environ.get("XBTUSD_TRADES", "XBTUSD-trades.csv.gz"))

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

GRID_STEP_BPS = 20
REQUOTE_BPS = 20
NUM_LEVELS = 2
NROWS_QUOTES = 200_000
NROWS_TRADES = 30_000


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
    instrument_id = InstrumentId.from_str("XBTUSD.BITMEX")
    loader = TardisCSVDataLoader(instrument_id=instrument_id)
    quotes = loader.load_quotes(QUOTES_PATH)[:NROWS_QUOTES]
    if TRADES_PATH.exists():
        trades = loader.load_trades(TRADES_PATH)[:NROWS_TRADES]
    else:
        trades = []
    print(f"quotes={len(quotes)} trades={len(trades)}")

    XBTUSD = PerpetualContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol("XBTUSD"),
        underlying="XBT",
        asset_class=AssetClass.CRYPTOCURRENCY,
        base_currency=BTC,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=True,
        price_precision=1,
        size_precision=0,
        price_increment=Price.from_str("0.5"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal("-0.00025"),
        taker_fee=Decimal("0.00075"),
        ts_event=0,
        ts_init=0,
    )

    engine = BacktestEngine(
        BacktestEngineConfig(
            trader_id=TraderId("BACKTESTER-001"),
            logging=LoggingConfig(log_level="ERROR"),
        ),
    )
    BITMEX = Venue("BITMEX")
    engine.add_venue(
        venue=BITMEX,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=BTC,
        starting_balances=[Money(1, BTC)],
    )
    engine.add_instrument(XBTUSD)
    engine.add_data(quotes + trades)

    sampler = QuoteSampler(QuoteSamplerConfig(instrument_id=instrument_id))
    engine.add_actor(sampler)

    strategy = GridMarketMaker(
        GridMarketMakerConfig(
            instrument_id=instrument_id,
            max_position=Quantity.from_int(300),
            trade_size=Quantity.from_int(100),
            num_levels=NUM_LEVELS,
            grid_step_bps=GRID_STEP_BPS,
            skew_factor=0.5,
            requote_threshold_bps=REQUOTE_BPS,
        ),
    )
    engine.add_strategy(strategy)
    engine.run()

    samples = pd.DataFrame(sampler.samples)
    fills = engine.trader.generate_fills_report()
    return samples, fills


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


def panel_a_grid_overlay(samples: pd.DataFrame, fills: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if samples.empty:
        apply_layout(fig, "No samples", height=520)
        return fig

    fill_records = fills_to_records(fills)
    if fill_records:
        burst_lo = fill_records[0]["ts"] - pd.Timedelta(minutes=5)
        burst_hi = fill_records[-1]["ts"] + pd.Timedelta(minutes=5)
    else:
        burst_lo = samples["ts"].min()
        burst_hi = samples["ts"].max()
    z = samples[(samples["ts"] >= burst_lo) & (samples["ts"] <= burst_hi)].reset_index(drop=True)
    z["mid"] = (z["bid"] + z["ask"]) / 2.0

    pct = GRID_STEP_BPS / 10_000.0
    for level in range(1, NUM_LEVELS + 1):
        buy_band = z["mid"] * (1.0 - pct) ** level
        sell_band = z["mid"] * (1.0 + pct) ** level
        fig.add_trace(
            go.Scatter(
                x=z["ts"],
                y=buy_band,
                mode="lines",
                name=f"Buy L{level}",
                line={"color": POSITIVE, "width": 0.8, "dash": "dot"},
                opacity=0.7,
            ),
        )
        fig.add_trace(
            go.Scatter(
                x=z["ts"],
                y=sell_band,
                mode="lines",
                name=f"Sell L{level}",
                line={"color": NEGATIVE, "width": 0.8, "dash": "dot"},
                opacity=0.7,
            ),
        )

    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["mid"],
            mode="lines",
            name="Mid",
            line={"color": PRIMARY, "width": 1.4},
        ),
    )

    f_in = [r for r in fill_records if burst_lo <= r["ts"] <= burst_hi]
    longs = [r for r in f_in if r["side"] == "BUY"]
    shorts = [r for r in f_in if r["side"] == "SELL"]

    if longs:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in longs],
                y=[r["price"] for r in longs],
                mode="markers",
                name="Buy fill",
                marker={
                    "symbol": "triangle-up",
                    "size": 11,
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
                name="Sell fill",
                marker={
                    "symbol": "triangle-down",
                    "size": 11,
                    "color": NEGATIVE,
                    "line": {"color": "white", "width": 1},
                },
            ),
        )

    title = (
        f"XBTUSD mid with theoretical grid bands ({GRID_STEP_BPS} bps step, "
        f"{NUM_LEVELS} levels) and maker fills"
    )

    if not fill_records:
        title = "XBTUSD mid with theoretical grid bands"
    apply_layout(fig, title, height=560)
    fig.update_yaxes(title_text="USD")
    return fig


def panel_b_requote_rate(samples: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if samples.empty:
        apply_layout(fig, "Per-bucket mid moves (no samples)", height=420)
        return fig
    df = samples.copy().sort_values("ts").reset_index(drop=True)
    df["mid"] = (df["bid"] + df["ask"]) / 2.0
    df["bps"] = (df["mid"].pct_change().abs() * 1e4).fillna(0.0)
    df["bucket"] = df["ts"].dt.floor("5min")
    bucketed = (
        df.groupby("bucket")
        .agg(samples=("ts", "count"), max_bps=("bps", "max"), mean_bps=("bps", "mean"))
        .reset_index()
    )
    fig.add_trace(
        go.Bar(
            x=bucketed["bucket"],
            y=bucketed["max_bps"],
            marker={"color": PRIMARY, "line": {"color": COLORS["background"], "width": 0.4}},
            name="max bps move",
        ),
    )
    fig.add_hline(
        y=REQUOTE_BPS,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"requote threshold {REQUOTE_BPS} bps",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
    )
    apply_layout(
        fig,
        "Maximum mid-price step per 5-minute bucket against requote threshold",
        height=420,
    )
    fig.update_xaxes(title_text="bucket start")
    fig.update_yaxes(title_text="bps")
    return fig


def panel_c_position(fills: pd.DataFrame, samples: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if fills.empty:
        apply_layout(fig, "Net position trajectory (no fills)", height=420)
        return fig
    df = fills.copy()
    df["ts_event"] = pd.to_datetime(df["ts_event"], utc=True)
    df = df.sort_values("ts_event").reset_index(drop=True)
    if not samples.empty:
        df = df[df["ts_event"] <= samples["ts"].max()]
    df["delta"] = df["last_qty"].astype(float) * df["order_side"].map({"BUY": 1.0, "SELL": -1.0})
    df["net_position"] = df["delta"].cumsum()
    fig.add_trace(
        go.Scatter(
            x=df["ts_event"],
            y=df["net_position"],
            mode="lines+markers",
            line={"color": PRIMARY, "width": 1.4},
            marker={
                "size": 7,
                "color": [POSITIVE if d > 0 else NEGATIVE for d in df["delta"]],
                "line": {"color": COLORS["background"], "width": 0.5},
            },
            showlegend=False,
        ),
    )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1})
    apply_layout(fig, "Net XBTUSD contract position across the maker fill sequence", height=420)
    fig.update_xaxes(title_text="fill time")
    fig.update_yaxes(title_text="contracts (signed)")
    return fig


def panel_d_deadman_timeline() -> go.Figure:
    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.10,
        row_heights=[0.6, 0.4],
        subplot_titles=(
            "Deadman's switch timer (timeout=60s, refresh=15s)",
            "Connectivity",
        ),
    )

    refresh_times = list(range(0, 51, 15))
    server_remaining_xs: list[float] = []
    server_remaining_ys: list[float] = []
    last = -15

    for t in refresh_times:
        # Timer drains linearly between refreshes.
        if last >= 0:
            server_remaining_xs.extend([last, t])
            server_remaining_ys.extend([60.0, max(0.0, 60.0 - (t - last))])
        last = t
    # Connectivity fails at t=50, no more refreshes; timer drains to 0 at t=105.
    server_remaining_xs.extend([50, 105])
    server_remaining_ys.extend([60.0 - (50 - 45), 0.0])

    fig.add_trace(
        go.Scatter(
            x=server_remaining_xs,
            y=server_remaining_ys,
            mode="lines",
            line={"color": PRIMARY, "width": 2.0},
            name="server timer (s remaining)",
            showlegend=False,
        ),
        row=1,
        col=1,
    )

    for t in refresh_times:
        fig.add_vline(x=t, line={"color": POSITIVE, "dash": "dot", "width": 1}, row=1, col=1)
    fig.add_vline(
        x=105,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.5},
        row=1,
        col=1,
        annotation_text="cancel-all fires at t=105s",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
    )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1}, row=1, col=1)

    # Connectivity row: 1.0 = up, 0.0 = down. Drops at t=50.
    fig.add_trace(
        go.Scatter(
            x=[0, 50, 50, 120],
            y=[1, 1, 0, 0],
            mode="lines",
            line={"color": POSITIVE, "width": 2.0},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    fig.add_vline(
        x=50,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.5},
        row=2,
        col=1,
        annotation_text="connection lost",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
    )

    apply_layout(
        fig,
        "Deadman's switch under a connectivity loss at t=50s",
        height=520,
    )
    fig.update_xaxes(title_text="seconds since strategy start", row=2, col=1, range=[0, 120])
    fig.update_yaxes(title_text="seconds remaining", range=[0, 65], row=1, col=1)
    fig.update_yaxes(
        title_text="link",
        tickvals=[0, 1],
        ticktext=["down", "up"],
        range=[-0.2, 1.2],
        row=2,
        col=1,
    )
    return fig


def main() -> None:
    samples, fills = run_backtest()
    print(f"samples={len(samples)} fills={len(fills)}")
    panels = {
        "panel_a_grid_overlay.png": panel_a_grid_overlay(samples, fills),
        "panel_b_requote_rate.png": panel_b_requote_rate(samples),
        "panel_c_position.png": panel_c_position(fills, samples),
        "panel_d_deadman_timeline.png": panel_d_deadman_timeline(),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
