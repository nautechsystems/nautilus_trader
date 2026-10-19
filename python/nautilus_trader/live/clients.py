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
"""
Base classes for Python adapter clients and their factories.
"""

from __future__ import annotations

import inspect
from typing import TYPE_CHECKING

from nautilus_trader._libnautilus.live import _ClientOutput
from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime
from nautilus_trader.config import DataClientConfig
from nautilus_trader.config import ExecutionClientConfig
from nautilus_trader.model import ClientId


if TYPE_CHECKING:
    from asyncio import AbstractEventLoop
    from asyncio import Task
    from collections.abc import Coroutine
    from decimal import Decimal
    from types import NotImplementedType

    from nautilus_trader.common import Clock
    from nautilus_trader.live import BatchCancelOrders
    from nautilus_trader.live import BatchModifyOrders
    from nautilus_trader.live import CancelAllOrders
    from nautilus_trader.live import CancelOrder
    from nautilus_trader.live import ClientCache
    from nautilus_trader.live import GenerateFillReports
    from nautilus_trader.live import GenerateOrderStatusReport
    from nautilus_trader.live import GenerateOrderStatusReports
    from nautilus_trader.live import GeneratePositionStatusReports
    from nautilus_trader.live import InstrumentProvider
    from nautilus_trader.live import ModifyOrder
    from nautilus_trader.live import QueryAccount
    from nautilus_trader.live import QueryOrder
    from nautilus_trader.live import RequestBars
    from nautilus_trader.live import RequestBookDeltas
    from nautilus_trader.live import RequestBookDepth
    from nautilus_trader.live import RequestBookSnapshot
    from nautilus_trader.live import RequestCustomData
    from nautilus_trader.live import RequestFundingRates
    from nautilus_trader.live import RequestInstrument
    from nautilus_trader.live import RequestInstruments
    from nautilus_trader.live import RequestOptionChainReferencePrice
    from nautilus_trader.live import RequestQuotes
    from nautilus_trader.live import RequestTrades
    from nautilus_trader.live import SubmitOrder
    from nautilus_trader.live import SubmitOrderList
    from nautilus_trader.live import SubscribeBars
    from nautilus_trader.live import SubscribeBookDeltas
    from nautilus_trader.live import SubscribeBookDepth
    from nautilus_trader.live import SubscribeCustomData
    from nautilus_trader.live import SubscribeFundingRates
    from nautilus_trader.live import SubscribeIndexPrices
    from nautilus_trader.live import SubscribeInstrument
    from nautilus_trader.live import SubscribeInstrumentClose
    from nautilus_trader.live import SubscribeInstruments
    from nautilus_trader.live import SubscribeInstrumentStatus
    from nautilus_trader.live import SubscribeMarkPrices
    from nautilus_trader.live import SubscribeOptionGreeks
    from nautilus_trader.live import SubscribeQuotes
    from nautilus_trader.live import SubscribeTrades
    from nautilus_trader.live import UnsubscribeBars
    from nautilus_trader.live import UnsubscribeBookDeltas
    from nautilus_trader.live import UnsubscribeBookDepth
    from nautilus_trader.live import UnsubscribeCustomData
    from nautilus_trader.live import UnsubscribeFundingRates
    from nautilus_trader.live import UnsubscribeIndexPrices
    from nautilus_trader.live import UnsubscribeInstrument
    from nautilus_trader.live import UnsubscribeInstrumentClose
    from nautilus_trader.live import UnsubscribeInstruments
    from nautilus_trader.live import UnsubscribeInstrumentStatus
    from nautilus_trader.live import UnsubscribeMarkPrices
    from nautilus_trader.live import UnsubscribeOptionGreeks
    from nautilus_trader.live import UnsubscribeQuotes
    from nautilus_trader.live import UnsubscribeTrades
    from nautilus_trader.model import AccountBalance
    from nautilus_trader.model import AccountId
    from nautilus_trader.model import AccountType
    from nautilus_trader.model import ClientOrderId
    from nautilus_trader.model import Currency
    from nautilus_trader.model import ExecutionMassStatus
    from nautilus_trader.model import FillReport
    from nautilus_trader.model import InstrumentId
    from nautilus_trader.model import LiquiditySide
    from nautilus_trader.model import MarginBalance
    from nautilus_trader.model import Money
    from nautilus_trader.model import OmsType
    from nautilus_trader.model import OrderStatusReport
    from nautilus_trader.model import PositionId
    from nautilus_trader.model import PositionStatusReport
    from nautilus_trader.model import Price
    from nautilus_trader.model import Quantity
    from nautilus_trader.model import StrategyId
    from nautilus_trader.model import TradeId
    from nautilus_trader.model import TraderId
    from nautilus_trader.model import Venue
    from nautilus_trader.model import VenueOrderId


