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
Reconciliation functions for live trading.
"""

from decimal import Decimal

from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price


def calculate_reconciliation_price(
    current_position_qty: Decimal,
    current_position_avg_px: Decimal | None,
    target_position_qty: Decimal,
    target_position_avg_px: Decimal | None,
    instrument: Instrument,
) -> Price | None:
    """
    Calculate the price needed for a reconciliation order to achieve target position.

    This is a pure function that calculates what price a fill would need to have
    to move from the current position state to the target position state with the
    correct average price.

    Parameters
    ----------
    current_position_qty : Decimal
        The current signed position quantity (positive for long, negative for short).
    current_position_avg_px : Decimal, optional
        The current position average price (can be None for flat position).
    target_position_qty : Decimal
        The target signed position quantity.
    target_position_avg_px : Decimal, optional
        The target position average price.
    instrument : Instrument
        The instrument for price precision.

    Returns
    -------
    Price or ``None``

    Notes
    -----
    The function calculates the reconciliation price using the formula:
    (target_qty * target_avg_px) = (current_qty * current_avg_px) + (qty_diff * reconciliation_px)

    """
    # If target average price is not provided, we cannot calculate
    if target_position_avg_px is None or target_position_avg_px == 0:
        return None

    # Calculate the difference in quantity
    qty_diff = target_position_qty - current_position_qty

    if qty_diff == 0:
        return None  # No reconciliation needed

    # If current position is flat, the reconciliation price equals target avg price
    if current_position_qty == 0 or current_position_avg_px is None:
        return instrument.make_price(target_position_avg_px)

    # Calculate the price needed to achieve target average
    # Formula: (target_qty * target_avg_px) = (current_qty * current_avg_px) + (qty_diff * reconciliation_px)
    # Solving for reconciliation_px:
    target_value = target_position_qty * target_position_avg_px
    current_value = current_position_qty * current_position_avg_px
    diff_value = target_value - current_value

    if qty_diff != 0:
        reconciliation_px = diff_value / qty_diff
        # Ensure price is positive
        if reconciliation_px > 0:
            return instrument.make_price(reconciliation_px)

    return None
