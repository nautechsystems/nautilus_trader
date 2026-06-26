# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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


_THEMES: dict[str, dict[str, Any]] = {
    "plotly_white": {
        "template": "plotly_white",
        "colors": {
            "primary": "#4a4a4a",
            "positive": "#2ca02c",
            "negative": "#d62728",
            "neutral": "#7f7f7f",
            "background": "#ffffff",
            "grid": "#e0e0e0",
            "table_section": "#e0e0e0",
            "table_row_odd": "#f0f0f0",
            "table_row_even": "#ffffff",
            "table_text": "#000000",
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
            "table_section": "#2a2a2a",
            "table_row_odd": "#1e1e1e",
            "table_row_even": "#181818",
            "table_text": "#eeeeee",
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
            "table_section": "#e8e8e8",
            "table_row_odd": "#f5f5f5",
            "table_row_even": "#ffffff",
            "table_text": "#000000",
        },
    },
    "nautilus_dark": {
        "template": "plotly_dark",
        "colors": {
            "primary": "#00cfbe",
            "positive": "#2fadd7",
            "negative": "#ff6b6b",
            "neutral": "#a7aab5",
            "background": "#2a2a2d",
            "grid": "#202022",
            "table_section": "#35353a",
            "table_row_odd": "#2a2a2d",
            "table_row_even": "#242428",
            "table_text": "#eeeeee",
        },
    },
}


def _require_not_none(value: Any, name: str) -> None:
    if value is None:
        raise ValueError(f"{name} must not be None")


def get_theme(name: str) -> dict[str, Any]:
    """
    Get theme configuration by name.

    Parameters
    ----------
    name : str
        The theme name. Built-in themes: "plotly_white", "plotly_dark", "nautilus", "nautilus_dark".

    Returns
    -------
    dict[str, Any]
        Theme configuration dictionary with "template" and "colors" keys.

    Raises
    ------
    KeyError
        If the theme name is not registered.

    """
    _require_not_none(name, "name")

    if name not in _THEMES:
        available = ", ".join(_THEMES.keys())

        suggestions = get_close_matches(name, _THEMES.keys(), n=3, cutoff=0.6)
        suggestion_text = f" Did you mean: {', '.join(suggestions)}?" if suggestions else ""

        raise KeyError(
            f"Theme '{name}' not found.{suggestion_text} "
            f"Available themes: {available}. "
            f"Register custom themes with register_theme().",
        )

    theme = _THEMES[name].copy()
    theme["colors"] = theme["colors"].copy()
    return theme


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
    ...     },
    ... )

    """
    _require_not_none(name, "name")
    _require_not_none(template, "template")
    _require_not_none(colors, "colors")

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
