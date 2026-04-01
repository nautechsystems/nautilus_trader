# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class CustomClientOrderIdGenerator:
    def __init__(self, prefix: str = "t-") -> None:
        self._prefix = prefix
        self.count = 0

    def generate(self) -> ClientOrderId:
        self.count += 1
        return ClientOrderId(f"{self._prefix}{self.count}")

    def set_count(self, count: int) -> None:
        self.count = count

    def reset(self) -> None:
        self.count = 0


class RepeatingClientOrderIdGenerator:
    def __init__(self, values: list[ClientOrderId]) -> None:
        self._values = values
        self.count = 0

    def generate(self) -> ClientOrderId:
        value = self._values[self.count]
        self.count += 1
        return value

    def set_count(self, count: int) -> None:
        self.count = count

    def reset(self) -> None:
        self.count = 0


class TestOrderFactory:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

    def test_counts(self):
        # Arrange, Act, Assert
        assert self.order_factory.get_client_order_id_count() == 0
        assert self.order_factory.get_order_list_id_count() == 0

    def test_generate_client_order_id(self):
        # Arrange, Act
        result = self.order_factory.generate_client_order_id()

        # Assert
        assert result == ClientOrderId("O-19700101-000000-000-001-1")
        assert self.order_factory.get_client_order_id_count() == 1

    def test_generate_uuid_client_order_id(self):
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            use_uuid_client_order_ids=True,
        )

        # Act
        result = order_factory.generate_client_order_id()

        # Assert
        assert order_factory.use_uuid_client_order_ids
        assert len(result.value) == 36

    def test_generate_client_order_id_with_hyphens_removed(self):
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            use_hyphens_in_client_order_ids=False,
        )

        # Act
        result = order_factory.generate_client_order_id()

        # Assert
        assert result == ClientOrderId("O197001010000000000011")
        assert not order_factory.use_hyphens_in_client_order_ids

    def test_generate_uuid_client_order_id_with_hyphens_removed(self):
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            use_uuid_client_order_ids=True,
            use_hyphens_in_client_order_ids=False,
        )

        # Act
        result = order_factory.generate_client_order_id()

        # Assert
        assert order_factory.use_uuid_client_order_ids
        assert not order_factory.use_hyphens_in_client_order_ids
        assert len(result.value) == 32  # UUID without hyphens is 32 characters
        assert "-" not in result.value

    def test_generate_order_list_id(self):
        # Arrange, Act
        result = self.order_factory.generate_order_list_id()

        # Assert
        assert result == OrderListId("OL-19700101-000000-000-001-1")

    def test_set_client_order_id_count(self):
        # Arrange, Act
        self.order_factory.set_client_order_id_count(1)

        result = self.order_factory.generate_client_order_id()

        # Assert
        assert result == ClientOrderId("O-19700101-000000-000-001-2")

    def test_set_order_list_id_count(self):
        # Arrange, Act
        self.order_factory.set_order_list_id_count(1)

        result = self.order_factory.generate_order_list_id()

        # Assert
        assert result == OrderListId("OL-19700101-000000-000-001-2")

    def test_generate_client_order_id_with_custom_generator(self):
        # Arrange
        generator = CustomClientOrderIdGenerator()
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            client_order_id_generator=generator,
        )

        # Act
        result = order_factory.generate_client_order_id()

        # Assert
        assert result == ClientOrderId("t-1")
        assert order_factory.get_client_order_id_count() == 1

    def test_set_client_order_id_count_with_custom_generator(self):
        # Arrange
        generator = CustomClientOrderIdGenerator()
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            client_order_id_generator=generator,
        )

        # Act
        order_factory.set_client_order_id_count(4)
        result = order_factory.generate_client_order_id()

        # Assert
        assert result == ClientOrderId("t-5")

    def test_reset_with_custom_generator(self):
        # Arrange
        generator = CustomClientOrderIdGenerator()
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            client_order_id_generator=generator,
        )

        # Act
        order_factory.generate_client_order_id()
        order_factory.reset()

        # Assert
        assert order_factory.get_client_order_id_count() == 0

    def test_custom_generator_requires_generate(self):
        # Arrange
        class MissingGenerate:
            def __init__(self) -> None:
                self.count = 0

            def set_count(self, count: int) -> None:
                self.count = count

            def reset(self) -> None:
                self.count = 0

        # Act, Assert
        with pytest.raises(TypeError):
            OrderFactory(
                trader_id=self.trader_id,
                strategy_id=self.strategy_id,
                clock=TestClock(),
                client_order_id_generator=MissingGenerate(),
            )

    def test_custom_generator_retries_when_cache_contains_id(self):
        # Arrange
        cache = TestComponentStubs.cache()
        existing_order = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.5"),
            client_order_id=ClientOrderId("t-1"),
        )
        cache.add_order(existing_order)
        generator = RepeatingClientOrderIdGenerator(
            [ClientOrderId("t-1"), ClientOrderId("t-2")],
        )
        order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
            cache=cache,
            client_order_id_generator=generator,
        )

        # Act
        result = order_factory.generate_client_order_id()

        # Assert
        assert result == ClientOrderId("t-2")
        assert order_factory.get_client_order_id_count() == 2

    def test_create_list(self):
        # Arrange
        order1 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.5"),
        )

        order2 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.5"),
        )

        order3 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.5"),
        )

        # Act
        order_list = self.order_factory.create_list([order1, order2, order3])

        # Assert
        assert len(order_list) == 3
        assert self.order_factory.get_order_list_id_count() == 1
