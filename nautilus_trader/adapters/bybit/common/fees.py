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

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Currency


def determine_fee_currency(
    product_type: BybitProductType,
    instrument: Instrument,
    order_side: OrderSide,
    is_maker: bool,
    is_rebate: bool = False,
) -> Currency:
    """
    Determine the fee currency for a Bybit execution.

    SPOT fee currency logic per Bybit documentation:
    https://bybit-exchange.github.io/docs/v5/websocket/private/execution
    https://bybit-exchange.github.io/docs/v5/enum#spot-fee-currency-instruction

    When fee is positive (normal fees):
      - Buy → base currency, Sell → quote currency
    When fee is negative (rebates):
      - Maker: Buy → quote currency, Sell → base currency
      - Taker: Buy → base currency, Sell → quote currency

    LINEAR: Fees in settlement currency (typically USDT)
    INVERSE: Fees in settlement currency (base coin, e.g., BTC for BTCUSD)
    OPTION: Fees in settlement currency (USDC for legacy, USDT for new contracts post Feb 2025)

    Parameters
    ----------
    product_type : BybitProductType
        The product type of the instrument.
    instrument : Instrument
        The instrument being traded.
    order_side : OrderSide
        The side of the order (BUY or SELL).
    is_maker : bool
        Whether this is a maker order.
    is_rebate : bool, default False
        Whether the fee is a rebate (negative fee).

    Returns
    -------
    Currency

    """
    if product_type == BybitProductType.SPOT:
        if is_rebate and is_maker:
            # Maker with rebate: inverted logic
            return (
                instrument.quote_currency
                if order_side == OrderSide.BUY
                else instrument.base_currency
            )
        else:
            # Normal fees or taker (even with rebate)
            return (
                instrument.base_currency
                if order_side == OrderSide.BUY
                else instrument.quote_currency
            )
    elif product_type in (
        BybitProductType.LINEAR,
        BybitProductType.INVERSE,
        BybitProductType.OPTION,
    ):
        # All derivatives use their settlement currency for fees
        return instrument.settlement_currency

    # Unreachable unless new product_type added
    raise NotImplementedError(f"Unsupported product type {product_type}")
