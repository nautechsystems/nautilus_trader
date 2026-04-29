"""
Render the AX EURUSD-PERP mean reversion tutorial panels from a backtest run.

Usage:

    uv sync --extra visualization
    TRUEFX_CSV=tests/test_data/local/truefx/EURUSD-2025-12.csv \
        python3 docs/tutorials/assets/fx_mean_reversion_ax/render_panels.py

Replays TrueFX EUR/USD ticks through the shipped ``BBMeanReversion`` strategy,
then writes four PNG panels using the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

import os
from decimal import Decimal
from pathlib import Path

import numpy as np
import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversion
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversionConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarType
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
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler


OUT = Path(__file__).resolve().parent
TRUEFX_CSV = Path(
    os.environ.get("TRUEFX_CSV", "tests/test_data/local/truefx/EURUSD-2025-12.csv"),
)

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

BB_PERIOD = 20
BB_STD = 2.0
RSI_PERIOD = 14
RSI_BUY = 0.30
RSI_SELL = 0.70

ZOOM_HOURS = 12


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
    instrument_id = InstrumentId.from_str("EURUSD-PERP.AX")
    EURUSD_PERP = PerpetualContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol("EURUSD-PERP"),
        underlying="EUR",
        asset_class=AssetClass.FX,
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=5,
        size_precision=0,
        price_increment=Price.from_str("0.00001"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1000),
        lot_size=Quantity.from_int(1),
        margin_init=Decimal("0.05"),
        margin_maint=Decimal("0.025"),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0005"),
        ts_event=0,
        ts_init=0,
    )

    df = pd.read_csv(
        TRUEFX_CSV,
        header=None,
        names=["pair", "timestamp", "bid", "ask"],
    )
    df["timestamp"] = pd.to_datetime(df["timestamp"], format="%Y%m%d %H:%M:%S.%f")
    df = df.set_index("timestamp")[["bid", "ask"]]

    wrangler = QuoteTickDataWrangler(instrument=EURUSD_PERP)
    ticks = wrangler.process(df)

    engine = BacktestEngine(
        BacktestEngineConfig(
            trader_id=TraderId("BACKTESTER-001"),
            logging=LoggingConfig(log_level="ERROR"),
            cache=CacheConfig(bar_capacity=200_000, tick_capacity=10_000),
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
    engine.add_instrument(EURUSD_PERP)
    engine.add_data(ticks)

    bar_type = BarType.from_str("EURUSD-PERP.AX-1-MINUTE-MID-INTERNAL")
    strategy = BBMeanReversion(
        BBMeanReversionConfig(
            instrument_id=instrument_id,
            bar_type=bar_type,
            trade_size=Decimal(1),
            bb_period=BB_PERIOD,
            bb_std=BB_STD,
            rsi_period=RSI_PERIOD,
            rsi_buy_threshold=RSI_BUY,
            rsi_sell_threshold=RSI_SELL,
        ),
    )
    engine.add_strategy(strategy)
    engine.run()

    bars = engine.cache.bars(bar_type)
    fills = engine.trader.generate_fills_report()
    positions = engine.trader.generate_positions_report()

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

    return bars_df, fills, positions


def add_indicators(bars: pd.DataFrame) -> pd.DataFrame:
    df = bars.copy()
    sma = df["close"].rolling(BB_PERIOD).mean()
    std = df["close"].rolling(BB_PERIOD).std(ddof=0)
    df["bb_mid"] = sma
    df["bb_upper"] = sma + BB_STD * std
    df["bb_lower"] = sma - BB_STD * std

    delta = df["close"].diff()
    gain = delta.clip(lower=0.0)
    loss = (-delta).clip(lower=0.0)
    avg_gain = gain.ewm(alpha=1.0 / RSI_PERIOD, adjust=False).mean()
    avg_loss = loss.ewm(alpha=1.0 / RSI_PERIOD, adjust=False).mean()
    rs = avg_gain / avg_loss.replace(0.0, np.nan)
    df["rsi"] = 1.0 - 1.0 / (1.0 + rs)
    df["rsi"] = df["rsi"].fillna(0.5)
    return df


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


def walk_fills_netting(fills: list[dict]) -> tuple[list[dict], list[dict], list[dict]]:
    entries: list[dict] = []
    closes: list[dict] = []
    cycles: list[dict] = []
    pos = 0.0
    open_side = 0
    open_ts = None
    open_price = 0.0
    EPS = 1e-9

    for f in fills:
        delta = f["qty"] if f["side"] == "BUY" else -f["qty"]
        new_pos = pos + delta
        if abs(pos) < EPS and abs(new_pos) >= EPS:
            open_side = 1 if new_pos > 0 else -1
            open_ts = f["ts"]
            open_price = f["price"]
            entries.append({**f, "side_sign": open_side})
        elif abs(new_pos) < EPS and abs(pos) >= EPS:
            closes.append({**f, "side_sign": -open_side})
            cycles.append(
                {
                    "open_ts": open_ts,
                    "close_ts": f["ts"],
                    "side": open_side,
                    "open_price": open_price,
                    "close_price": f["price"],
                    "qty": f["qty"],
                },
            )
            open_side = 0
            open_ts = None
        pos = new_pos
    return entries, closes, cycles


def panel_a_overview(bars: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=bars["close"],
            mode="lines",
            name="Close (mid)",
            line={"color": NEUTRAL, "width": 1.0},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=bars["bb_mid"],
            mode="lines",
            name=f"BB middle ({BB_PERIOD})",
            line={"color": PRIMARY, "width": 1.4},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=bars["bb_upper"],
            mode="lines",
            name=f"BB upper +{BB_STD}sd",
            line={"color": NEGATIVE, "width": 0.9, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=bars["ts"],
            y=bars["bb_lower"],
            mode="lines",
            name=f"BB lower -{BB_STD}sd",
            line={"color": POSITIVE, "width": 0.9, "dash": "dot"},
        ),
    )
    apply_layout(
        fig,
        f"EUR/USD 1-minute mid bars (Dec 2025) with BB({BB_PERIOD},{BB_STD}sd) envelope",
        height=460,
    )
    fig.update_yaxes(title_text="USD")
    return fig


def panel_b_zoom(bars: pd.DataFrame, entries, closes) -> go.Figure:
    if bars.empty or "rsi" not in bars.columns:
        return go.Figure()

    pivot = bars.iloc[len(bars) // 2]["ts"]
    lo = pivot - pd.Timedelta(hours=ZOOM_HOURS // 2)
    hi = pivot + pd.Timedelta(hours=ZOOM_HOURS // 2)
    sel = (bars["ts"] >= lo) & (bars["ts"] <= hi)
    z = bars.loc[sel].reset_index(drop=True)

    e = [r for r in entries if lo <= r["ts"] <= hi]
    c = [r for r in closes if lo <= r["ts"] <= hi]

    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.06,
        row_heights=[0.7, 0.3],
        subplot_titles=("Mid + BB envelope with entries and exits", "RSI"),
    )

    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["close"],
            mode="lines",
            name="Close",
            line={"color": NEUTRAL, "width": 1.0},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["bb_mid"],
            mode="lines",
            name="BB middle",
            line={"color": PRIMARY, "width": 1.4},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["bb_upper"],
            mode="lines",
            name="BB upper",
            line={"color": NEGATIVE, "width": 0.9, "dash": "dot"},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["bb_lower"],
            mode="lines",
            name="BB lower",
            line={"color": POSITIVE, "width": 0.9, "dash": "dot"},
        ),
        row=1,
        col=1,
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
                    "size": 10,
                    "color": POSITIVE,
                    "line": {"color": "white", "width": 1},
                },
            ),
            row=1,
            col=1,
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
            row=1,
            col=1,
        )
    if c:
        fig.add_trace(
            go.Scatter(
                x=[r["ts"] for r in c],
                y=[r["price"] for r in c],
                mode="markers",
                name="Close",
                marker={"symbol": "x", "size": 9, "color": "#eeeeee"},
            ),
            row=1,
            col=1,
        )

    fig.add_trace(
        go.Scatter(
            x=z["ts"],
            y=z["rsi"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.0},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    fig.add_hline(
        y=RSI_BUY,
        line={"color": POSITIVE, "dash": "dash", "width": 1},
        annotation_text=f"buy {RSI_BUY}",
        annotation_position="bottom right",
        annotation={"font": {"size": 11, "color": POSITIVE}},
        row=2,
        col=1,
    )
    fig.add_hline(
        y=RSI_SELL,
        line={"color": NEGATIVE, "dash": "dash", "width": 1},
        annotation_text=f"sell {RSI_SELL}",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
        row=2,
        col=1,
    )
    fig.add_hline(y=0.5, line={"color": GRID, "width": 1}, row=2, col=1)

    apply_layout(
        fig,
        f"Zoom {lo.strftime('%Y-%m-%d %H:%M')} to {hi.strftime('%H:%M')} UTC: "
        f"BB envelope, RSI({RSI_PERIOD}), entries and exits",
        height=720,
    )
    fig.update_layout(legend={"y": 1.06})
    fig.update_yaxes(title_text="USD", row=1, col=1)
    fig.update_yaxes(title_text="RSI", range=[0, 1], row=2, col=1)
    return fig


def panel_c_decision_scatter(bars: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if "rsi" not in bars.columns or bars["rsi"].isna().all():
        apply_layout(fig, "Decision space: no indicator samples", height=520)
        return fig
    df = bars.dropna(subset=["bb_lower", "bb_upper", "rsi"])
    band_width = (df["bb_upper"] - df["bb_lower"]).replace(0, np.nan)
    df = df.assign(z=(df["close"] - df["bb_mid"]) / (band_width / 2.0))
    df = df.dropna(subset=["z"])

    fig.add_shape(
        type="rect",
        x0=-3.0,
        x1=-1.0,
        y0=0.0,
        y1=RSI_BUY,
        fillcolor=POSITIVE,
        opacity=0.15,
        line_width=0,
        layer="below",
    )
    fig.add_shape(
        type="rect",
        x0=1.0,
        x1=3.0,
        y0=RSI_SELL,
        y1=1.0,
        fillcolor=NEGATIVE,
        opacity=0.15,
        line_width=0,
        layer="below",
    )

    eligible_long = (df["z"] <= -1.0) & (df["rsi"] < RSI_BUY)
    eligible_short = (df["z"] >= 1.0) & (df["rsi"] > RSI_SELL)
    other = ~(eligible_long | eligible_short)

    for mask, color, name, size in (
        (other, NEUTRAL, "neutral bars", 4),
        (eligible_long, POSITIVE, "long-eligible bars", 7),
        (eligible_short, NEGATIVE, "short-eligible bars", 7),
    ):
        sel = df[mask]
        if not sel.empty:
            fig.add_trace(
                go.Scatter(
                    x=sel["z"],
                    y=sel["rsi"],
                    mode="markers",
                    name=name,
                    marker={"size": size, "color": color, "line": {"width": 0}},
                ),
            )

    fig.add_vline(x=-1.0, line={"color": POSITIVE, "dash": "dash", "width": 1})
    fig.add_vline(x=1.0, line={"color": NEGATIVE, "dash": "dash", "width": 1})
    fig.add_hline(y=RSI_BUY, line={"color": POSITIVE, "dash": "dash", "width": 1})
    fig.add_hline(y=RSI_SELL, line={"color": NEGATIVE, "dash": "dash", "width": 1})

    apply_layout(
        fig,
        "Decision space per bar: BB z-score vs RSI (shaded = entry-eligible)",
        height=520,
    )
    fig.update_xaxes(title_text="z = (close - mid) / sd", range=[-3.0, 3.0])
    fig.update_yaxes(title_text="RSI", range=[0.0, 1.0])
    return fig


def panel_d_pnl(positions: pd.DataFrame, cycles: list[dict]) -> go.Figure:
    fig = go.Figure()
    if not cycles:
        apply_layout(fig, "Cumulative realised pnl per closed cycle (no cycles)", height=420)
        return fig
    if "realized_pnl" in positions.columns:
        df = positions.copy()
        df = df[df["ts_closed"].notna()]
        df["pnl_usd"] = (
            df["realized_pnl"].astype(str).str.replace(" USD", "", regex=False).astype(float)
        )
        df = df.sort_values("ts_closed").reset_index(drop=True)
        df["cum_pnl"] = df["pnl_usd"].cumsum()
        ts_x = df["ts_closed"]
        cum_y = df["cum_pnl"]
        per_y = df["pnl_usd"]
    else:
        df = pd.DataFrame(cycles)
        df["pnl_usd"] = (df["close_price"] - df["open_price"]) * df["side"] * df["qty"]
        df["cum_pnl"] = df["pnl_usd"].cumsum()
        ts_x = df["close_ts"]
        cum_y = df["cum_pnl"]
        per_y = df["pnl_usd"]

    colors = [POSITIVE if p >= 0 else NEGATIVE for p in per_y]
    fig.add_trace(
        go.Scatter(
            x=ts_x,
            y=cum_y,
            mode="lines",
            line={"color": PRIMARY, "width": 1.6},
            showlegend=False,
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=ts_x,
            y=cum_y,
            mode="markers",
            marker={
                "size": 5,
                "color": colors,
                "line": {"color": COLORS["background"], "width": 0.4},
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
    bars, fills, positions = run_backtest()
    bars_ind = add_indicators(bars)
    fill_records = fills_to_records(fills)
    entries, closes, cycles = walk_fills_netting(fill_records)
    print(
        f"bars={len(bars_ind)} fill_rows={len(fills)} "
        f"entries={len(entries)} closes={len(closes)} cycles={len(cycles)} "
        f"positions={len(positions)}",
    )
    panels = {
        "panel_a_overview.png": panel_a_overview(bars_ind),
        "panel_b_zoom.png": panel_b_zoom(bars_ind, entries, closes),
        "panel_c_decision_scatter.png": panel_c_decision_scatter(bars_ind),
        "panel_d_pnl.png": panel_d_pnl(positions, cycles),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
