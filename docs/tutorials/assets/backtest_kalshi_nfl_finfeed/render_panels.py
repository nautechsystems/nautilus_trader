"""
Render the Kalshi trade replay panels from the tutorial's cached catalog.

Run the tutorial first to build its catalog. With Plotly, Kaleido, and Chrome installed,
run from the repository root:

    uv run --project python --no-sync \
        python docs/tutorials/assets/backtest_kalshi_nfl_finfeed/render_panels.py

"""

import sys
from pathlib import Path

import plotly.graph_objects as go

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.core import dt_to_unix_nanos
from nautilus_trader.core import unix_nanos_to_iso8601
from nautilus_trader.persistence import ParquetDataCatalog


OUT = Path(__file__).resolve().parent
sys.path.insert(0, str(OUT.parents[3] / "examples" / "backtest"))
sys.path.insert(0, str(OUT.parent))

import finfeed_kalshi_nfl_ema_cross as tutorial
from orderbook_panels import COLORS
from orderbook_panels import apply_layout


def main() -> None:
    """
    Plot the tutorial actor's EMA values at each recorded trade.
    """
    catalog = ParquetDataCatalog(str(tutorial.CATALOG_PATH))
    trades = catalog.query_trades(
        identifiers=[str(tutorial.INSTRUMENT_ID)],
        start=dt_to_unix_nanos(tutorial.START),
        end=dt_to_unix_nanos(tutorial.END),
    )
    actor = tutorial.TradeEmaCross()
    fast = []
    slow = []

    for trade in trades:
        actor.on_trade(trade)
        fast.append(actor.fast.value if actor.fast.initialized else None)
        slow.append(actor.slow.value if actor.slow.initialized else None)

    if not actor.crossovers:
        raise ValueError(f"No crossover found in {tutorial.CATALOG_PATH}")

    first, _ = actor.crossovers[0]
    times = [unix_nanos_to_iso8601(trade.ts_event) for trade in trades]
    quarter = go.Figure()
    quarter.add_scatter(
        x=times,
        y=[float(trade.price) for trade in trades],
        name="Recorded trade",
        mode="markers",
        marker={"size": 6, "color": COLORS["text"], "opacity": 0.75},
    )

    for values, period, color in [
        (fast, tutorial.FAST_PERIOD, COLORS["primary"]),
        (slow, tutorial.SLOW_PERIOD, "#fbbf24"),
    ]:
        quarter.add_scatter(
            x=times,
            y=values,
            name=f"{period}-trade EMA",
            mode="lines",
            line={"shape": "hv", "width": 2, "color": color},
        )
    quarter.update_layout(title_text="Green Bay to win by more than 7.5 points")
    apply_layout(quarter)
    quarter.add_vline(x=times[first], line_dash="dot", line_color=COLORS["muted"])
    quarter.add_annotation(
        x=times[first],
        y=1,
        yref="paper",
        text="First crossover",
        xanchor="right",
        showarrow=False,
    )
    quarter.update_xaxes(
        title_text="First quarter, October 19 (UTC)",
        tickformat="%H:%M",
        range=[tutorial.START, tutorial.END],
    )
    quarter.update_yaxes(title_text="Price per Yes contract (USD)", tickformat=".2f")
    _write_figure(quarter, str(OUT / "panel_a_trade_prices.png"))

    sample = trades[first : first + 3]
    elapsed = [(trade.ts_event - sample[0].ts_event) / 1_000 for trade in sample]
    gaps = [100 * (fast[i] - slow[i]) for i in range(first, first + len(sample))]
    detail = go.Figure()
    detail.add_scatter(
        x=elapsed,
        y=gaps,
        mode="lines+markers+text",
        line={"shape": "hv", "width": 2, "color": COLORS["primary"]},
        marker={"size": 10, "color": COLORS["primary"]},
        text=[f"{gap:+.3f}" for gap in gaps],
        textposition=["top right", "top left", "bottom left"][: len(sample)],
    )
    detail.update_layout(title_text="The first crossover and the next two trades")
    apply_layout(detail)
    detail.add_hline(y=0, line_dash="dot", line_color=COLORS["muted"], layer="below")
    detail.update_xaxes(
        title_text="Microseconds since the first crossover",
        tickvals=elapsed,
        ticktext=[
            f"{us:g}<br>Trade {i} at ${float(trade.price):.2f}"
            for i, (us, trade) in enumerate(zip(elapsed, sample, strict=True), first + 1)
        ],
    )
    detail.update_yaxes(title_text="Fast EMA minus slow EMA (cents per contract)")
    _write_figure(detail, str(OUT / "panel_b_first_crossover.png"))


if __name__ == "__main__":
    main()
