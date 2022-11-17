# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from ib_insync import Contract as IBContract
from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import MarketOrder as IBMarketOrder

from nautilus_trader.adapters.interactive_brokers.parsing.execution import (
    nautilus_order_to_ib_order,
)
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    ib_contract_to_instrument_id,
)
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import (
    nautilus_instrument_to_ib_contract,
)
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.test_kit.stubs.execution import TestExecStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = IBTestDataStubs.instrument("AAPL")

    def test_nautilus_order_to_ib_market_order(self):
        # Arrange
        nautilus_market_order = TestExecStubs.market_order(instrument_id=self.instrument.id)

        # Act
        result = nautilus_order_to_ib_order(nautilus_market_order)

        # Assert
        expected = IBMarketOrder(action="BUY", totalQuantity=100.0)
        assert result.action == expected.action
        assert result.totalQuantity == expected.totalQuantity

    def test_nautilus_order_to_ib_limit_order(self):
        # Arrange
        nautilus_market_order = TestExecStubs.limit_order(instrument_id=self.instrument.id)

        # Act
        result = nautilus_order_to_ib_order(nautilus_market_order)

        # Assert
        expected = IBLimitOrder(action="BUY", totalQuantity=100.0, lmtPrice=55.0)
        assert result.action == expected.action
        assert result.totalQuantity == expected.totalQuantity
        assert result.lmtPrice == expected.lmtPrice

    def test_nautilus_instrument_to_ib_contract(self):
        # Act
        result = nautilus_instrument_to_ib_contract(self.instrument)

        # Assert
        expected = IBContract(
            secType="STK", exchange="SMART", primaryExchange="NASDAQ", localSymbol="AAPL"
        )
        assert result.secType == expected.secType
        assert result.exchange == expected.exchange
        assert result.primaryExchange == expected.primaryExchange
        assert result.localSymbol == expected.localSymbol

    def test_ib_contract_to_instrument_id(self):
        # Arrange
        ib_contract = IBTestDataStubs.contract_details(self.instrument.native_symbol.value).contract

        # Act
        result = ib_contract_to_instrument_id(ib_contract)

        # Assert
        expected = self.instrument.id
        assert result.symbol == expected.symbol
        assert result.venue == expected.venue
