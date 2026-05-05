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
Utilities for Hyperliquid outcome-market paper trading workflows.
"""

from __future__ import annotations

from collections.abc import Iterable
from decimal import Decimal

from nautilus_trader.model.identifiers import InstrumentId


OUTCOME_MIN_PRICE = Decimal("0.001")
OUTCOME_MAX_PRICE = Decimal("0.999")


def is_outcome_instrument_id(instrument_id: InstrumentId) -> bool:
    """
    Return whether `instrument_id` is a Hyperliquid outcome market symbol.
    """
    symbol = instrument_id.symbol.value
    return symbol.startswith("OUTCOME-") and symbol.endswith("-OUTCOME")


def validate_outcome_price(price: Decimal) -> None:
    """
    Validate that `price` sits in the paper-trading guardrail band [0.001, 0.999].
    """
    if price < OUTCOME_MIN_PRICE:
        raise ValueError(
            f"Outcome price {price} is below minimum {OUTCOME_MIN_PRICE}",
        )
    if price > OUTCOME_MAX_PRICE:
        raise ValueError(
            f"Outcome price {price} is above maximum {OUTCOME_MAX_PRICE}",
        )


def select_outcome_instrument_id(
    instrument_ids: Iterable[InstrumentId],
    preferred: str | InstrumentId | None = None,
) -> InstrumentId:
    """
    Select one outcome instrument ID from loaded instruments.

    Parameters
    ----------
    instrument_ids : Iterable[InstrumentId]
        Candidate instrument IDs.
    preferred : str | InstrumentId, optional
        Preferred ID to use if present in loaded instruments.

    Returns
    -------
    InstrumentId
        The selected outcome instrument ID.

    Raises
    ------
    ValueError
        If no outcome instruments are available, or if preferred is not an outcome ID.

    """
    outcomes = sorted(
        (inst_id for inst_id in instrument_ids if is_outcome_instrument_id(inst_id)),
        key=lambda inst_id: inst_id.value,
    )
    if not outcomes:
        raise ValueError("No Hyperliquid outcome instruments were loaded")

    if preferred is None:
        return outcomes[0]

    preferred_id = (
        preferred
        if isinstance(preferred, InstrumentId)
        else InstrumentId.from_str(preferred)
    )
    if not is_outcome_instrument_id(preferred_id):
        raise ValueError(
            f"Preferred instrument is not an outcome market: {preferred_id}",
        )
    if preferred_id not in outcomes:
        raise ValueError(
            f"Preferred outcome instrument {preferred_id} was not found in loaded instruments",
        )
    return preferred_id
