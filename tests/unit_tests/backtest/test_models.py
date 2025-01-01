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

from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import FixedFeeModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import PerContractFeeModel
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestFillModel:
    def test_instantiate_with_no_random_seed(self):
        # Arrange
        fill_model = FillModel()

        # Act, Assert
        assert not fill_model.is_slipped()
        assert fill_model.is_limit_filled()
        assert fill_model.is_stop_filled()

    def test_instantiate_with_random_seed(self):
        # Arrange
        fill_model = FillModel(random_seed=42)

        # Act, Assert
        assert not fill_model.is_slipped()
        assert fill_model.is_limit_filled()
        assert fill_model.is_stop_filled()

    def test_is_stop_filled_with_random_seed(self):
        # Arrange
        fill_model = FillModel(
            prob_fill_on_stop=0.5,
            random_seed=42,
        )

        # Act, Assert
        assert not fill_model.is_stop_filled()

    def test_is_limit_filled_with_random_seed(self):
        # Arrange
        fill_model = FillModel(
            prob_fill_on_limit=0.5,
            random_seed=42,
        )

        # Act, Assert
        assert not fill_model.is_limit_filled()

    def test_is_slipped_with_random_seed(self):
        # Arrange
        fill_model = FillModel(
            prob_slippage=0.5,
            random_seed=42,
        )

        # Act, Assert
        assert not fill_model.is_slipped()


class TestExchangeLatency:
    NANOSECONDS_IN_MILLISECOND = 1_000_000

    def test_instantiate_with_no_random_seed(self):
        latency = LatencyModel()
        assert latency.base_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
        assert latency.insert_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
        assert latency.update_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
        assert latency.cancel_latency_nanos == self.NANOSECONDS_IN_MILLISECOND


def test_fixed_fee_model() -> None:
    # Arrange
    trader_id = TestIdStubs.trader_id()
    strategy_id = TestIdStubs.strategy_id()

    aapl_xnas = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")

    order_factory = OrderFactory(
        trader_id=trader_id,
        strategy_id=strategy_id,
        clock=TestClock(),
    )

    quantity = Quantity.from_int(100)
    order = order_factory.market(
        aapl_xnas.id,
        OrderSide.BUY,
        quantity,
    )

    commission = Money(2.00, USD)
    fee_model = FixedFeeModel(commission)

    # Act
    result = fee_model.get_commission(
        order,
        quantity,
        Price.from_str("100.00"),
        aapl_xnas,
    )

    # Assert
    assert result == commission


def test_per_contract_fee_model() -> None:
    # Arrange
    trader_id = TestIdStubs.trader_id()
    strategy_id = TestIdStubs.strategy_id()

    esz4 = TestInstrumentProvider.es_future(2024, 12)

    order_factory = OrderFactory(
        trader_id=trader_id,
        strategy_id=strategy_id,
        clock=TestClock(),
    )

    contracts = 10
    quantity = Quantity.from_int(contracts)
    order = order_factory.market(
        esz4.id,
        OrderSide.BUY,
        quantity,
    )

    commission = Money(2.50, USD)
    fee_model = PerContractFeeModel(commission)

    # Act
    result = fee_model.get_commission(
        order,
        quantity,
        Price.from_str("6000.00"),
        esz4,
    )

    # Assert
    assert result == Money(commission * contracts, USD)
