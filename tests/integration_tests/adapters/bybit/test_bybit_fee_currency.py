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

import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.fees import determine_fee_currency
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import OrderSide
from tests.integration_tests.adapters.bybit.conftest import create_bybit_inverse_perpetual
from tests.integration_tests.adapters.bybit.conftest import create_bybit_linear_perpetual
from tests.integration_tests.adapters.bybit.conftest import create_bybit_option_instrument
from tests.integration_tests.adapters.bybit.conftest import create_bybit_spot_instrument


@pytest.mark.parametrize(
    "order_side,is_maker,is_rebate,expected_currency",
    [
        # Normal fees (positive)
        (OrderSide.BUY, True, False, BTC),  # Buy maker -> base
        (OrderSide.BUY, False, False, BTC),  # Buy taker -> base
        (OrderSide.SELL, True, False, USDT),  # Sell maker -> quote
        (OrderSide.SELL, False, False, USDT),  # Sell taker -> quote
        # Maker rebates (negative fee, inverted logic)
        (OrderSide.BUY, True, True, USDT),  # Buy maker rebate -> quote (inverted)
        (OrderSide.SELL, True, True, BTC),  # Sell maker rebate -> base (inverted)
        # Taker with rebate flag (should still use normal logic)
        (OrderSide.BUY, False, True, BTC),  # Buy taker -> base (normal)
        (OrderSide.SELL, False, True, USDT),  # Sell taker -> quote (normal)
    ],
)
def test_spot_fee_currency(order_side, is_maker, is_rebate, expected_currency):
    """
    Test SPOT fee currency determination according to Bybit rules.
    """
    instrument = create_bybit_spot_instrument(BTC, USDT)

    fee_currency = determine_fee_currency(
        product_type=BybitProductType.SPOT,
        instrument=instrument,
        order_side=order_side,
        is_maker=is_maker,
        is_rebate=is_rebate,
    )

    assert fee_currency == expected_currency


def test_spot_eth_btc_fee_currency():
    """
    Test SPOT fee currency for ETH/BTC pair.
    """
    instrument = create_bybit_spot_instrument(ETH, BTC)

    # Normal buy -> ETH (base)
    assert (
        determine_fee_currency(
            BybitProductType.SPOT,
            instrument,
            OrderSide.BUY,
            is_maker=False,
            is_rebate=False,
        )
        == ETH
    )

    # Normal sell -> BTC (quote)
    assert (
        determine_fee_currency(
            BybitProductType.SPOT,
            instrument,
            OrderSide.SELL,
            is_maker=False,
            is_rebate=False,
        )
        == BTC
    )

    # Maker rebate buy -> BTC (quote, inverted)
    assert (
        determine_fee_currency(
            BybitProductType.SPOT,
            instrument,
            OrderSide.BUY,
            is_maker=True,
            is_rebate=True,
        )
        == BTC
    )

    # Maker rebate sell -> ETH (base, inverted)
    assert (
        determine_fee_currency(
            BybitProductType.SPOT,
            instrument,
            OrderSide.SELL,
            is_maker=True,
            is_rebate=True,
        )
        == ETH
    )


@pytest.mark.parametrize(
    "order_side,is_maker,is_rebate",
    [
        (OrderSide.BUY, True, False),
        (OrderSide.BUY, False, False),
        (OrderSide.SELL, True, False),
        (OrderSide.SELL, False, False),
        (OrderSide.BUY, True, True),  # Rebate doesn't affect derivatives
        (OrderSide.SELL, True, True),
    ],
)
def test_linear_fee_currency_always_usdt(order_side, is_maker, is_rebate):
    """
    Test LINEAR perpetual always uses settlement currency (USDT) for fees.
    """
    instrument = create_bybit_linear_perpetual()

    fee_currency = determine_fee_currency(
        product_type=BybitProductType.LINEAR,
        instrument=instrument,
        order_side=order_side,
        is_maker=is_maker,
        is_rebate=is_rebate,
    )

    assert fee_currency == USDT


@pytest.mark.parametrize(
    "order_side,is_maker,is_rebate",
    [
        (OrderSide.BUY, True, False),
        (OrderSide.BUY, False, False),
        (OrderSide.SELL, True, False),
        (OrderSide.SELL, False, False),
        (OrderSide.BUY, True, True),  # Rebate doesn't affect derivatives
        (OrderSide.SELL, True, True),
    ],
)
def test_inverse_fee_currency_always_btc(order_side, is_maker, is_rebate):
    """
    Test INVERSE perpetual always uses settlement currency (BTC) for fees.
    """
    instrument = create_bybit_inverse_perpetual()

    fee_currency = determine_fee_currency(
        product_type=BybitProductType.INVERSE,
        instrument=instrument,
        order_side=order_side,
        is_maker=is_maker,
        is_rebate=is_rebate,
    )

    assert fee_currency == BTC


def test_option_fee_currency():
    """
    Test OPTION uses settlement currency for fees.
    """
    instrument = create_bybit_option_instrument()

    # All combinations should return settlement currency
    for order_side in [OrderSide.BUY, OrderSide.SELL]:
        for is_maker in [True, False]:
            for is_rebate in [True, False]:
                fee_currency = determine_fee_currency(
                    product_type=BybitProductType.OPTION,
                    instrument=instrument,
                    order_side=order_side,
                    is_maker=is_maker,
                    is_rebate=is_rebate,
                )
                assert fee_currency == USDC
