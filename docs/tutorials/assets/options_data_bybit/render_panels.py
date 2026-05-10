"""
Render the Bybit options data tutorial panels from captured live runs.

Usage:

    timeout 30 ./target/release/examples/bybit-greeks-tester > /tmp/bybit_greeks.log 2>&1
    timeout 30 ./target/release/examples/bybit-option-chain > /tmp/bybit_chain.log 2>&1

    uv sync --extra visualization
    GREEKS_LOG=/tmp/bybit_greeks.log CHAIN_LOG=/tmp/bybit_chain.log \
        python3 docs/tutorials/assets/options_data_bybit/render_panels.py

Parses ``GREEKS | ...`` lines from the per-instrument tester and
``OPTION_CHAIN | ...`` / ``K=...`` lines from the chain tester, then writes
four PNG panels using the ``nautilus_dark`` tearsheet theme.

"""

from __future__ import annotations

import os
import re
from pathlib import Path

import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from nautilus_trader.analysis.tearsheet import _write_figure
from nautilus_trader.analysis.themes import get_theme


OUT = Path(__file__).resolve().parent
GREEKS_LOG = Path(os.environ.get("GREEKS_LOG", "/tmp/bybit_greeks.log"))  # noqa: S108
CHAIN_LOG = Path(os.environ.get("CHAIN_LOG", "/tmp/bybit_chain.log"))  # noqa: S108

THEME = get_theme("nautilus_dark")
TEMPLATE = THEME["template"]
COLORS = THEME["colors"]
PRIMARY = COLORS["primary"]
POSITIVE = COLORS["positive"]
NEGATIVE = COLORS["negative"]
NEUTRAL = COLORS["neutral"]
GRID = COLORS["grid"]

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TS = r"(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)"

GREEKS = re.compile(
    rf"{TS}.*GREEKS \| (?P<inst>\S+) \| "
    r"delta=(?P<delta>[\-0-9.]+) gamma=(?P<gamma>[\-0-9.]+) "
    r"vega=(?P<vega>[\-0-9.]+) theta=(?P<theta>[\-0-9.]+) rho=(?P<rho>[\-0-9.]+) \| "
    r"mark_iv=(?P<mark_iv>[\-0-9.]+) bid_iv=(?P<bid_iv>[\-0-9.]+) ask_iv=(?P<ask_iv>[\-0-9.]+) \| "
    r"underlying=(?P<u>[\-0-9.]+) oi=(?P<oi>[\-0-9.]+)",
)
CHAIN = re.compile(
    rf"{TS}.*OPTION_CHAIN \| (?P<series>\S+) \| atm=(?P<atm>[\-0-9.]+) \| "
    r"calls=(?P<calls>\d+) puts=(?P<puts>\d+) \| strikes=(?P<strikes>\d+)",
)
STRIKE_ROW = re.compile(
    r"K=(?P<k>[\-0-9.]+)\s+\|\s+CALL: bid=(?P<cb>[\-0-9.]+) ask=(?P<ca>[\-0-9.]+) "
    r"\[d=(?P<cd>[\-0-9.]+) g=(?P<cg>[\-0-9.]+) v=(?P<cv>[\-0-9.]+) iv=(?P<civ>[\-0-9.]+)%\]"
    r"\s+\|\s+PUT: bid=(?P<pb>[\-0-9.]+) ask=(?P<pa>[\-0-9.]+) "
    r"\[d=(?P<pd>[\-0-9.]+) g=(?P<pg>[\-0-9.]+) v=(?P<pv>[\-0-9.]+) iv=(?P<piv>[\-0-9.]+)%\]",
)


def parse_greeks(path: Path) -> pd.DataFrame:
    rows: list[dict] = []

    if not path.exists():
        return pd.DataFrame()
    for raw in path.read_text().splitlines():
        line = ANSI.sub("", raw)
        m = GREEKS.search(line)
        if m:
            rows.append({**m.groupdict(), "ts": pd.Timestamp(m.group("ts"))})
    df = pd.DataFrame(rows)
    if df.empty:
        return df
    for c in ["delta", "gamma", "vega", "theta", "rho", "mark_iv", "bid_iv", "ask_iv", "u", "oi"]:
        df[c] = pd.to_numeric(df[c], errors="coerce")
    df["strike"] = df["inst"].str.extract(r"-(\d+)-[CP]-")[0].astype(float)
    df["kind"] = df["inst"].str.extract(r"-(\d+)-([CP])-")[1]
    return df


