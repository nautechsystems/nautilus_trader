"""
Render the FX bars tutorial panels from a backtest run.

Usage:

    uv sync --extra visualization
    python3 docs/tutorials/assets/backtest_fx_bars/render_panels.py

Runs the same EMACross backtest as ``docs/tutorials/backtest_fx_bars.py``
on bundled FXCM USD/JPY 2013-02 1-minute bars, then writes four PNG panels
to the same directory using the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

from decimal import Decimal
from pathlib import Path

import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.model import BarType
from nautilus_trader.model import Money
from nautilus_trader.model import Venue
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


OUT = Path(__file__).resolve().parent

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

FAST_EMA = 10
SLOW_EMA = 20

ZOOM_START = pd.Timestamp("2013-02-12T00:00:00Z")
ZOOM_END = pd.Timestamp("2013-02-15T00:00:00Z")


def apply_layout(fig: go.Figure, title: str, height: int = 500) -> None:
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
    config = BacktestEngineConfig(
        trader_id="BACKTESTER-001",
        logging=LoggingConfig(log_level="ERROR"),
        risk_engine=RiskEngineConfig(bypass=True),
    )
    engine = BacktestEngine(config=config)

    provider = TestDataProvider()
    rollover_config = FXRolloverInterestConfig(provider.read_csv("short-term-interest.csv"))
    rollover = FXRolloverInterestModule(config=rollover_config)

    fill_model = FillModel(
        prob_fill_on_limit=0.2,
        prob_slippage=0.5,
        random_seed=42,
    )

    SIM = Venue("SIM")
    engine.add_venue(
        venue=SIM,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=None,
        starting_balances=[Money(1_000_000, USD), Money(10_000_000, JPY)],
        fill_model=fill_model,
        modules=[rollover],
    )

    USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY", SIM)
    engine.add_instrument(USDJPY_SIM)

    wrangler = QuoteTickDataWrangler(instrument=USDJPY_SIM)
    ticks = wrangler.process_bar_data(
        bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
        ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
    )
    engine.add_data(ticks)

    bar_type = BarType.from_str("USD/JPY.SIM-5-MINUTE-BID-INTERNAL")
    strategy = EMACross(
        EMACrossConfig(
            instrument_id=USDJPY_SIM.id,
            bar_type=bar_type,
            fast_ema_period=FAST_EMA,
            slow_ema_period=SLOW_EMA,
            trade_size=Decimal(1_000_000),
        ),
    )
    engine.add_strategy(strategy)

    engine.run()

    bars = engine.cache.bars(bar_type)
    fills = engine.trader.generate_fills_report()
    bars_df = (
        pd.DataFrame(
            [
                {
                    "ts": pd.Timestamp(b.ts_init, unit="ns", tz="UTC"),
                    "open": float(b.open),
                    "high": float(b.high),
                    "low": float(b.low),
                    "close": float(b.close),
                }
                for b in bars
            ],
        )
        .sort_values("ts")
        .reset_index(drop=True)
    )

    return bars_df, fills


def _ema(series: pd.Series, period: int) -> pd.Series:
    return series.ewm(alpha=2.0 / (period + 1.0), adjust=False).mean()


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


def walk_fills_hedging(fills: list[dict]) -> tuple[list[dict], list[dict], list[dict]]:
    """
    Walk fills for HEDGING-OMS EMACross.

    Each crossover after the first emits two
    fills: the first closes the existing position, the second opens the new one.
    Pair them so we can plot entry markers, close markers, and per-cycle pnl.

    """
    entries: list[dict] = []
    closes: list[dict] = []
    cycles: list[dict] = []

    open_side = 0
    open_ts = None
    open_price = 0.0
    open_qty = 0.0

    for f in fills:
        sign = 1 if f["side"] == "BUY" else -1
        if open_side == 0:
            open_side = sign
            open_ts = f["ts"]
            open_price = f["price"]
            open_qty = f["qty"]
            entries.append({**f, "side_sign": sign})
        elif sign != open_side:
            closes.append({**f, "side_sign": sign})
            cycles.append(
                {
                    "open_ts": open_ts,
                    "close_ts": f["ts"],
                    "side": open_side,
                    "open_price": open_price,
                    "close_price": f["price"],
                    "qty": min(open_qty, f["qty"]),
                },
            )
            open_side = 0
            open_ts = None
        else:
            open_qty += f["qty"]
            entries.append({**f, "side_sign": sign})

    return entries, closes, cycles


def _filter_window(records: list[dict], lo: pd.Timestamp, hi: pd.Timestamp) -> list[dict]:
    return [r for r in records if lo <= r["ts"] <= hi]


def panel_a_price_overview(bars: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=bars["close"],
            mode="lines",
            name="Close (BID)",
            line={"color": NEUTRAL, "width": 1.0},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=_ema(bars["close"], FAST_EMA),
            mode="lines",
            name=f"EMA({FAST_EMA})",
            line={"color": PRIMARY, "width": 1.6},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=_ema(bars["close"], SLOW_EMA),
            mode="lines",
            name=f"EMA({SLOW_EMA})",
            line={"color": POSITIVE, "width": 1.6, "dash": "dot"},
        ),
    )
    apply_layout(
        fig,
        f"USD/JPY 5-minute BID bars across 2013-02 with EMA({FAST_EMA}) and EMA({SLOW_EMA})",
        height=460,
    )
    fig.update_yaxes(title_text="JPY")
    return fig


def panel_b_zoom(
    bars: pd.DataFrame,
    entries: list[dict],
    closes: list[dict],
    cycles: list[dict],
) -> go.Figure:
    sel = (bars["ts"] >= ZOOM_START) & (bars["ts"] <= ZOOM_END)
    z = bars.loc[sel].reset_index(drop=True)
    e = _filter_window(entries, ZOOM_START, ZOOM_END)

    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["close"],
            mode="lines",
            name="Close (BID)",
            line={"color": NEUTRAL, "width": 1.2},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=_ema(bars["close"], FAST_EMA).loc[sel],
            mode="lines",
            name=f"EMA({FAST_EMA})",
            line={"color": PRIMARY, "width": 1.6},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=_ema(bars["close"], SLOW_EMA).loc[sel],
            mode="lines",
            name=f"EMA({SLOW_EMA})",
            line={"color": POSITIVE, "width": 1.6, "dash": "dot"},
        ),
    )

    longs = [r for r in e if r["side_sign"] == 1]
    shorts = [r for r in e if r["side_sign"] == -1]

    if longs:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in longs],
                y=[r["price"] for r in longs],
                mode="markers",
                name="Long entry",
                marker={
                    "symbol": "triangle-up",
                    "size": 9,
                    "color": POSITIVE,
                    "line": {"color": "white", "width": 0.8},
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
                    "size": 9,
                    "color": NEGATIVE,
                    "line": {"color": "white", "width": 0.8},
                },
            ),
        )

    apply_layout(
        fig,
        "USD/JPY 5-minute bars 2013-02-12 to 2013-02-15 UTC with crossover entries",
        height=520,
    )
    fig.update_yaxes(title_text="JPY")
    return fig


def panel_c_pnl_curve(cycles: list[dict]) -> go.Figure:
    fig = go.Figure()
    if not cycles:
        apply_layout(fig, "Cumulative realised pnl per closed cycle (no fills)", height=400)
        return fig
    df = pd.DataFrame(cycles)
    df["pnl_jpy"] = (df["close_price"] - df["open_price"]) * df["side"] * df["qty"]
    df["cum_pnl_jpy"] = df["pnl_jpy"].cumsum()

    colors = [POSITIVE if p >= 0 else NEGATIVE for p in df["pnl_jpy"]]
    fig.add_trace(
        go.Scatter(
            x=df["close_ts"],
            y=df["cum_pnl_jpy"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.6},
            showlegend=False,
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=df["close_ts"],
            y=df["cum_pnl_jpy"],
            mode="markers",
            marker={
                "size": 7,
                "color": colors,
                "line": {"color": COLORS["background"], "width": 0.5},
            },
            showlegend=False,
        ),
    )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1})
    apply_layout(fig, "Cumulative realised pnl across all closed cycles (JPY)", height=420)
    fig.update_xaxes(title_text="cycle close time")
    fig.update_yaxes(title_text="JPY")
    return fig


def panel_d_distributions(cycles: list[dict]) -> go.Figure:
    fig = make_subplots(
        rows=1,
        cols=2,
        subplot_titles=("Cycle hold time (minutes)", "Per-cycle realised pnl (JPY)"),
    )

    if not cycles:
        apply_layout(fig, "No completed cycles", height=400)
        return fig
    df = pd.DataFrame(cycles)
    df["hold_min"] = (df["close_ts"] - df["open_ts"]).dt.total_seconds() / 60.0
    df["pnl_jpy"] = (df["close_price"] - df["open_price"]) * df["side"] * df["qty"]

    fig.add_trace(
        go.Histogram(
            x=df["hold_min"],
            nbinsx=30,
            marker={"color": PRIMARY, "line": {"color": COLORS["background"], "width": 0.5}},
            showlegend=False,
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Histogram(
            x=df["pnl_jpy"],
            nbinsx=30,
            marker={"color": POSITIVE, "line": {"color": COLORS["background"], "width": 0.5}},
            showlegend=False,
        ),
        row=1,
        col=2,
    )
    fig.add_vline(x=0.0, line={"color": NEUTRAL, "dash": "dash"}, row=1, col=2)
    apply_layout(fig, "Cycle hold-time and pnl distributions across the run", height=420)
    fig.update_xaxes(title_text="minutes", row=1, col=1)
    fig.update_xaxes(title_text="JPY", row=1, col=2)
    fig.update_yaxes(title_text="count", row=1, col=1)
    fig.update_yaxes(title_text="count", row=1, col=2)
    return fig


def main() -> None:
    bars, fills = run_backtest()
    print(f"bars={len(bars)} fill_rows={len(fills)}")
    fill_records = fills_to_records(fills)
    entries, closes, cycles = walk_fills_hedging(fill_records)
    df_cycles = pd.DataFrame(cycles) if cycles else pd.DataFrame()
    if not df_cycles.empty:
        df_cycles["pnl_jpy"] = (
            (df_cycles["close_price"] - df_cycles["open_price"])
            * df_cycles["side"]
            * df_cycles["qty"]
        )
        winners = (df_cycles["pnl_jpy"] > 0).sum()
        print(
            f"entries={len(entries)} closes={len(closes)} cycles={len(cycles)} "
            f"winners={winners} cum_pnl_jpy={df_cycles['pnl_jpy'].sum():+.0f}",
        )
    panels = {
        "panel_a_price_overview.png": panel_a_price_overview(bars),
        "panel_b_zoom.png": panel_b_zoom(bars, entries, closes, cycles),
        "panel_c_pnl_curve.png": panel_c_pnl_curve(cycles),
        "panel_d_distributions.png": panel_d_distributions(cycles),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
