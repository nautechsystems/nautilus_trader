# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestCommands:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()

        self.trader_id = TraderId("TESTER-000")
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_submit_order(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        command = SubmitOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            position_id=PositionId("P-001"),
            order=order,
            command_id=uuid,
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitOrder.from_dict(SubmitOrder.to_dict(command)) == command
        assert (
            f"SubmitOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-19700101-000000-000-001-1, position_id=P-001, strategy_id=S-001, command_id={uuid})"  # noqa
            == str(command)
        )
        assert (
            f"SubmitOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-19700101-000000-000-001-1, position_id=P-001, strategy_id=S-001, command_id={uuid})"  # noqa
            == repr(command)
        )

    def test_submit_bracket_order(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        entry = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket = self.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("1.00000"),
            take_profit=Price.from_str("1.00100"),
        )

        command = SubmitBracketOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            bracket_order=bracket,
            command_id=uuid,
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert SubmitBracketOrder.from_dict(SubmitBracketOrder.to_dict(command)) == command
        assert (
            f"SubmitBracketOrder(trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_link_id=BO-19700101-000000-000-001-1, command_id={uuid})"  # noqa
            == str(command)
        )
        assert (
            f"SubmitBracketOrder(trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_link_id=BO-19700101-000000-000-001-1, command_id={uuid})"  # noqa
            == repr(command)
        )

    def test_update_order(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = UpdateOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            price=Price.from_str("1.00000"),
            trigger=Price.from_str("1.00010"),
            quantity=Quantity.from_int(100000),
            command_id=uuid,
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert UpdateOrder.from_dict(UpdateOrder.to_dict(command)) == command
        assert (
            f"UpdateOrder(trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, quantity=100_000, price=1.00000, trigger=1.00010, command_id={uuid})"  # noqa
            == str(command)
        )
        assert (
            f"UpdateOrder(trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, quantity=100_000, price=1.00000, trigger=1.00010, command_id={uuid})"  # noqa
            == repr(command)
        )

    def test_cancel_order(self):
        # Arrange
        uuid = self.uuid_factory.generate()

        command = CancelOrder(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            instrument_id=AUDUSD_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=uuid,
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act, Assert
        assert CancelOrder.from_dict(CancelOrder.to_dict(command)) == command
        assert (
            f"CancelOrder(trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, command_id={uuid})"  # noqa
            == str(command)
        )
        assert (
            f"CancelOrder(trader_id=TRADER-001, strategy_id=S-001, instrument_id=AUD/USD.SIM, client_order_id=O-123456, venue_order_id=001, command_id={uuid})"  # noqa
            == repr(command)
        )
