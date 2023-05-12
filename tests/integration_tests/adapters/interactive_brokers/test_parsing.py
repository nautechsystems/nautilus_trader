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
from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import MarketOrder as IBMarketOrder

# fmt: off
from nautilus_trader.adapters.interactive_brokers.parsing.execution import nautilus_order_to_ib_order
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


# fmt: on

pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def test_nautilus_order_to_ib_market_order(instrument):
    # Arrange
    nautilus_market_order = TestExecStubs.market_order(instrument_id=instrument.id)

    # Act
    result = nautilus_order_to_ib_order(nautilus_market_order)

    # Assert
    expected = IBMarketOrder(action="BUY", totalQuantity=100.0)
    assert result.action == expected.action
    assert result.totalQuantity == expected.totalQuantity


def test_nautilus_order_to_ib_limit_order(instrument):
    # Arrange
    nautilus_market_order = TestExecStubs.limit_order(instrument_id=instrument.id)

    # Act
    result = nautilus_order_to_ib_order(nautilus_market_order)

    # Assert
    expected = IBLimitOrder(action="BUY", totalQuantity=100.0, lmtPrice=55.0)
    assert result.action == expected.action
    assert result.totalQuantity == expected.totalQuantity
    assert result.lmtPrice == expected.lmtPrice


@pytest.mark.parametrize(
    "contract, instrument_id",
    [
        (IBTestProviderStubs.aapl_equity_contract_details().contract, "AAPL.AMEX"),
        (IBTestProviderStubs.cl_future_contract_details().contract, "CLZ3.NYMEX"),
        (IBTestProviderStubs.eurusd_forex_contract_details().contract, "EUR/USD.IDEALPRO"),
        (
            IBTestProviderStubs.tsla_option_contract_details().contract,
            "TSLA230120C00100000.MIAX",
        ),
    ],
)
def test_ib_contract_to_instrument_id(contract, instrument_id):
    # Arrange, Act
    result = ib_contract_to_instrument_id(contract)

    # Assert
    expected = InstrumentId.from_str(instrument_id)
    assert result == expected