class DataClientFactory:
    """
    Constructs a data client without scheduling work or opening network resources.
    """

    @staticmethod
    def create(
        *,
        name: str,
        config: DataClientConfig,
        cache: ClientCache,
        clock: Clock,
    ) -> DataClient:
        """
        Construct a client using the owning node context.
        """
        raise NotImplementedError


class _Client:
    """
    An adapter whose async hooks run on the owning node's event loop.
    """

    def __init__(  # noqa: PLR0913 - Preserve the native client and event field contract.
        self,
        *,
        name: str,
        config: DataClientConfig | ExecutionClientConfig,
        cache: ClientCache,
        clock: Clock,
        venue: Venue | None = None,
        instrument_provider: InstrumentProvider | None = None,
    ) -> None:
        """
        Retain configuration and local state without starting network work.
        """
        self.client_id = ClientId(name)
        self.venue = venue
        self.config = config
        self.cache = cache
        self.clock = clock
        self._output = _ClientOutput()
        self._runtime = ClientRuntime(self)
        self.instrument_provider = instrument_provider
        if instrument_provider is not None:
            instrument_provider._bind(self._runtime)  # noqa: SLF001 - The client and runtime jointly own this private lifecycle.

    @property
    def loop(self) -> AbstractEventLoop | None:
        """
        Return the actual owner loop after startup binding, or None before binding.
        """
        return self._runtime.event_loop

    @property
    def is_connected(self) -> bool:
        """
        Return whether the connection hook completed before shutdown.
        """
        return self._runtime.connected

    def create_task(
        self,
        coroutine: Coroutine[object, object, object],
        name: str = "background",
    ) -> Task[object]:
        """
        Schedule and supervise a coroutine on the bound owner loop.
        """
        return self._runtime.create_task(coroutine, name)

    def _handle_instrument(self, instrument: object) -> None:
        self._output.instrument(instrument)

    def _handle_data(self, data: object) -> None:
        self._output.data(data)

    def _handle_response(self, response: object) -> None:
        self._output.response(response)

    async def _connect(self) -> None:
        raise NotImplementedError

    async def _disconnect(self) -> None:
        raise NotImplementedError


