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

from __future__ import annotations

from nautilus_trader.common.config import NautilusConfig


class PortfolioConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``Portfolio`` instances.

    Parameters
    ----------
    use_mark_prices : bool, default False
        The type of prices used for P&L and net exposure calculations.
        If False (default), uses quote prices if available; otherwise, last trade prices
        (or falls back to bar prices if `bar_updates` is True).
        If True, uses mark prices.
    use_mark_xrates : bool, default False
        The type of exchange rates used for P&L and net exposure calculations.
        If False (default), uses quote prices.
        If True, uses mark prices.
    bar_updates : bool, default True
        If external bar prices should be considered for calculations.
    convert_to_account_base_currency : bool, default True
        If calculations should be converted into each account's base currency.
        This setting is only effective for accounts with a specified base currency.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    use_mark_prices: bool = False
    use_mark_xrates: bool = False
    bar_updates: bool = True
    convert_to_account_base_currency: bool = True
    debug: bool = False
