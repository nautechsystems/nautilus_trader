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
Price conversion utilities for Interactive Brokers adapter.

Interactive Brokers uses a price magnifier field in contract details to scale prices.
All prices received from IB need to be divided by the price magnifier to get the real
price. All prices sent to IB need to be multiplied by the price magnifier.

"""

from nautilus_trader.model.identifiers import InstrumentId


def ib_price_to_nautilus_price(ib_price: float, price_magnifier: int) -> float:
    """
    Convert an Interactive Brokers price to a Nautilus price.

    Parameters
    ----------
    ib_price : float
        The price received from Interactive Brokers.
    price_magnifier : int
        The price magnifier from the contract details.

    Returns
    -------
    float
        The real price for use in Nautilus.

    """
    if price_magnifier <= 0:
        return ib_price

    return ib_price / price_magnifier


def nautilus_price_to_ib_price(nautilus_price: float, price_magnifier: int) -> float:
    """
    Convert a Nautilus price to an Interactive Brokers price.

    Parameters
    ----------
    nautilus_price : float
        The price from Nautilus to send to Interactive Brokers.
    price_magnifier : int
        The price magnifier from the contract details.

    Returns
    -------
    float
        The scaled price for sending to Interactive Brokers.

    """
    if price_magnifier <= 0:
        return nautilus_price

    return nautilus_price * price_magnifier


def get_price_magnifier_for_instrument(
    instrument_id: InstrumentId,
    instrument_provider,
) -> int:
    """
    Get the price magnifier for an instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument identifier.
    instrument_provider : InteractiveBrokersInstrumentProvider | None
        The instrument provider to get contract details from.

    Returns
    -------
    int
        The price magnifier, defaults to 1 if not found or provider is None.

    """
    if instrument_provider is None:
        return 1

    return instrument_provider.get_price_magnifier(instrument_id)