class DataClient(_Client):
    """
    An adapter providing custom or instrument data through queued events.
    """

    def __init__(  # noqa: PLR0913 - Preserve the native client and event field contract.
        self,
        *,
        name: str,
        config: DataClientConfig,
        cache: ClientCache,
        clock: Clock,
        venue: Venue | None = None,
        instrument_provider: InstrumentProvider | None = None,
    ) -> None:
        """
        Retain configuration and local state without starting network work.
        """
        if not isinstance(config, DataClientConfig):
            raise TypeError("Expected DataClientConfig")
        super().__init__(
            name=name,
            config=config,
            cache=cache,
            clock=clock,
            venue=venue,
            instrument_provider=instrument_provider,
        )

    async def _subscribe(self, command: SubscribeCustomData) -> None:
        raise NotImplementedError

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        raise NotImplementedError

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        raise NotImplementedError

    async def _subscribe_book_deltas(self, command: SubscribeBookDeltas) -> None:
        raise NotImplementedError

    async def _subscribe_book_depth(self, command: SubscribeBookDepth) -> None:
        raise NotImplementedError

    async def _subscribe_quotes(self, command: SubscribeQuotes) -> None:
        raise NotImplementedError

    async def _subscribe_trades(self, command: SubscribeTrades) -> None:
        raise NotImplementedError

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        raise NotImplementedError

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        raise NotImplementedError

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        raise NotImplementedError

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        raise NotImplementedError

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        raise NotImplementedError

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        raise NotImplementedError

    async def _subscribe_option_greeks(self, command: SubscribeOptionGreeks) -> None:
        raise NotImplementedError

    async def _unsubscribe(self, command: UnsubscribeCustomData) -> None:
        raise NotImplementedError

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        raise NotImplementedError

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        raise NotImplementedError

    async def _unsubscribe_book_deltas(self, command: UnsubscribeBookDeltas) -> None:
        raise NotImplementedError

    async def _unsubscribe_book_depth(self, command: UnsubscribeBookDepth) -> None:
        raise NotImplementedError

    async def _unsubscribe_quotes(self, command: UnsubscribeQuotes) -> None:
        raise NotImplementedError

    async def _unsubscribe_trades(self, command: UnsubscribeTrades) -> None:
        raise NotImplementedError

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        raise NotImplementedError

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        raise NotImplementedError

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        raise NotImplementedError

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        raise NotImplementedError

    async def _unsubscribe_instrument_status(
        self,
        command: UnsubscribeInstrumentStatus,
    ) -> None:
        raise NotImplementedError

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        raise NotImplementedError

    async def _unsubscribe_option_greeks(self, command: UnsubscribeOptionGreeks) -> None:
        raise NotImplementedError

    async def _request_data(self, request: RequestCustomData) -> None:
        raise NotImplementedError

    async def _request_instruments(self, request: RequestInstruments) -> None:
        raise NotImplementedError

    async def _request_instrument(self, request: RequestInstrument) -> None:
        raise NotImplementedError

    async def _request_book_snapshot(self, request: RequestBookSnapshot) -> None:
        raise NotImplementedError

    async def _request_quotes(self, request: RequestQuotes) -> None:
        raise NotImplementedError

    async def _request_trades(self, request: RequestTrades) -> None:
        raise NotImplementedError

    async def _request_funding_rates(self, request: RequestFundingRates) -> None:
        raise NotImplementedError

    async def _request_option_chain_reference_price(
        self,
        request: RequestOptionChainReferencePrice,
    ) -> None:
        raise NotImplementedError

    async def _request_bars(self, request: RequestBars) -> None:
        raise NotImplementedError

    async def _request_book_depth(self, request: RequestBookDepth) -> None:
        raise NotImplementedError

    async def _request_book_deltas(self, request: RequestBookDeltas) -> None:
        raise NotImplementedError


class MarketDataClient(DataClient):
    """
    A data client providing instrument market data.
    """


class ExecutionClientFactory:
    """
    Constructs an execution client with the owning node's identity and clock.
    """

    @staticmethod
    def create(
        *,
        name: str,
        config: ExecutionClientConfig,
        cache: ClientCache,
        clock: Clock,
        trader_id: TraderId,
    ) -> ExecutionClient:
        """
        Construct a client using the owning node context.
        """
        raise NotImplementedError


