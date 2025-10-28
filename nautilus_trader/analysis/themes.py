# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Theme registry and built-in themes for tearsheet visualization.
"""

from __future__ import annotations

from difflib import get_close_matches
from typing import Any

from nautilus_trader.core.correctness import PyCondition


# Built-in theme configurations
_THEMES: dict[str, dict[str, Any]] = {
    "plotly_white": {
        "template": "plotly_white",
        "colors": {
            "primary": "#4a4a4a",  # Dark gray for table headers
            "positive": "#2ca02c",
            "negative": "#d62728",
            "neutral": "#7f7f7f",
            "background": "#ffffff",
            "grid": "#e0e0e0",
            "table_section": "#e0e0e0",  # Section header background
            "table_row_odd": "#f0f0f0",  # Odd row background
            "table_row_even": "#ffffff",  # Even row background
            "table_text": "#000000",  # Table text color
        },
    },
    "plotly_dark": {
        "template": "plotly_dark",
        "colors": {
            "primary": "#1f77b4",
            "positive": "#2ca02c",
            "negative": "#d62728",
            "neutral": "#aaaaaa",
            "background": "#111111",
            "grid": "#333333",
            "table_section": "#2a2a2a",  # Section header background
            "table_row_odd": "#1e1e1e",  # Odd row background
            "table_row_even": "#181818",  # Even row background
            "table_text": "#eeeeee",  # Table text color
        },
    },
    "nautilus": {
        "template": "plotly_white",
        "colors": {
            "primary": "#0066cc",
            "positive": "#00cc66",
            "negative": "#cc3300",
            "neutral": "#666666",
            "background": "#ffffff",
            "grid": "#e8e8e8",
            "table_section": "#e8e8e8",  # Section header background
            "table_row_odd": "#f5f5f5",  # Odd row background
            "table_row_even": "#ffffff",  # Even row background
            "table_text": "#000000",  # Table text color
        },
    },
    "nautilus_dark": {
        "template": "plotly_dark",
        "colors": {
            "primary": "#00cfbe",  # Signature teal/cyan
            "positive": "#2fadd7",  # Sky blue for positive metrics
            "negative": "#ff6b6b",  # Coral red (softer than harsh red)
            "neutral": "#a7aab5",  # Brand gray for secondary elements
            "background": "#2a2a2d",  # Lighter dark gray background
            "grid": "#202022",  # Subtle grid
            "table_section": "#35353a",  # Section header background (darker)
            "table_row_odd": "#2a2a2d",  # Odd row background (matches bg)
            "table_row_even": "#242428",  # Even row background (slightly darker)
            "table_text": "#eeeeee",  # Table text color
        },
    },
}


def get_theme(name: str) -> dict[str, Any]:
    """
    Get theme configuration by name.

    Parameters
    ----------
    name : str
        The theme name. Built-in themes: "plotly_white", "plotly_dark", "nautilus".

    Returns
    -------
    dict[str, Any]
        Theme configuration dictionary with "template" and "colors" keys.

    Raises
    ------
    KeyError
        If the theme name is not registered.

    """
    PyCondition.not_none(name, "name")

    if name not in _THEMES:
        available = ", ".join(_THEMES.keys())

        # Suggest close matches
        suggestions = get_close_matches(name, _THEMES.keys(), n=3, cutoff=0.6)
        suggestion_text = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""

        raise KeyError(
            f"Theme '{name}' not found.{suggestion_text} "
            f"Available themes: {available}. "
            f"Register custom themes with register_theme().",
        )

    return _THEMES[name].copy()


def register_theme(name: str, template: str, colors: dict[str, str]) -> None:
    """
    Register a custom theme.

    Parameters
    ----------
    name : str
        The theme name for future reference.
    template : str
        Plotly template name (e.g., "plotly_white", "plotly_dark", "ggplot2").
    colors : dict[str, str]
        Color palette dictionary. Expected keys: "primary", "positive", "negative",
        "neutral", "background", "grid". All values should be hex color codes.

    Raises
    ------
    ValueError
        If name is empty or colors dict is missing required keys.

    Examples
    --------
    >>> register_theme(
    ...     "custom",
    ...     "plotly_white",
    ...     {
    ...         "primary": "#ff6600",
    ...         "positive": "#00ff00",
    ...         "negative": "#ff0000",
    ...         "neutral": "#808080",
    ...         "background": "#ffffff",
    ...         "grid": "#dddddd",
    ...     }
    ... )

    """
    PyCondition.not_none(name, "name")
    PyCondition.not_none(template, "template")
    PyCondition.not_none(colors, "colors")

    if not name.strip():
        raise ValueError("Theme name cannot be empty")

    required_keys = {"primary", "positive", "negative", "neutral", "background", "grid"}
    missing_keys = required_keys - set(colors.keys())
    if missing_keys:
        raise ValueError(
            f"Colors dict missing required keys: {missing_keys}. Required keys: {required_keys}",
        )

    _THEMES[name] = {
        "template": template,
        "colors": colors.copy(),
    }


def list_themes() -> list[str]:
    """
    List all registered theme names.

    Returns
    -------
    list[str]
        List of available theme names.

    """
    return list(_THEMES.keys())
