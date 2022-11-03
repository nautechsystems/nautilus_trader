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

import inspect
from datetime import datetime
from typing import Optional

from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId


class MockExecutionClient(ExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.

    Parameters
    ----------
    client_id : ClientId
        The client ID.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    """

    def __init__(
        self,
        client_id,
        venue,
        account_type,
        base_currency,
        msgbus,
        cache,
        clock,
        logger,
        config=None,
    ):
        super().__init__(
            client_id=client_id,
            venue=venue,
            oms_type=OMSType.HEDGING,
            account_type=account_type,
            base_currency=base_currency,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self.calls = []
        self.commands = []

    def _start(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected()

    def _stop(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected(False)

    def _reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def _dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- COMMANDS ---------------------------------------------------------------------------------

    def account_inquiry(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order_list(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def modify_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_all_orders(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)


class MockLiveExecutionClient(LiveExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.

    Parameters
    ----------
    client_id : ClientId
        The client ID.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    """

    def __init__(
        self,
        loop,
        client_id,
        venue,
        account_type,
        base_currency,
        instrument_provider,
        msgbus,
        cache,
        clock,
        logger,
    ):
        super().__init__(
            loop=loop,
            client_id=client_id,
            venue=venue,
            oms_type=OMSType.HEDGING,
            account_type=account_type,
            base_currency=base_currency,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._set_account_id(AccountId(f"{client_id}-001"))
        self._order_status_reports: dict[VenueOrderId, OrderStatusReport] = {}
        self._trades_reports: dict[VenueOrderId, list[TradeReport]] = {}
        self._position_status_reports: dict[InstrumentId, list[PositionStatusReport]] = {}

        self.calls = []
        self.commands = []

    def connect(self) -> None:
        pass  # Do nothing

    def disconnect(self) -> None:
        pass  # Do nothing

    def add_order_status_report(self, report: OrderStatusReport) -> None:
        self._order_status_reports[report.venue_order_id] = report

    def add_trade_reports(self, venue_order_id: VenueOrderId, trades: list[TradeReport]) -> None:
        self._trades_reports[venue_order_id] = trades

    def add_position_status_report(self, report: PositionStatusReport) -> None:
        if report.instrument_id not in self._position_status_reports:
            self._position_status_reports[report.instrument_id] = []
        self._position_status_reports[report.instrument_id].append(report)

    def dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- COMMANDS ---------------------------------------------------------------------------------

    def account_inquiry(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order_list(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def modify_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_all_orders(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def query_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        return self._order_status_reports.get(venue_order_id)

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        reports = []
        for _, report in self._order_status_reports.items():
            reports.append(report)

        if instrument_id is not None:
            reports = [r for r in reports if r.instrument_id == instrument_id]

        if start is not None:
            reports = [r for r in reports if r.ts_accepted >= start]

        if end is not None:
            reports = [r for r in reports if r.ts_accepted <= end]

        return reports

    async def generate_trade_reports(
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> list[TradeReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        if venue_order_id is not None:
            trades = self._trades_reports.get(venue_order_id, [])
        else:
            trades = []
            for t_list in self._trades_reports.values():
                trades = [*trades, *t_list]

        if instrument_id is not None:
            trades = [t for t in trades if t.instrument_id == instrument_id]

        if start is not None:
            trades = [t for t in trades if t.ts_event >= start]

        if end is not None:
            trades = [t for t in trades if t.ts_event <= end]

        return trades

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> list[PositionStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        if instrument_id is not None:
            reports = self._position_status_reports.get(instrument_id, [])
        else:
            reports = []
            for p_list in self._position_status_reports.values():
                reports = [*reports, *p_list]

        if start is not None:
            reports = [r for r in reports if r.ts_event >= start]

        if end is not None:
            reports = [r for r in reports if r.ts_event <= end]

        return reports
