"""
Shared chart layout for the order book backtest tutorials.
"""

import plotly.graph_objects as go

from nautilus_trader.analysis.themes import get_theme


THEME = get_theme("nautilus_dark")
COLORS = THEME["colors"]


def apply_layout(figure: go.Figure, *, height: int = 520) -> None:
    """
    Apply the shared chart typography, colors, margins, and legend placement.
    """
    figure.update_layout(
        template=THEME["template"],
        title={
            "font": {"size": 21},
            "subtitle": {"font": {"size": 14}},
            "x": 0.06,
            "xanchor": "left",
        },
        paper_bgcolor=COLORS["background"],
        plot_bgcolor=COLORS["background"],
        font={"family": "Inter, system-ui, sans-serif", "size": 13, "color": COLORS["text"]},
        margin={"l": 85, "r": 35, "t": 115, "b": 65},
        width=1200,
        height=height,
        legend={"orientation": "h", "x": 1, "xanchor": "right", "y": 1.02, "yanchor": "bottom"},
    )
    figure.update_xaxes(gridcolor=COLORS["grid"], zeroline=False)
    figure.update_yaxes(gridcolor=COLORS["grid"], zerolinecolor=COLORS["muted"])
