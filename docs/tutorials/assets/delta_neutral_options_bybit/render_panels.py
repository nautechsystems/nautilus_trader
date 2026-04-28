"""
Render the Bybit delta-neutral options tutorial panels.

Usage:

    timeout 30 ./target/release/examples/bybit-delta-neutral > /tmp/bybit_dn.log 2>&1

    uv sync --extra visualization
    DN_LOG=/tmp/bybit_dn.log \
        python3 docs/tutorials/assets/delta_neutral_options_bybit/render_panels.py

The default example has ``enter_strangle: false`` so a clean account
places no orders. The renderer parses the log for the selected call /
put strikes and underlying prices, then draws four illustrative panels
explaining the strategy mechanics: a short-strangle payoff curve, a
delta-drift simulation, the rehedge threshold visualization, and the
hedge order timeline. PNGs use the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

import os
import re
from pathlib import Path

import numpy as np
import plotly.graph_objects as go

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme


OUT = Path(__file__).resolve().parent
LOG_PATH = Path(os.environ.get("DN_LOG", "/tmp/bybit_dn.log"))  # noqa: S108

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

ANSI = re.compile(r"\x1b\[[0-9;]*m")
SELECTED_CALL = re.compile(r"Selected call: ([^\s]+) \(strike=(?P<k>[\-0-9.]+)\)")
SELECTED_PUT = re.compile(r"Selected put: ([^\s]+) \(strike=(?P<k>[\-0-9.]+)\)")
GREEKS_LINE = re.compile(
    r"GREEKS \| (?P<inst>\S+) \| delta=(?P<delta>[\-0-9.]+) "
    r"gamma=[\-0-9.]+ vega=[\-0-9.]+ theta=[\-0-9.]+ rho=[\-0-9.]+ \| "
    r"mark_iv=(?P<iv>[\-0-9.]+) [^|]+ \| underlying=(?P<u>[\-0-9.]+)",
)
PORTFOLIO_DELTA = re.compile(r"portfolio_delta=(?P<d>[\-0-9.]+)")


def parse_log(path: Path) -> dict:
    out = {
        "call_strike": None,
        "put_strike": None,
        "call_inst": None,
        "put_inst": None,
        "underlying": None,
        "deltas": [],
    }

    if not path.exists():
        return out
    for raw in path.read_text().splitlines():
        line = ANSI.sub("", raw)
        m = SELECTED_CALL.search(line)
        if m:
            out["call_strike"] = float(m.group("k"))
            out["call_inst"] = m.group(1)
        m = SELECTED_PUT.search(line)
        if m:
            out["put_strike"] = float(m.group("k"))
            out["put_inst"] = m.group(1)
        m = GREEKS_LINE.search(line)
        if m:
            out["underlying"] = float(m.group("u"))
        m = PORTFOLIO_DELTA.search(line)
        if m:
            out["deltas"].append(float(m.group("d")))
    return out


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


def panel_a_strangle_payoff(
    call_k: float,
    put_k: float,
    underlying: float,
    premium: float = 1500.0,
) -> go.Figure:
    fig = go.Figure()
    span = max(call_k, underlying * 1.1) - min(put_k, underlying * 0.9)
    s = np.linspace(
        min(put_k, underlying * 0.9) - span * 0.1,
        max(call_k, underlying * 1.1) + span * 0.1,
        300,
    )
    payoff = premium - np.maximum(s - call_k, 0) - np.maximum(put_k - s, 0)
    fig.add_trace(
        go.Scatter(
            x=s,
            y=payoff,
            mode="lines",
            name="Strangle pnl at expiry",
            line={"color": PRIMARY, "width": 2.0},
            fill="tozeroy",
            fillcolor="rgba(0, 207, 190, 0.15)",
        ),
    )
    fig.add_hline(y=0, line={"color": NEUTRAL, "dash": "dash", "width": 1})
    fig.add_vline(
        x=put_k,
        line={"color": POSITIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"PUT strike {put_k:.0f}",
        annotation_position="top left",
        annotation={"font": {"size": 11, "color": POSITIVE}},
    )
    fig.add_vline(
        x=call_k,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.2},
        annotation_text=f"CALL strike {call_k:.0f}",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
    )
    fig.add_vline(
        x=underlying,
        line={"color": NEUTRAL, "dash": "dot", "width": 1.0},
        annotation_text=f"current {underlying:.0f}",
        annotation_position="bottom right",
        annotation={"font": {"size": 11, "color": NEUTRAL}},
    )
    apply_layout(
        fig,
        f"Short-strangle payoff at expiry: short {put_k:.0f} put + short {call_k:.0f} call",
        height=460,
    )
    fig.update_xaxes(title_text="underlying at expiry (USDT)")
    fig.update_yaxes(title_text="pnl (USDT, premium net of intrinsic)")
    return fig


def panel_b_delta_drift(call_k: float, put_k: float, underlying: float) -> go.Figure:
    fig = go.Figure()
    moves = np.linspace(-0.05, 0.05, 200)
    spot = underlying * (1.0 + moves)

    def call_delta_bs(s, k):
        # Toy approximation: monotonic delta from -0 to 1 around the strike.
        z = (s - k) / (underlying * 0.05)
        return 0.5 * (1.0 + np.tanh(z))

    def put_delta_bs(s, k):
        z = (k - s) / (underlying * 0.05)
        return -0.5 * (1.0 + np.tanh(z))

    short_call_delta = -call_delta_bs(spot, call_k)
    short_put_delta = -put_delta_bs(spot, put_k)
    portfolio_delta = short_call_delta + short_put_delta

    fig.add_trace(
        go.Scatter(
            x=spot,
            y=short_call_delta,
            mode="lines",
            name="short CALL leg delta",
            line={"color": NEGATIVE, "width": 1.4, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=spot,
            y=short_put_delta,
            mode="lines",
            name="short PUT leg delta",
            line={"color": POSITIVE, "width": 1.4, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=spot,
            y=portfolio_delta,
            mode="lines",
            name="portfolio delta (pre-hedge)",
            line={"color": PRIMARY, "width": 2.0},
        ),
    )
    fig.add_hline(y=0.5, line={"color": POSITIVE, "dash": "dash", "width": 1})
    fig.add_hline(y=-0.5, line={"color": NEGATIVE, "dash": "dash", "width": 1})
    fig.add_vline(
        x=underlying,
        line={"color": NEUTRAL, "dash": "dot", "width": 1.0},
        annotation_text=f"entry {underlying:.0f}",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": NEUTRAL}},
    )
    apply_layout(
        fig,
        "Portfolio delta drift as the underlying moves around entry (toy approximation)",
        height=460,
    )
    fig.update_xaxes(title_text="underlying spot (USDT)")
    fig.update_yaxes(title_text="delta")
    return fig


def panel_c_hedge_threshold(threshold: float = 0.5, interval_secs: int = 30) -> go.Figure:
    fig = go.Figure()
    rng = np.random.default_rng(7)
    t = np.linspace(0, 5 * interval_secs, 600)
    base = 0.04 * np.cumsum(rng.standard_normal(t.size))
    base = base - base[0]
    pre_hedge = base.copy()
    hedge_marks_x: list[float] = []
    hedge_marks_y: list[float] = []
    delta = 0.0
    out = []

    for i, b in enumerate(base):
        delta = b
        if abs(delta) > threshold:
            hedge_marks_x.append(t[i])
            hedge_marks_y.append(delta)
            base = base - (delta)
            delta = 0.0
        out.append(delta)
    out = np.asarray(out)

    fig.add_trace(
        go.Scatter(
            x=t,
            y=pre_hedge,
            mode="lines",
            name="hypothetical drift (no hedge)",
            line={"color": NEUTRAL, "width": 1.0, "dash": "dot"},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=t,
            y=out,
            mode="lines",
            name="portfolio delta (with hedge)",
            line={"color": PRIMARY, "width": 1.6},
        ),
    )

    if hedge_marks_x:
        fig.add_trace(
            go.Scatter(
                x=hedge_marks_x,
                y=hedge_marks_y,
                mode="markers",
                name="hedge fired",
                marker={"symbol": "x", "size": 12, "color": "#eeeeee"},
            ),
        )
    fig.add_hline(y=threshold, line={"color": POSITIVE, "dash": "dash", "width": 1.2})
    fig.add_hline(y=-threshold, line={"color": NEGATIVE, "dash": "dash", "width": 1.2})
    fig.add_hline(y=0, line={"color": GRID, "width": 1})
    apply_layout(
        fig,
        f"Synthetic delta drift with rehedge_delta_threshold = {threshold} (simulated)",
        height=460,
    )
    fig.update_xaxes(title_text="seconds since entry")
    fig.update_yaxes(title_text="portfolio delta")
    return fig


def panel_d_strike_picker(
    call_k: float,
    put_k: float,
    underlying: float,
    target: float = 0.20,
) -> go.Figure:
    fig = go.Figure()
    span = max(call_k, underlying * 1.1) - min(put_k, underlying * 0.9)
    strikes = np.linspace(
        min(put_k, underlying * 0.9) - span * 0.05,
        max(call_k, underlying * 1.1) + span * 0.05,
        30,
    )
    iv_smile = 0.30 + 0.0006 * np.abs(strikes - underlying)
    fig.add_trace(
        go.Scatter(
            x=strikes,
            y=iv_smile,
            mode="lines+markers",
            line={"color": PRIMARY, "width": 1.4},
            marker={"size": 6},
            name="IV (toy smile)",
        ),
    )
    fig.add_vline(
        x=call_k,
        line={"color": NEGATIVE, "dash": "dash", "width": 1.4},
        annotation_text=f"selected CALL {call_k:.0f}",
        annotation_position="top left",
        annotation={"font": {"size": 11, "color": NEGATIVE}},
    )
    fig.add_vline(
        x=put_k,
        line={"color": POSITIVE, "dash": "dash", "width": 1.4},
        annotation_text=f"selected PUT {put_k:.0f}",
        annotation_position="top right",
        annotation={"font": {"size": 11, "color": POSITIVE}},
    )
    fig.add_vline(
        x=underlying,
        line={"color": NEUTRAL, "dash": "dot", "width": 1.0},
        annotation_text=f"underlying {underlying:.0f}",
        annotation_position="bottom right",
        annotation={"font": {"size": 11, "color": NEUTRAL}},
    )
    apply_layout(
        fig,
        f"Strike selection: percentile heuristic places call near +{target:.0%} delta and put near -{target:.0%}",
        height=420,
    )
    fig.update_xaxes(title_text="strike (USDT)")
    fig.update_yaxes(title_text="mark_iv")
    return fig


def main() -> None:
    info = parse_log(LOG_PATH)
    print(info)
    call_k = info["call_strike"] or 81000.0
    put_k = info["put_strike"] or 75000.0
    underlying = info["underlying"] or 76800.0

    panels = {
        "panel_a_strangle_payoff.png": panel_a_strangle_payoff(call_k, put_k, underlying),
        "panel_b_delta_drift.png": panel_b_delta_drift(call_k, put_k, underlying),
        "panel_c_hedge_threshold.png": panel_c_hedge_threshold(),
        "panel_d_strike_picker.png": panel_d_strike_picker(call_k, put_k, underlying),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
