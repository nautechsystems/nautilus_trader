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
from ib_insync import IB
from ib_insync import CommissionReport
from ib_insync import Contract
from ib_insync import Fill
from ib_insync import LimitOrder
from ib_insync import Trade

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveExecClientFactory,
)
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters._template.test_template_execution import TestBaseExecClient
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


class TestInteractiveBrokersExecution(TestBaseExecClient):
    def setup(self):
        super().setup(
            venue=IB_VENUE,
            instrument=TestInstrumentProvider.aapl_equity(),
            exec_client_factory=InteractiveBrokersLiveExecClientFactory(),
            exec_client_config=InteractiveBrokersExecClientConfig(  # noqa: S106
                username="test",
                password="test",
                account_id="DU123456",
            ),
            instrument_provider=None,
        )
        self.ib = IB()
        self.contract_details = IBTestDataStubs.contract_details("AAPL")
        self.contract = self.contract_details.contract
        self.client_order_id = TestIdStubs.client_order_id()

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

    def order_setup(self, status: OrderStatus = OrderStatus.SUBMITTED):
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("C-1"),
        )
        if status == OrderStatus.SUBMITTED:
            order = TestExecStubs.make_submitted_order(order)
        elif status == OrderStatus.ACCEPTED:
            order = TestExecStubs.make_accepted_order(order)
        else:
            raise ValueError(status)
        self.exec_client._cache.add_order(order, PositionId("1"))

    @property
    def exec_client(self) -> InteractiveBrokersExecutionClient:
        with patch(
            "nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client",
            return_value=self.ib,
        ):
            return super().exec_client

    @pytest.mark.asyncio
    @pytest.mark.skip("mocks broken")
    async def test_connect(self, mocker):
        # Arrange
        account_values = IBTestDataStubs.account_values()
        assert self.exec_client
        mocker.patch.object(self.exec_client._client, "accountValues", return_value=account_values)
        mocker.patch.object(self.exec_client._client, "connect")

        # Act
        await super().test_connect()

    def test_submit_order(self):
        # Arrange
        instrument = IBTestDataStubs.instrument("AAPL")
        contract_details = IBTestDataStubs.contract_details("AAPL")
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        order = TestExecStubs.limit_order(instrument_id=instrument.id)
        command = TestCommandStubs.submit_order_command(order=order)
        trade = IBTestExecStubs.trade_submitted()

        # Act
        with patch.object(self.exec_client._client, "placeOrder", return_value=trade) as mock:
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

        # Assert
        kwargs = mock.call_args.kwargs
        # Can't directly compare kwargs for some reason?
        assert kwargs["contract"] == expected["contract"]
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    def test_submit_bracket_order(self):
        pass

    def test_modify_order(self):
        # Arrange
        instrument = IBTestDataStubs.instrument("AAPL")
        contract_details = IBTestDataStubs.contract_details("AAPL")
        contract = contract_details.contract
        order = IBTestExecStubs.create_order(quantity=50)
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        self.exec_client._ib_insync_orders[self.client_order_id] = Trade(
            contract=contract,
            order=order,
        )

        # Act
        command = TestCommandStubs.modify_order_command(
            instrument_id=instrument.id,
            client_order_id=self.client_order_id,
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
            "order": LimitOrder(
                orderId=1,
                clientId=1,
                action="BUY",
                totalQuantity=100.0,
                lmtPrice=10.0,
                orderRef="C-1",
            ),
        }

        # Assert
        kwargs = mock.call_args.kwargs
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
        order = IBTestExecStubs.create_order()
        self.instrument_setup(instrument=instrument, contract_details=contract_details)
        self.exec_client._ib_insync_orders[TestIdStubs.client_order_id()] = Trade(
            contract=contract,
            order=order,
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

        # Assert
        kwargs = mock.call_args.kwargs
        # Can't directly compare kwargs for some reason?
        assert kwargs["order"].action == expected["order"].action
        assert kwargs["order"].totalQuantity == expected["order"].totalQuantity
        assert kwargs["order"].lmtPrice == expected["order"].lmtPrice

    @pytest.mark.asyncio
    async def test_on_submitted_event(self):
        # Arrange
        self.instrument_setup()
        self.order_setup()
        trade = IBTestExecStubs.trade_pre_submit()

        # Act
        with patch.object(self.exec_client, "generate_order_accepted") as mock:
            self.exec_client._on_order_update_event(trade)

        # Assert
        kwargs = mock.call_args.kwargs
        expected = {
            "client_order_id": ClientOrderId("C-1"),
            "instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
            "strategy_id": StrategyId("S-001"),
            "ts_event": 1646449586871811000,
            "venue_order_id": VenueOrderId("0"),
        }
        assert kwargs == expected

    @pytest.mark.asyncio
    async def test_on_exec_details(self):
        # Arrange
        self.instrument_setup()
        self.order_setup()
        contract = IBTestDataStubs.contract_details("AAPL").contract

        # Act
        execution = IBTestExecStubs.execution()
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
        trade = IBTestExecStubs.trade_submitted()
        with patch.object(self.exec_client, "generate_order_filled") as mock:
            self.exec_client._on_execution_detail(trade, fill)

        # Assert
        kwargs = mock.call_args.kwargs

        expected = {
            "client_order_id": ClientOrderId("C-1"),
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
        self.order_setup(status=OrderStatus.ACCEPTED)
        order = IBTestExecStubs.create_order(permId=1)
        trade = IBTestExecStubs.trade_submitted(order=order)

        # Act
        with patch.object(self.exec_client, "generate_order_updated") as mock:
            self.exec_client._on_order_modify(trade)

        # Assert
        kwargs = mock.call_args.kwargs
        expected = {
            "client_order_id": ClientOrderId("C-1"),
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
        self.order_setup()
        nautilus_order = TestExecStubs.limit_order()
        order = IBTestExecStubs.create_order(permId=1)
        trade = IBTestExecStubs.trade_pre_cancel(order=order)
        self.cache.add_order(nautilus_order, None)

        # Act
        with patch.object(self.exec_client, "generate_order_pending_cancel") as mock:
            self.exec_client._on_order_pending_cancel(trade)

        # Assert
        call = mock.call_args_list[0]
        expected = {
            "client_order_id": ClientOrderId("C-1"),
            "instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
            "strategy_id": StrategyId("S-001"),
            "ts_event": 1646533038455087000,
            "venue_order_id": None,
        }
        assert call.kwargs == expected

    @pytest.mark.asyncio
    async def test_on_order_cancel_cancelled(self):
        # Arrange
        self.instrument_setup()
        self.order_setup(status=OrderStatus.ACCEPTED)
        order = IBTestExecStubs.create_order(permId=1)
        trade = IBTestExecStubs.trade_canceled(order=order)

        # Act
        with patch.object(self.exec_client, "generate_order_canceled") as mock:
            self.exec_client._on_order_cancelled(trade)

        # Assert
        kwargs = mock.call_args.kwargs
        expected = {
            "client_order_id": ClientOrderId("C-1"),
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
        kwargs = mock.call_args.kwargs
        expected = {
            "balances": [
                AccountBalance.from_dict(
                    {
                        "free": "900000.08",
                        "locked": "100000.16",
                        "total": "1000000.24",
                        "currency": "AUD",
                    },
                ),
            ],
            "margins": [
                MarginBalance.from_dict(
                    {
                        "currency": "AUD",
                        "initial": "200000.97",
                        "instrument_id": None,
                        "maintenance": "200000.36",
                        "type": "MarginBalance",
                    },
                ),
            ],
            "reported": True,
            "ts_event": kwargs["ts_event"],
        }
        assert expected["balances"][0].to_dict() == kwargs["balances"][0].to_dict()
        assert expected["margins"][0].to_dict() == kwargs["margins"][0].to_dict()
        assert all([kwargs[k] == expected[k] for k in kwargs if k not in ("balances", "margins")])

    @pytest.mark.skip(reason="not implemented")
    def test_generate_order_status_report(self):
        pass
