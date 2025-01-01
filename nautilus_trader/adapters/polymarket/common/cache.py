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


def get_polymarket_trades_key(taker_order_id: str, trade_id: str) -> str:
    """
    Return the cache key for a Polymarket orders trades.

    Parameters
    ----------
    taker_order_id : str
        The aggressor Polymarket order ID for the trades.
    trade_id : str
        The trade ID.

    Returns
    -------
    str

    """
    return f"polymarket:trades:{taker_order_id}:{trade_id}"
