# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import pytest

from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.core.rust.model import OrderSide


@pytest.mark.parametrize(
    "side, price, quantity, expected",
    [
        (OrderSide.BUY, 5.0, 100.0, 100),
        (OrderSide.BUY, 1.50, 100.0, 100),
        (OrderSide.SELL, 5.0, 100.0, 400),
        (OrderSide.SELL, 1.5, 100.0, 50),
        (OrderSide.SELL, 5.0, 300.0, 1200),
    ],
)
def test_betting_instrument_notional_value_buy(instrument, side, price, quantity, expected):
    notional = instrument.notional_value(
        price=betfair_float_to_price(price),
        quantity=betfair_float_to_quantity(quantity),
        order_side=side,
    ).as_double()
    assert notional == expected