class ExecutionClient(_Client):
    """
    An execution adapter which emits account, order, and reconciliation events.
    """

    def __init__(  # noqa: PLR0913 - Preserve the native client and event field contract.
        self,
        *,
        name: str,
        config: ExecutionClientConfig,
        cache: ClientCache,
        clock: Clock,
        trader_id: TraderId,
        venue: Venue | None,
        account_id: AccountId,
        account_type: AccountType,
        oms_type: OmsType,
        base_currency: Currency | None = None,
        instrument_provider: InstrumentProvider | None = None,
        position_reconciliation_tolerance: Decimal | None = None,
    ) -> None:
        """
        Retain configuration and local state without starting network work.
        """
        if not isinstance(config, ExecutionClientConfig):
            raise TypeError("Expected ExecutionClientConfig")
        super().__init__(
            name=name,
            config=config,
            cache=cache,
            clock=clock,
            venue=venue,
            instrument_provider=instrument_provider,
        )
        self.trader_id = trader_id
        self.account_id = account_id
        self.account_type = account_type
        self.oms_type = oms_type
        self.base_currency = base_currency
        self.position_reconciliation_tolerance = position_reconciliation_tolerance

    def _handle_event(self, event: object) -> None:
        self._output.event(event)

    def _handle_report(self, report: object, fills: list[FillReport] | None = None) -> None:
        self._output.report(report, fills)

    def generate_account_state(
        self,
        balances: list[AccountBalance],
        margins: list[MarginBalance],
        reported: bool,  # noqa: FBT001 - Preserve the established event or provider call signature.
        ts_event: int,
        info: dict[str, object] | None = None,
    ) -> None:
        """
        Queue an account snapshot for core processing.
        """
        self._output.account_state(
            balances,
            margins,
            reported,
            ts_event,
            self.clock.timestamp_ns(),
            info,
        )

    def generate_order_denied(self, order: object, reason: str) -> None:
        """
        Queue the order denied event using the owning clock.
        """
        self._output.order_denied(order, reason, self.clock.timestamp_ns())

    def generate_order_submitted(self, order: object) -> None:
        """
        Queue the order submitted event using the owning clock.
        """
        self._output.order_submitted(order, self.clock.timestamp_ns())

    def generate_order_rejected(
        self,
        order: object,
        reason: str,
        ts_event: int,
        due_post_only: bool,  # noqa: FBT001 - Preserve the established event or provider call signature.
    ) -> None:
        """
        Queue the order rejected event using the owning clock.
        """
        self._output.order_rejected(
            order,
            reason,
            ts_event,
            self.clock.timestamp_ns(),
            due_post_only,
        )

    def generate_order_accepted(
        self,
        order: object,
        venue_order_id: VenueOrderId,
        ts_event: int,
    ) -> None:
        """
        Queue the order accepted event using the owning clock.
        """
        self._output.order_accepted(order, venue_order_id, ts_event, self.clock.timestamp_ns())

    def generate_order_modify_rejected(
        self,
        order: object,
        venue_order_id: VenueOrderId | None,
        reason: str,
        ts_event: int,
    ) -> None:
        """
        Queue the order modify rejected event using the owning clock.
        """
        self._output.order_modify_rejected(
            order,
            venue_order_id,
            reason,
            ts_event,
            self.clock.timestamp_ns(),
        )

    def generate_order_cancel_rejected(
        self,
        order: object,
        venue_order_id: VenueOrderId | None,
        reason: str,
        ts_event: int,
    ) -> None:
        """
        Queue the order cancel rejected event using the owning clock.
        """
        self._output.order_cancel_rejected(
            order,
            venue_order_id,
            reason,
            ts_event,
            self.clock.timestamp_ns(),
        )

    def generate_order_updated(  # noqa: PLR0913, PLR0917 - Preserve the native client and event field contract.
        self,
        order: object,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price | None,
        trigger_price: Price | None,
        protection_price: Price | None,
        ts_event: int,
    ) -> None:
        """
        Queue the order updated event using the owning clock.
        """
        self._output.order_updated(
            order,
            venue_order_id,
            quantity,
            price,
            trigger_price,
            protection_price,
            ts_event,
            self.clock.timestamp_ns(),
        )

    def generate_order_canceled(
        self,
        order: object,
        venue_order_id: VenueOrderId | None,
        ts_event: int,
    ) -> None:
        """
        Queue the order canceled event using the owning clock.
        """
        self._output.order_canceled(order, venue_order_id, ts_event, self.clock.timestamp_ns())

    def generate_order_triggered(
        self,
        order: object,
        venue_order_id: VenueOrderId | None,
        ts_event: int,
    ) -> None:
        """
        Queue the order triggered event using the owning clock.
        """
        self._output.order_triggered(order, venue_order_id, ts_event, self.clock.timestamp_ns())

    def generate_order_expired(
        self,
        order: object,
        venue_order_id: VenueOrderId | None,
        ts_event: int,
    ) -> None:
        """
        Queue the order expired event using the owning clock.
        """
        self._output.order_expired(order, venue_order_id, ts_event, self.clock.timestamp_ns())

    def generate_order_filled(  # noqa: PLR0913, PLR0917 - Preserve the native client and event field contract.
        self,
        order: object,
        venue_order_id: VenueOrderId,
        venue_position_id: PositionId | None,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money | None,
        liquidity_side: LiquiditySide,
        ts_event: int,
    ) -> None:
        """
        Queue the order filled event using the owning clock.
        """
        self._output.order_filled(
            order,
            venue_order_id,
            venue_position_id,
            trade_id,
            last_qty,
            last_px,
            quote_currency,
            commission,
            liquidity_side,
            ts_event,
            self.clock.timestamp_ns(),
        )

    def _handle_order_submitted_batch(self, events: list[object]) -> None:
        self._output.order_submitted_batch(events)

    def _handle_order_accepted_batch(self, events: list[object]) -> None:
        self._output.order_accepted_batch(events)

    def _handle_order_canceled_batch(self, events: list[object]) -> None:
        self._output.order_canceled_batch(events)

    def _handles_order_venue(self, venue: Venue | None) -> bool:
        return self.venue == venue

    def _provides_bulk_position_coverage(self, instrument_id: InstrumentId) -> bool:  # noqa: ARG002 - Keep the adapter hook signature for subclasses.
        return True

    def _calculate_commission(
        self,
        instrument: object,  # noqa: ARG002 - Keep the adapter hook signature for subclasses.
        last_qty: Quantity,  # noqa: ARG002 - Keep the adapter hook signature for subclasses.
        last_px: Price,  # noqa: ARG002 - Keep the adapter hook signature for subclasses.
        liquidity_side: LiquiditySide,  # noqa: ARG002 - Keep the adapter hook signature for subclasses.
    ) -> Money | None:
        return None

    async def _register_external_order(
        self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_init: int,
    ) -> None:
        pass

    async def _on_instrument(self, instrument: object) -> None:
        pass

    async def _submit_order(self, command: SubmitOrder) -> None:
        raise NotImplementedError

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        raise NotImplementedError

    async def _modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError

    async def _cancel_order(self, command: CancelOrder) -> None:
        raise NotImplementedError

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        raise NotImplementedError

    async def _query_account(self, command: QueryAccount) -> None:
        raise NotImplementedError

    async def _query_order(self, command: QueryOrder) -> None:
        raise NotImplementedError

    async def _generate_mass_status(
        self,
        lookback_mins: int | None,  # noqa: ARG002 - Keep the adapter hook signature for subclasses.
    ) -> ExecutionMassStatus | NotImplementedType | None:
        return NotImplemented

    async def _generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        raise NotImplementedError

    async def _generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        raise NotImplementedError

    async def _generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        raise NotImplementedError

    async def _generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        raise NotImplementedError

    async def _batch_modify_orders(self, command: BatchModifyOrders) -> None:
        for modify in command.modifies:
            await self._modify_order(modify)

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        for cancel in command.cancels:
            await self._cancel_order(cancel)


def _create_client(factory: object, kwargs: dict[str, object]) -> _Client:
    create = factory.create
    if inspect.iscoroutinefunction(create):
        raise TypeError("Factory create must be synchronous")
    try:
        parameters = inspect.signature(create).parameters
    except (TypeError, ValueError):
        parameters = {}

    if "loop" in parameters:
        kwargs["loop"] = None

    client = create(**kwargs)
    if inspect.iscoroutine(client):
        client.close()
        raise TypeError("Factory create must be synchronous")
    return client
