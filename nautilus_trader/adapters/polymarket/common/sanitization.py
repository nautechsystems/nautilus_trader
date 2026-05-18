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
Functions for redacting resolution-revealing fields from Polymarket market info.

Loaders persist the raw venue payload into ``BinaryOption.info``. For markets
that are already resolved at backtest construction time the payload includes
the answer (``closed``, ``closedTime``, ``umaResolutionStatus``, the per-token
``winner`` flag). A backtest strategy that reads ``cache.instrument(...).info``
from ``on_start`` then has a look-ahead vector that silently inflates results.

This module redacts those fields. The resolution slice is returned separately
so the loader can still surface a realized outcome for post-hoc analytics
(Brier scores, settlement PnL).

"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any


_RESOLUTION_TOP_LEVEL_KEYS: frozenset[str] = frozenset(
    {
        "closed",
        "closedTime",
        "umaResolutionStatus",
        "uma_resolution_status",
        "resolutionSource",
        "resolution_source",
    },
)

_RESOLUTION_TOKEN_KEYS: frozenset[str] = frozenset({"winner"})


def _slim_resolution_tokens(raw_tokens: list[Any]) -> list[dict[str, Any]]:
    slim_tokens: list[dict[str, Any]] = []

    for entry in raw_tokens:
        if not isinstance(entry, Mapping):
            continue

        slim_entry: dict[str, Any] = {}
        outcome = entry.get("outcome")

        if outcome is not None:
            slim_entry["outcome"] = outcome

        for key in _RESOLUTION_TOKEN_KEYS:
            if key in entry:
                slim_entry[key] = entry[key]

        if len(slim_entry) > 1:  # outcome + at least one resolution flag
            slim_tokens.append(slim_entry)

    return slim_tokens


def extract_resolution_metadata(info: Mapping[str, Any] | None) -> dict[str, Any]:
    """
    Return just the resolution-bearing slice of an info payload.

    The result is a fresh dict suitable for storage on the loader for
    post-backtest analytics. Per-token ``winner`` flags are mirrored under a
    parallel ``tokens`` list so downstream readers can still locate the
    winning outcome.

    Parameters
    ----------
    info : Mapping[str, Any] or None
        The raw market info payload.

    Returns
    -------
    dict[str, Any]
        The resolution-bearing fields, or an empty dict if none are present.

    """
    if not info:
        return {}

    metadata: dict[str, Any] = {}

    for key in _RESOLUTION_TOP_LEVEL_KEYS:
        if key in info:
            metadata[key] = info[key]

    raw_tokens = info.get("tokens")
    if isinstance(raw_tokens, list):
        slim_tokens = _slim_resolution_tokens(raw_tokens)
        if slim_tokens:
            metadata["tokens"] = slim_tokens

    return metadata


def sanitize_info_for_simulation(info: Mapping[str, Any] | None) -> dict[str, Any]:
    """
    Return a copy of ``info`` with resolution-revealing fields stripped.

    Top-level resolution keys are removed. Per-token entries are shallow-copied
    so per-token resolution flags can be redacted without mutating the caller's
    original payload.

    Parameters
    ----------
    info : Mapping[str, Any] or None
        The raw market info payload.

    Returns
    -------
    dict[str, Any]
        A new dict with the same structure as ``info`` but with resolution
        fields removed.

    """
    if not info:
        return {}

    sanitized: dict[str, Any] = {
        key: value for key, value in info.items() if key not in _RESOLUTION_TOP_LEVEL_KEYS
    }

    raw_tokens = sanitized.get("tokens")
    if isinstance(raw_tokens, list):
        scrubbed_tokens: list[Any] = []

        for entry in raw_tokens:
            if isinstance(entry, Mapping):
                scrubbed_tokens.append(
                    {
                        key: value
                        for key, value in entry.items()
                        if key not in _RESOLUTION_TOKEN_KEYS
                    },
                )
            else:
                scrubbed_tokens.append(entry)
        sanitized["tokens"] = scrubbed_tokens

    return sanitized
