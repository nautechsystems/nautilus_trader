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

import time
from collections.abc import Iterable
from decimal import Decimal

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.instruments import Instrument


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
        for inst_id in outcomes:
            if "-YES-" in inst_id.symbol.value:
                return inst_id
        return outcomes[0]

    preferred_id = (
        preferred if isinstance(preferred, InstrumentId) else InstrumentId.from_str(preferred)
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


def select_active_outcome_instrument(
    instruments: Iterable[Instrument],
    *,
    preferred: str | InstrumentId | None = None,
    side: str = "YES",
    now_ns: int | None = None,
) -> InstrumentId:
    """
    Select the currently active Hyperliquid outcome instrument from loaded instruments.

    This prefers non-expired instruments (expiration_ns > now) and defaults to the YES
    side.

    """
    now_ns = now_ns if now_ns is not None else time.time_ns()

    candidates: list[BinaryOption] = [
        inst
        for inst in instruments
        if isinstance(inst, BinaryOption) and is_outcome_instrument_id(inst.id)
    ]

    if not candidates:
        raise ValueError("No Hyperliquid outcome instruments were loaded")

    # Preferred wins if present.
    if preferred is not None:
        preferred_id = (
            preferred if isinstance(preferred, InstrumentId) else InstrumentId.from_str(preferred)
        )

        for inst in candidates:
            if inst.id == preferred_id:
                return inst.id
        raise ValueError(
            f"Preferred outcome instrument {preferred_id} was not found in loaded instruments",
        )

    # Filter by expiry. For the current recurring daily BTC market the InstrumentId
    # remains stable (e.g., OUTCOME-4-YES-OUTCOME) and rotation is handled by
    # instrument refresh updating expiration_ns/metadata.
    active = [inst for inst in candidates if int(inst.expiration_ns) > now_ns]
    if not active:
        active = candidates

    side = side.upper()

    def score(inst: BinaryOption) -> tuple:
        matches_side = (inst.outcome or "").upper() == side
        return (
            0 if matches_side else 1,
            int(inst.expiration_ns),
            inst.id.value,
        )

    active.sort(key=score)
    return active[0].id


def get_outcome_target_price(instrument: BinaryOption) -> Decimal:
    """
    Return the `targetPrice` threshold for a Hyperliquid `priceBinary` outcome market.

    Notes
    -----
    - For recurring `priceBinary` markets, `targetPrice` is the comparison threshold used at
      settlement (i.e., the "start/reference" price for the period).
    - The adapter stores this under `instrument.info["hyperliquid"]["price_binary"]["target_price"]`.
      For backward compatibility, it also falls back to `description_parsed["targetPrice"]`.

    """
    if not is_outcome_instrument_id(instrument.id):
        raise ValueError(f"Instrument is not a Hyperliquid outcome market: {instrument.id}")

    info = instrument.info or {}
    hl = info.get("hyperliquid") if isinstance(info, dict) else None
    if isinstance(hl, dict):
        price_binary = hl.get("price_binary")
        if isinstance(price_binary, dict) and price_binary.get("target_price") is not None:
            return Decimal(str(price_binary["target_price"]))

        parsed = hl.get("description_parsed")
        if isinstance(parsed, dict) and parsed.get("targetPrice") is not None:
            return Decimal(str(parsed["targetPrice"]))

    raise ValueError(f"No `targetPrice` found on instrument.info for {instrument.id}")
