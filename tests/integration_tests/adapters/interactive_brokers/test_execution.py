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
import datetime
from unittest.mock import patch

import pytest
from ib_insync import CommissionReport
from ib_insync import Contract
from ib_insync import Fill
from ib_insync import LimitOrder
from ib_insync import Trade

from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.interactive_brokers.base import InteractiveBrokersTestBase
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBExecTestStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.test_kit.stubs.commands import TestCommandStubs
from tests.test_kit.stubs.execution import TestExecStubs
from tests.test_kit.stubs.identifiers import TestIdStubs


class TestInteractiveBrokersData(InteractiveBrokersTestBase):
    def setup(self):
        super().setup()
        self.instrument = IBTestDataStubs.instrument("AAPL")
        self.contract_details = IBTestDataStubs.contract_details("AAPL")
        self.contract = self.contract_details.contract

    def instrument_setup(self, instrument=None, contract_details=None):
        instrument = instrument or self.instrument
        contract_details = contract_details or self.contract_details
        self.exec_client._instrument_provider.contract_details[
            instrument.id.value
        ] = contract_details
        self.exec_client._instrument_provider.contract_id_to_instrument_id[
            contract_details.contract.conId
        ] = instrument.id
        self.exec_client._instrument_provider.add(instrument)
        self.cache.add_instrument(instrument)

    @pytest.mark.asyncio
    async def test_factory(self, event_loop):
        # Act
        exec_client = self.exec_client

        # Assert
        assert exec_client is not None

    def test_place_order(self):
        # Arrange
        instrument = IBTestDataStubs.instrument("AAPL")
        contract_details = IBTestDataStubs.contract_details("AAPL")
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        order = TestExecStubs.limit_order(
            instrument_id=instrument.id,
        )
        command = TestCommandStubs.submit_order_command(order=order)

        # Act
        with patch.object(self.exec_client._client, "placeOrder") as mock:
            self.exec_client.submit_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(action="BUY", totalQuantity=100.0, lmtPrice=55.0),
        }
        name, args, kwargs = mock.mock_calls[0]
        # Can't directly compare kwargs for some reason?
        assert kwargs["contract"] == expected["contract"]
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    def test_update_order(self):
        # Arrange
        instrument = IBTestDataStubs.instrument("AAPL")
        contract_details = IBTestDataStubs.contract_details("AAPL")
        contract = contract_details.contract
        order = IBExecTestStubs.create_order()
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        self.exec_client._ib_insync_orders[TestIdStubs.client_order_id()] = Trade(
            contract=contract, order=order
        )

        # Act
        command = TestCommandStubs.modify_order_command(
            instrument_id=instrument.id,
            price=Price.from_int(10),
            quantity=Quantity.from_str("100"),
        )
        with patch.object(self.exec_client._client, "placeOrder") as mock:
            self.exec_client.modify_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(action="BUY", totalQuantity=100, lmtPrice=10.0),
        }
        name, args, kwargs = mock.mock_calls[0]
        # Can't directly compare kwargs for some reason?
        assert kwargs["contract"] == expected["contract"]
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    def test_cancel_order(self):
        # Arrange
        instrument = IBTestDataStubs.instrument("AAPL")
        contract_details = IBTestDataStubs.contract_details("AAPL")
        contract = contract_details.contract
        order = IBExecTestStubs.create_order()
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        self.exec_client._ib_insync_orders[TestIdStubs.client_order_id()] = Trade(
            contract=contract, order=order
        )

        # Act
        command = TestCommandStubs.cancel_order_command(instrument_id=instrument.id)
        with patch.object(self.exec_client._client, "cancelOrder") as mock:
            self.exec_client.cancel_order(command=command)

        # Assert
        expected = {
            "contract": Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="SMART",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            "order": LimitOrder(action="BUY", totalQuantity=100_000, lmtPrice=105.0),
        }
        name, args, kwargs = mock.mock_calls[0]
        # Can't directly compare kwargs for some reason?
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    def test_on_new_order(self):
        # Arrange
        self.instrument_setup()
        self.exec_client._client_order_id_to_strategy_id[
            TestIdStubs.client_order_id()
        ] = TestIdStubs.strategy_id()
        trade = IBExecTestStubs.trade_pre_submit()
        self.exec_client._venue_order_id_to_client_order_id[
            VenueOrderId(str(trade.order.permId))
        ] = TestIdStubs.client_order_id()

        # Act
        with patch.object(self.exec_client, "generate_order_submitted") as mock:
            self.exec_client._on_new_order(trade)

        # Assert
        name, args, kwargs = mock.mock_calls[0]
        expected = {
            "strategy_id": TestIdStubs.strategy_id(),
            "instrument_id": self.instrument.id,
            "client_order_id": TestIdStubs.client_order_id(),
            "ts_event": 1646449586871811000,
        }
        assert kwargs == expected

    def test_on_open_order(self):
        # Arrange
        self.instrument_setup()
        self.exec_client._client_order_id_to_strategy_id[
            TestIdStubs.client_order_id()
        ] = TestIdStubs.strategy_id()
        trade = IBExecTestStubs.trade_submitted()
        self.exec_client._venue_order_id_to_client_order_id[
            VenueOrderId(str(trade.order.permId))
        ] = TestIdStubs.client_order_id()

        # Act
        with patch.object(self.exec_client, "generate_order_accepted") as mock:
            self.exec_client._on_open_order(trade)

        # Assert
        name, args, kwargs = mock.mock_calls[0]
        expected = {
            "strategy_id": TestIdStubs.strategy_id(),
            "instrument_id": self.instrument.id,
            "client_order_id": TestIdStubs.client_order_id(),
            "venue_order_id": VenueOrderId("0"),
            "ts_event": 1646449588378175000,
        }
        assert kwargs == expected

    def test_on_exec_details(self):
        # Arrange
        self.instrument_setup()
        nautilus_order = TestExecStubs.limit_order()
        contract = IBTestDataStubs.contract_details("AAPL").contract
        self.exec_client._venue_order_id_to_client_order_id[
            VenueOrderId("0")
        ] = TestIdStubs.client_order_id()
        self.exec_client._client_order_id_to_strategy_id[
            nautilus_order.client_order_id
        ] = TestIdStubs.strategy_id()

        # Act
        execution = IBExecTestStubs.execution()
        fill = Fill(
            contract=contract,
            execution=execution,
            commissionReport=CommissionReport(
                execId="1",
                commission=1.0,
                currency="USD",
            ),
            time=datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc),
        )
        trade = IBExecTestStubs.trade_submitted()
        with patch.object(self.exec_client, "generate_order_filled") as mock:
            self.exec_client._on_execution_detail(trade, fill)

        # Assert
        name, args, kwargs = mock.mock_calls[0]

        expected = {
            "client_order_id": ClientOrderId("O-20210410-022422-001-001-1"),
            "commission": Money("1.00", USD),
            "instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
            "last_px": Price.from_str("50.00"),
            "last_qty": Quantity.from_str("100"),
            "liquidity_side": LiquiditySide.NONE,
            "order_side": 1,
            "order_type": OrderType.LIMIT,
            "quote_currency": USD,
            "strategy_id": StrategyId("S-001"),
            "trade_id": TradeId("1"),
            "ts_event": 0,
            "venue_order_id": VenueOrderId("0"),
            "venue_position_id": None,
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_on_order_modify(self):
        # Arrange
        self.instrument_setup()
        nautilus_order = TestExecStubs.limit_order()
        self.exec_client._client_order_id_to_strategy_id[
            nautilus_order.client_order_id
        ] = TestIdStubs.strategy_id()
        order = IBExecTestStubs.create_order(permId=1)
        trade = IBExecTestStubs.trade_submitted(order=order)
        self.cache.add_order(nautilus_order, None)
        self.exec_client._venue_order_id_to_client_order_id[
            VenueOrderId(str(trade.order.permId))
        ] = TestIdStubs.client_order_id()

        # Act
        with patch.object(self.exec_client, "generate_order_updated") as mock:
            self.exec_client._on_order_modify(trade)

        # Assert
        name, args, kwargs = mock.mock_calls[0]
        expected = {
            "client_order_id": nautilus_order.client_order_id,
            "instrument_id": self.instrument.id,
            "price": Price.from_str("105.00"),
            "quantity": Quantity.from_str("100000"),
            "strategy_id": TestIdStubs.strategy_id(),
            "trigger_price": None,
            "ts_event": 1646449588378175000,
            "venue_order_id": VenueOrderId("1"),
            "venue_order_id_modified": False,
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_on_order_cancel_pending(self):
        # Arrange
        self.instrument_setup()
        nautilus_order = TestExecStubs.limit_order()
        self.exec_client._client_order_id_to_strategy_id[
            nautilus_order.client_order_id
        ] = TestIdStubs.strategy_id()
        order = IBExecTestStubs.create_order(permId=1)
        trade = IBExecTestStubs.trade_pre_cancel(order=order)
        self.cache.add_order(nautilus_order, None)
        self.exec_client._venue_order_id_to_client_order_id[
            VenueOrderId(str(trade.order.permId))
        ] = TestIdStubs.client_order_id()

        # Act
        with patch.object(self.exec_client, "generate_order_pending_cancel") as mock:
            self.exec_client._on_order_cancel(trade)

        # Assert
        call = mock.call_args_list[0]
        expected = {
            "client_order_id": ClientOrderId("O-20210410-022422-001-001-1"),
            "instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
            "strategy_id": StrategyId("S-001"),
            "ts_event": 1646533038455087000,
            "venue_order_id": VenueOrderId("1"),
        }
        assert call.kwargs == expected

    @pytest.mark.asyncio
    async def test_on_order_cancel_cancelled(self):
        # Arrange
        self.instrument_setup()
        nautilus_order = TestExecStubs.limit_order()
        self.exec_client._client_order_id_to_strategy_id[
            nautilus_order.client_order_id
        ] = TestIdStubs.strategy_id()
        order = IBExecTestStubs.create_order(permId=1)
        trade = IBExecTestStubs.trade_canceled(order=order)
        self.cache.add_order(nautilus_order, None)
        self.exec_client._venue_order_id_to_client_order_id[
            VenueOrderId(str(order.permId))
        ] = nautilus_order.client_order_id

        # Act
        with patch.object(self.exec_client, "generate_order_canceled") as mock:
            self.exec_client._on_order_cancel(trade)

        # Assert
        name, args, kwargs = mock.mock_calls[0]
        expected = {
            "client_order_id": ClientOrderId("O-20210410-022422-001-001-1"),
            "instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
            "strategy_id": StrategyId("S-001"),
            "ts_event": 1646533382000847000,
            "venue_order_id": VenueOrderId("1"),
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_on_account_update(self):
        # Arrange
        account_values = IBTestDataStubs.account_values()

        # Act
        with patch.object(self.exec_client, "generate_account_state") as mock:
            self.exec_client.on_account_update(account_values)

        # Assert
        name, args, kwargs = mock.mock_calls[0]
        expected = {
            "balances": [
                AccountBalance(
                    total=Money.from_str("0.00 AUD"),
                    locked=Money.from_str("0.00 AUD"),
                    free=Money.from_str("0.00 AUD"),
                ),
                AccountBalance(
                    total=Money.from_str("100_000.00 USD"),
                    locked=Money.from_str("0.00 USD"),
                    free=Money.from_str("100_000.00 USD"),
                ),
            ],
            "margins": [
                MarginBalance(
                    initial=Money.from_str("0.00 AUD"),
                    maintenance=Money.from_str("0.00 AUD"),
                    instrument_id=None,
                )
            ],
            "reported": True,
            "ts_event": kwargs["ts_event"],
        }
        assert expected["balances"][0].to_dict() == kwargs["balances"][0].to_dict()
        assert expected["margins"][0].to_dict() == kwargs["margins"][0].to_dict()
        assert all([kwargs[k] == expected[k] for k in kwargs if k not in ("balances", "margins")])