def parse_chain(path: Path) -> tuple[pd.DataFrame, pd.DataFrame]:
    summaries: list[dict] = []
    strikes: list[dict] = []

    if not path.exists():
        return pd.DataFrame(), pd.DataFrame()
    current_ts = None
    current_atm = None
    current_series = None

    for raw in path.read_text().splitlines():
        line = ANSI.sub("", raw)
        m = CHAIN.search(line)
        if m:
            current_ts = pd.Timestamp(m.group("ts"))
            current_atm = float(m.group("atm"))
            current_series = m.group("series")
            summaries.append(
                {
                    "ts": current_ts,
                    "series": current_series,
                    "atm": current_atm,
                    "calls": int(m.group("calls")),
                    "puts": int(m.group("puts")),
                    "strikes": int(m.group("strikes")),
                },
            )
            continue
        s = STRIKE_ROW.search(line)
        if s and current_ts is not None:
            row = s.groupdict()
            row["ts"] = current_ts
            row["series"] = current_series
            row["atm"] = current_atm

            for k in [
                "k",
                "cb",
                "ca",
                "cd",
                "cg",
                "cv",
                "civ",
                "pb",
                "pa",
                "pd",
                "pg",
                "pv",
                "piv",
            ]:
                row[k] = float(row[k])
            strikes.append(row)
    return pd.DataFrame(summaries), pd.DataFrame(strikes)


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


