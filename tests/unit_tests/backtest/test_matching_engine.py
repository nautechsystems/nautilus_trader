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

from typing import Any

import pytest

from nautilus_trader.backtest.matching_engine import OrderMatchingEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


_ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestOrderMatchingEngine:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )
        self.instrument = _ETHUSDT_PERP_BINANCE
        self.instrument_id = self.instrument.id
        self.account_id = TestIdStubs.account_id()
        self.cache = TestComponentStubs.cache()
        self.cache.add_instrument(self.instrument)

        self.matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_repr(self) -> None:
        # Arrange, Act, Assert
        assert (
            repr(self.matching_engine)
            == "OrderMatchingEngine(venue=BINANCE, instrument_id=ETHUSDT-PERP.BINANCE, raw_id=0)"
        )

    def test_set_fill_model(self) -> None:
        # Arrange
        fill_model = FillModel()

        # Act
        self.matching_engine.set_fill_model(fill_model)

        # Assert
        assert True

    def test_update_instrument(self) -> None:
        # Arrange, Act
        self.matching_engine.update_instrument(_ETHUSDT_PERP_BINANCE)

        # Assert
        assert self.matching_engine.instrument.id == _ETHUSDT_PERP_BINANCE.id

    def test_process_instrument_status(self) -> None:
        self.matching_engine.process_status(MarketStatusAction.CLOSE)
        self.matching_engine.process_status(MarketStatusAction.PRE_OPEN)
        self.matching_engine.process_status(MarketStatusAction.PAUSE)
        self.matching_engine.process_status(MarketStatusAction.TRADING)

    def test_process_market_on_close_order(self) -> None:
        order: MarketOrder = TestExecStubs.market_order(
            instrument=self.instrument,
            time_in_force=TimeInForce.AT_THE_CLOSE,
        )
        self.matching_engine.process_order(order, self.account_id)

    def test_instrument_close_expiry_closes_position(self) -> None:
        # Arrange
        exec_messages = []
        self.msgbus.register("ExecEngine.process", lambda x: exec_messages.append(x))
        tick: QuoteTick = TestDataStubs.quote_tick(
            instrument=self.instrument,
        )
        self.matching_engine.process_quote_tick(tick)
        order: MarketOrder = TestExecStubs.limit_order(
            instrument=self.instrument,
        )
        self.matching_engine.process_order(order, self.account_id)

        # Act
        instrument_close = TestDataStubs.instrument_close(
            instrument_id=self.instrument_id,
            price=Price(2, 2),
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=2,
        )
        self.matching_engine.process_instrument_close(instrument_close)

        # Assert
        assert exec_messages

    @pytest.mark.skip(reason="WIP to introduce flags")
    def test_process_auction_book(self) -> None:
        # Arrange
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=100,
            ask_price=105,
        )
        self.matching_engine.process_order_book(snapshot)

        client_order: MarketOrder = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            time_in_force=TimeInForce.AT_THE_CLOSE,
        )
        self.cache.add_order(client_order)
        self.matching_engine.process_order(client_order, self.account_id)
        self.matching_engine.process_status(MarketStatusAction.PRE_OPEN)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Act
        self.matching_engine.process_status(MarketStatusAction.PAUSE)

        # Assert
        assert self.matching_engine.msgbus.sent_count == 1
        assert isinstance(messages[0], OrderFilled)