def panel_a_delta_vs_strike(greeks: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if greeks.empty:
        apply_layout(fig, "No greeks captured", height=460)
        return fig
    last = (
        greeks.sort_values("ts").drop_duplicates(subset=["inst"], keep="last").sort_values("strike")
    )
    fig.add_trace(
        go.Scatter(
            x=last["strike"],
            y=last["delta"],
            mode="lines+markers",
            name="delta",
            line={"color": PRIMARY, "width": 1.6},
            marker={"size": 8, "color": PRIMARY},
        ),
    )

    if "u" in last.columns and not last["u"].isna().all():
        underlying = float(last["u"].iloc[-1])
        fig.add_vline(
            x=underlying,
            line={"color": NEGATIVE, "dash": "dash", "width": 1.2},
            annotation_text=f"underlying {underlying:.0f}",
            annotation_position="top right",
            annotation={"font": {"size": 12, "color": NEGATIVE}},
        )
    fig.add_hline(y=0.5, line={"color": GRID, "width": 1})
    apply_layout(
        fig,
        "BTC CALL delta vs strike at the nearest expiry (last per instrument)",
        height=460,
    )
    fig.update_xaxes(title_text="strike (USDT)")
    fig.update_yaxes(title_text="delta", range=[-0.05, 1.05])
    return fig


def panel_b_iv_smile(strikes: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if strikes.empty:
        apply_layout(fig, "No chain snapshots captured", height=460)
        return fig
    last_ts = strikes["ts"].max()
    last_snap = strikes[strikes["ts"] == last_ts].sort_values("k")
    if last_snap.empty:
        apply_layout(fig, "No chain snapshots captured", height=460)
        return fig
    fig.add_trace(
        go.Scatter(
            x=last_snap["k"],
            y=last_snap["civ"],
            mode="lines+markers",
            name="CALL mark_iv",
            line={"color": POSITIVE, "width": 1.4},
            marker={"size": 8, "color": POSITIVE},
        ),
    )
    fig.add_trace(
        go.Scatter(
            x=last_snap["k"],
            y=last_snap["piv"],
            mode="lines+markers",
            name="PUT mark_iv",
            line={"color": NEGATIVE, "width": 1.4, "dash": "dot"},
            marker={"size": 8, "color": NEGATIVE},
        ),
    )
    atm = last_snap["atm"].iloc[0]
    fig.add_vline(
        x=atm,
        line={"color": NEUTRAL, "dash": "dash", "width": 1.2},
        annotation_text=f"ATM {atm:.0f}",
        annotation_position="top right",
        annotation={"font": {"size": 12, "color": NEUTRAL}},
    )
    apply_layout(
        fig,
        f"IV smile per strike at {last_ts.strftime('%H:%M:%S')} UTC (latest chain snapshot)",
        height=460,
    )
    fig.update_xaxes(title_text="strike (USDT)")
    fig.update_yaxes(title_text="mark_iv (%)")
    return fig


def panel_c_underlying_oi(greeks: pd.DataFrame) -> go.Figure:
    fig = make_subplots(
        rows=2,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.5, 0.5],
        subplot_titles=("Underlying forward (USDT)", "Open interest by strike"),
    )

    if greeks.empty:
        apply_layout(fig, "No greeks captured", height=520)
        return fig
    df = greeks.copy()
    underlying = df.sort_values("ts").drop_duplicates(subset=["ts"], keep="last").sort_values("ts")
    fig.add_trace(
        go.Scatter(
            x=underlying["ts"],
            y=underlying["u"],
            mode="lines",
            line={"color": PRIMARY, "width": 1.4},
            showlegend=False,
        ),
        row=1,
        col=1,
    )
    last = df.sort_values("ts").drop_duplicates(subset=["inst"], keep="last").sort_values("strike")
    fig.add_trace(
        go.Bar(
            x=last["strike"],
            y=last["oi"],
            marker={"color": POSITIVE, "line": {"color": COLORS["background"], "width": 0.4}},
            showlegend=False,
        ),
        row=2,
        col=1,
    )
    apply_layout(
        fig,
        "Underlying trajectory and open interest profile across captured strikes",
        height=560,
    )
    fig.update_xaxes(title_text="time (UTC)", row=1, col=1)
    fig.update_xaxes(title_text="strike (USDT)", row=2, col=1)
    fig.update_yaxes(title_text="USDT", row=1, col=1)
    fig.update_yaxes(title_text="open interest", row=2, col=1)
    return fig


def panel_d_call_spread(summaries: pd.DataFrame, strikes: pd.DataFrame) -> go.Figure:
    fig = go.Figure()
    if summaries.empty or strikes.empty:
        apply_layout(fig, "No chain snapshots captured", height=420)
        return fig
    df = summaries.copy().sort_values("ts").reset_index(drop=True)
    spread_call = strikes.groupby("ts")["ca"].mean() - strikes.groupby("ts")["cb"].mean()
    df["call_spread_avg"] = df["ts"].map(spread_call)
    fig.add_trace(
        go.Scatter(
            x=df["ts"],
            y=df["call_spread_avg"],
            mode="lines+markers",
            line={"color": PRIMARY, "width": 1.4},
            marker={"size": 7},
            showlegend=False,
        ),
    )
    apply_layout(
        fig,
        "Average CALL bid-ask spread per chain snapshot (USDT)",
        height=420,
    )
    fig.update_xaxes(title_text="snapshot time (UTC)")
    fig.update_yaxes(title_text="USDT")
    return fig


def main() -> None:
    greeks = parse_greeks(GREEKS_LOG)
    summaries, strikes = parse_chain(CHAIN_LOG)
    print(
        f"greeks_rows={len(greeks)} chain_summaries={len(summaries)} "
        f"chain_strike_rows={len(strikes)}",
    )
    panels = {
        "panel_a_delta_vs_strike.png": panel_a_delta_vs_strike(greeks),
        "panel_b_iv_smile.png": panel_b_iv_smile(strikes),
        "panel_c_underlying_oi.png": panel_c_underlying_oi(greeks),
        "panel_d_call_spread.png": panel_d_call_spread(summaries, strikes),
    }

    for name, fig in panels.items():
        path = OUT / name
        _write_figure(fig, str(path))
        print(f"wrote {path} ({path.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
