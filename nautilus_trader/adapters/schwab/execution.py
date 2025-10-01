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
from __future__ import annotations

import asyncio
from collections.abc import Mapping
from decimal import Decimal
from typing import Any

import pandas as pd
from schwab.orders.common import Duration as SchwabDuration
from schwab.orders.common import EquityInstruction
from schwab.orders.common import OrderStrategyType
from schwab.orders.common import OrderType as SchwabOrderType
from schwab.orders.common import PriceLinkBasis
from schwab.orders.common import PriceLinkType
from schwab.orders.common import Session as SchwabSession
from schwab.orders.generic import OrderBuilder

from nautilus_trader.adapters.schwab.common import SCHWAB_OPTION_VENUE
from nautilus_trader.adapters.schwab.common import SCHWAB_VENUE
from nautilus_trader.adapters.schwab.config import SchwabExecClientConfig
from nautilus_trader.adapters.schwab.http.client import SchwabHttpClient
from nautilus_trader.adapters.schwab.http.error import SchwabError
from nautilus_trader.adapters.schwab.http.error import should_retry
from nautilus_trader.adapters.schwab.providers import SchwabInstrumentProvider
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.model.orders import TrailingStopLimitOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder


ORDER_TYPE_MAP = {
    OrderType.MARKET: SchwabOrderType.MARKET,
    OrderType.LIMIT: SchwabOrderType.LIMIT,
    OrderType.STOP_MARKET: SchwabOrderType.STOP,
    OrderType.STOP_LIMIT: SchwabOrderType.STOP_LIMIT,
    OrderType.TRAILING_STOP_MARKET: SchwabOrderType.TRAILING_STOP,
    OrderType.TRAILING_STOP_LIMIT: SchwabOrderType.TRAILING_STOP_LIMIT,
}

ORDER_TYPE_REVERSE = {value: key for key, value in ORDER_TYPE_MAP.items()}

TIME_IN_FORCE_MAP = {
    TimeInForce.DAY: SchwabDuration.DAY,
    TimeInForce.GTC: SchwabDuration.GOOD_TILL_CANCEL,
    TimeInForce.IOC: SchwabDuration.IMMEDIATE_OR_CANCEL,
    TimeInForce.FOK: SchwabDuration.FILL_OR_KILL,
}

TIME_IN_FORCE_REVERSE = {value: key for key, value in TIME_IN_FORCE_MAP.items()}

ORDER_ACTION_MAP = {
    OrderSide.BUY: EquityInstruction.BUY,
    OrderSide.SELL: EquityInstruction.SELL,
}

TRAILING_OFFSET_TYPE_MAP = {
    TrailingOffsetType.PRICE: PriceLinkType.VALUE,
    TrailingOffsetType.BASIS_POINTS: PriceLinkType.PERCENT,
    TrailingOffsetType.TICKS: PriceLinkType.TICK,
}

TRAILING_OFFSET_TYPE_REVERSE = {value: key for key, value in TRAILING_OFFSET_TYPE_MAP.items()}

# TODO: schwab has no specific default here, need to double check
TRIGGER_TYPE_MAP = {
    TriggerType.LAST_PRICE: PriceLinkBasis.LAST,
    TriggerType.BID_ASK: PriceLinkBasis.ASK_BID,
    TriggerType.MARK_PRICE: PriceLinkBasis.MARK,
    TriggerType.MID_POINT: PriceLinkBasis.AVERAGE,
    TriggerType.DEFAULT: PriceLinkBasis.LAST,
}

SCHWAB_STATUS_MAP = {
    "ACCEPTED": OrderStatus.INITIALIZED,
    "WORKING": OrderStatus.SUBMITTED,
    "QUEUED": OrderStatus.SUBMITTED,
    "PENDING_ACTIVATION": OrderStatus.SUBMITTED,
    "FILLED": OrderStatus.FILLED,
    "CANCELED": OrderStatus.CANCELED,
    "REJECTED": OrderStatus.REJECTED,
    "EXPIRED": OrderStatus.EXPIRED,
}


class SchwabExecutionClient(LiveExecutionClient):
    """
    Execution client for Schwab brokerage accounts.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        http_client: SchwabHttpClient,
        instrument_provider: SchwabInstrumentProvider,
        config: SchwabExecClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or f"{SCHWAB_VENUE.value}-EXEC"),
            venue=SCHWAB_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._http_client = http_client
        self._config = config
        self._client_order_to_venue: dict[ClientOrderId, VenueOrderId] = {}
        self._client_order_to_instrument: dict[ClientOrderId, InstrumentId] = {}
        self._open_orders: dict[VenueOrderId, Mapping[str, Any]] = {}
        self._account_hash: str | None = None

        account_id_value = config.account_number
        if account_id_value:
            self._set_account_id(
                AccountId(f"{SCHWAB_VENUE.value}-{account_id_value}"),
            )
        else:
            self._set_account_id(AccountId(f"{SCHWAB_VENUE.value}-001"))

        self._retry_manager_pool = RetryManagerPool[None](
            pool_size=100,
            max_retries=config.max_retries or 0,
            delay_initial_ms=config.retry_delay_initial_ms or 1_000,
            delay_max_ms=config.retry_delay_max_ms or 10_000,
            backoff_factor=2,
            logger=self._log,
            exc_types=(SchwabError,),
            retry_check=should_retry,
        )

        self._submit_order_methods = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
            OrderType.STOP_MARKET: self._submit_stop_market_order,
            OrderType.STOP_LIMIT: self._submit_stop_limit_order,
            OrderType.TRAILING_STOP_MARKET: self._submit_trailing_stop_market_order,
            OrderType.TRAILING_STOP_LIMIT: self._submit_trailing_stop_limit_order,
        }

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._update_account_state()

    async def _update_account_state(self) -> None:
        if self._account_hash is None:
            account_hashmap = await self._http_client.get_account_numbers()
            self._account_hash = account_hashmap[self._config.account_number]

        balances, margins = await self._http_client.get_account(
            self._account_hash,
            self.base_currency,
        )
        self.generate_account_state(
            balances=balances,
            margins=margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _disconnect(self) -> None:
        self._log.info("Schwab execution client disconnected", LogColor.BLUE)

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        instrument = self._cache.instrument(order.instrument_id)

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            await retry_manager.run(
                "submit_order",
                [order.client_order_id],
                self._submit_order_methods[order.order_type],
                order,
            )
            if not retry_manager.result:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=retry_manager.message,
                    ts_event=self._clock.timestamp_ns(),
                )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def _submit_and_check_order(self, order: Order, order_spec: Mapping[str, Any]) -> None:
        if order.is_post_only:
            raise ValueError("`post_only` not supported by Schwab")
        venue_order_id = await self._http_client.place_order(self._account_hash, order_spec)
        if venue_order_id:
            order_status = await self._http_client.get_order(venue_order_id, self._account_hash)
            if order_status["status"] in ["WORKING", "PENDING_ACTIVATION"]:
                self.generate_order_accepted(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=VenueOrderId(venue_order_id),
                    ts_event=self._clock.timestamp_ns(),
                )
            elif order_status["status"] == "REJECTED":
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=order_status["statusDescription"],
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        schwab_order = (
            OrderBuilder()
            .set_order_type(ORDER_TYPE_MAP[order.order_type])
            .set_price(str(order.price))
            .set_session(SchwabSession.NORMAL)
            .set_duration(TIME_IN_FORCE_MAP[order.time_in_force])
            .set_order_strategy_type(OrderStrategyType.SINGLE)
            .add_equity_leg(
                ORDER_ACTION_MAP[order.side],
                order.instrument_id.symbol.value,
                int(
                    order.quantity,
                ),
            )
            .build()
        )
        await self._submit_and_check_order(order, schwab_order)

    async def _submit_market_order(self, order: MarketOrder) -> None:
        schwab_order = (
            OrderBuilder()
            .set_order_type(ORDER_TYPE_MAP[order.order_type])
            .set_session(SchwabSession.NORMAL)
            .set_duration(TIME_IN_FORCE_MAP[order.time_in_force])
            .set_order_strategy_type(OrderStrategyType.SINGLE)
            .add_equity_leg(
                ORDER_ACTION_MAP[order.side],
                order.instrument_id.symbol.value,
                int(
                    order.quantity,
                ),
            )
            .build()
        )
        await self._submit_and_check_order(order, schwab_order)

    async def _submit_stop_market_order(self, order: StopMarketOrder) -> None:
        schwab_order = (
            OrderBuilder()
            .set_order_type(ORDER_TYPE_MAP[order.order_type])
            .set_session(SchwabSession.NORMAL)
            .set_duration(TIME_IN_FORCE_MAP[order.time_in_force])
            .set_stop_price_link_basis(TRIGGER_TYPE_MAP[order.trigger_type])
            .set_stop_price(str(order.trigger_price))
            .set_order_strategy_type(OrderStrategyType.SINGLE)
            .add_equity_leg(
                ORDER_ACTION_MAP[order.side],
                order.instrument_id.symbol.value,
                int(
                    order.quantity,
                ),
            )
            .build()
        )
        await self._submit_and_check_order(order, schwab_order)

    async def _submit_stop_limit_order(self, order: StopMarketOrder) -> None:
        schwab_order = (
            OrderBuilder()
            .set_order_type(ORDER_TYPE_MAP[order.order_type])
            .set_session(SchwabSession.NORMAL)
            .set_duration(TIME_IN_FORCE_MAP[order.time_in_force])
            .set_price(str(order.price))
            .set_stop_price_link_basis(TRIGGER_TYPE_MAP[order.trigger_type])
            .set_stop_price(str(order.trigger_price))
            .set_order_strategy_type(OrderStrategyType.SINGLE)
            .add_equity_leg(
                ORDER_ACTION_MAP[order.side],
                order.instrument_id.symbol.value,
                int(
                    order.quantity,
                ),
            )
            .build()
        )
        await self._submit_and_check_order(order, schwab_order)

    async def _submit_trailing_stop_market_order(self, order: TrailingStopMarketOrder) -> None:
        schwab_order = (
            OrderBuilder()
            .set_order_type(ORDER_TYPE_MAP[order.order_type])
            .set_session(SchwabSession.NORMAL)
            .set_duration(TIME_IN_FORCE_MAP[order.time_in_force])
            .set_activation_price(str(order.trigger_price))
            .set_stop_price_link_basis(TRIGGER_TYPE_MAP[order.trigger_type])
            .set_stop_price_link_type(TRAILING_OFFSET_TYPE_MAP[order.trailing_offset_type])
            .set_stop_price_offset(float(order.trailing_offset))
            .set_order_strategy_type(OrderStrategyType.SINGLE)
            .add_equity_leg(
                ORDER_ACTION_MAP[order.side],
                order.instrument_id.symbol.value,
                int(
                    order.quantity,
                ),
            )
            .build()
        )
        await self._submit_and_check_order(order, schwab_order)

    async def _submit_trailing_stop_limit_order(self, order: TrailingStopLimitOrder) -> None:
        schwab_order = (
            OrderBuilder()
            .set_order_type(ORDER_TYPE_MAP[order.order_type])
            .set_session(SchwabSession.NORMAL)
            .set_duration(TIME_IN_FORCE_MAP[order.time_in_force])
            .set_activation_price(str(order.trigger_price))
            .set_price(str(order.price))
            .set_stop_price_link_basis(TRIGGER_TYPE_MAP[order.trigger_type])
            .set_stop_price_link_type(TRAILING_OFFSET_TYPE_MAP[order.trailing_offset_type])
            .set_stop_price_offset(float(order.trailing_offset))
            .set_order_strategy_type(OrderStrategyType.SINGLE)
            .add_equity_leg(
                ORDER_ACTION_MAP[order.side],
                order.instrument_id.symbol.value,
                int(
                    order.quantity,
                ),
            )
            .build()
        )
        await self._submit_and_check_order(order, schwab_order)

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.warning("Order list submission not supported for Schwab")

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.warning(
            "Order modification not supported for Schwab, the replace_order endpoint will cancel the old order and create a new one",
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`CancelOrder` command for {command.client_order_id!r} when order already {
                    order.status_string()
                } (will not send to exchange)",
            )
            return

        client_order_id = command.client_order_id.value
        venue_order_id = (
            str(
                command.venue_order_id,
            )
            if command.venue_order_id
            else None
        )

        if venue_order_id is None:
            self._log.error(
                f"Unable to cancel {
                    command.client_order_id
                }: missing venue order id",
            )
            return

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            response = await retry_manager.run(
                "cancel_order",
                [client_order_id, venue_order_id],
                self._http_client.cancel_order,
                order_id=venue_order_id,
                account_hash=self._account_hash,
            )
            if not retry_manager.result:
                self.generate_order_cancel_rejected(
                    order.strategy_id,
                    order.instrument_id,
                    order.client_order_id,
                    order.venue_order_id,
                    retry_manager.message,
                    self._clock.timestamp_ns(),
                )

            if response:
                ret_code = response.status_code
                if ret_code != 0:
                    if ret_code == 200:
                        self.generate_order_canceled(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=order.venue_order_id,
                            ts_event=self._clock.timestamp_ns(),
                        )
                    else:
                        self.generate_order_cancel_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=order.venue_order_id,
                            reason=response.json()["message"],
                            ts_event=self._clock.timestamp_ns(),
                        )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        cancels = command.cancels if hasattr(command, "cancels") else []
        for cancel in cancels:
            await self._cancel_order(cancel)

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        open_orders: list[Order] = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )

        # TODO: A future improvement could be to asyncio.gather all cancel tasks
        for order in open_orders:
            retry_manager = await self._retry_manager_pool.acquire()
            try:
                response = await retry_manager.run(
                    "cancel_order",
                    [order.client_order_id, order.venue_order_id],
                    self._http_client.cancel_order,
                    order_id=order.venue_order_id,
                    account_hash=self._account_hash,
                )
                if not retry_manager.result:
                    self.generate_order_cancel_rejected(
                        order.strategy_id,
                        order.instrument_id,
                        order.client_order_id,
                        order.venue_order_id,
                        retry_manager.message,
                        self._clock.timestamp_ns(),
                    )
                if response:
                    ret_code = response.status_code
                    if ret_code != 0:
                        if ret_code == 200:
                            self.generate_order_canceled(
                                strategy_id=order.strategy_id,
                                instrument_id=order.instrument_id,
                                client_order_id=order.client_order_id,
                                venue_order_id=order.venue_order_id,
                                ts_event=self._clock.timestamp_ns(),
                            )
                        else:
                            self.generate_order_cancel_rejected(
                                strategy_id=order.strategy_id,
                                instrument_id=order.instrument_id,
                                client_order_id=order.client_order_id,
                                venue_order_id=order.venue_order_id,
                                reason=response.json()["message"],
                                ts_event=self._clock.timestamp_ns(),
                            )
            finally:
                await self._retry_manager_pool.release(retry_manager)

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        instrument_id = command.instrument_id
        client_order_id = command.client_order_id
        venue_order_id = command.venue_order_id

        if venue_order_id:
            order_data = await self._http_client.get_order(venue_order_id, self._account_hash)

        report = self._build_order_status_report(
            order_data,
            instrument_id,
        )
        return report

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        try:
            orders = await self._http_client.get_orders_for_account(
                account_hash=self._account_hash,
                from_entered_datetime=command.start,
                to_entered_datetime=command.end,
            )
        except Exception as exc:
            self._log.exception("Failed to list orders", exc)
            return []

        reports: list[OrderStatusReport] = []
        for order_data in orders:
            # Apply filtering from command
            if command.instrument_id:
                instrument_id = self._infer_instrument_id(order_data)
                if instrument_id != command.instrument_id:
                    continue

            status = SCHWAB_STATUS_MAP.get(
                order_data.get("status", "").upper(),
            )
            if command.open_only and status not in (OrderStatus.SUBMITTED, OrderStatus.ACCEPTED):
                continue

            entered_time_str = order_data.get("enteredTime")
            if entered_time_str:
                # Timestamps from Schwab are like '2024-02-07T18:04:59+0000'
                entered_time = pd.to_datetime(entered_time_str).to_pydatetime()
                if command.start and entered_time < command.start:
                    continue
                if command.end and entered_time > command.end:
                    continue

            venue_order_id = order_data.get("orderId", None)
            instrument_id = self._infer_instrument_id(order_data)
            report = self._build_order_status_report(
                order_data,
                instrument_id,
            )
            if report:
                reports.append(report)
        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        return []
        # order_data = await self._fetch_order_snapshot(
        #     command.client_order_id,
        #     command.venue_order_id,
        # )
        # if not order_data:
        #     return []
        #
        # venue_order_id = self._resolve_venue_order_id(
        #     command.client_order_id,
        #     command.venue_order_id,
        # )
        # instrument_id = (
        #     command.instrument_id
        #     or self._client_order_to_instrument.get(command.client_order_id)
        #     or self._infer_instrument_id(order_data)
        # )
        # instrument = (
        #     self._cache.instrument(
        #         instrument_id,
        #     )
        #     if instrument_id
        #     else None
        # )
        # if instrument is None:
        #     return []
        #
        # fills = self._extract_fills(order_data)
        # reports: list[FillReport] = []
        # for fill in fills:
        #     report = self._build_fill_report(
        #         fill,
        #         instrument=instrument,
        #         instrument_id=instrument_id,
        #         venue_order_id=venue_order_id,
        #         client_order_id=command.client_order_id,
        #     )
        #     reports.append(report)
        # return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        return []

    # -- Helpers ---------------------------------------------------------------------------------

    # def _resolve_venue_order_id(
    #     self,
    #     client_order_id: ClientOrderId | None,
    #     venue_order_id: VenueOrderId | None,
    # ) -> VenueOrderId | None:
    #     if venue_order_id:
    #         return venue_order_id
    #     if client_order_id:
    #         return self._client_order_to_venue.get(client_order_id)
    #     return None
    #
    # async def _fetch_order_snapshot(
    #     self,
    #     client_order_id: ClientOrderId | None,
    #     venue_order_id: VenueOrderId | None,
    # ) -> Mapping[str, Any] | None:
    #     resolved = self._resolve_venue_order_id(
    #         client_order_id,
    #         venue_order_id,
    #     )
    #     if resolved is None:
    #         return None
    #     if resolved in self._open_orders:
    #         return self._open_orders[resolved]
    #     try:
    #         order_data = await self._http_client.get_order(resolved.value, self.account_id.get_id())
    #         self._open_orders[resolved] = order_data
    #         return order_data
    #     except Exception as exc:
    #         self._log.exception(f"Failed to load order {resolved.value}", exc)
    #         return None

    def _build_order_status_report(
        self,
        order_data: Mapping[str, Any],
        instrument_id: InstrumentId,
    ) -> OrderStatusReport | None:
        order_list_id = None
        contingency_type = ContingencyType.NO_CONTINGENCY
        venue_order_id = VenueOrderId(str(order_data.get("orderId", "")))
        client_order_id = self._cache.client_order_id(venue_order_id)
        status = SCHWAB_STATUS_MAP.get(
            order_data.get(
                "status",
                "",
            ).upper(),
            OrderStatus.ACCEPTED,
        )
        raw_type = str(order_data.get("orderType", "")).upper()
        order_type_enum = ORDER_TYPE_REVERSE.get(SchwabOrderType(raw_type))
        tif_value = str(
            order_data.get(
                "duration",
                self._config.default_duration,
            ),
        )
        time_in_force = TIME_IN_FORCE_REVERSE.get(
            SchwabDuration(tif_value),
            TimeInForce.DAY,
        )
        if order_type_enum in (OrderType.TRAILING_STOP_MARKET, OrderType.TRAILING_STOP_LIMIT):
            # TODO: need to refine here
            self._log.warning("Trailing stop orders not supported for now!")
            return None
            # trigger_price = Decimal(self.triggerPrice)
            # last_price = Decimal(self.lastPriceOnCreated)
            trailing_offset = order_data.get("priceOffset")
            trailing_offset_type = TRAILING_OFFSET_TYPE_REVERSE.get(
                PriceLinkType(order_data.get("priceLinkType")),
                TrailingOffsetType.NO_TRAILING_OFFSET,
            )
        else:
            trailing_offset = None
            trailing_offset_type = TrailingOffsetType.NO_TRAILING_OFFSET
        stop_price = order_data.get("stopPrice", None)
        if stop_price:
            stop_price = Price.from_str(str(stop_price))
        stop_type = order_data.get("stopType", None)
        if stop_type == "STANDARD":
            stop_type = TriggerType.DEFAULT
        elif stop_type == "LAST":
            stop_type = TriggerType.LAST_PRICE
        else:
            stop_type = TriggerType.NO_TRIGGER

        total_qty = float(order_data.get("quantity", 0.0))
        filled_qty = float(order_data.get("filledQuantity", 0.0))
        limit_price = order_data.get("price")
        if limit_price:
            limit_price = Price.from_str(str(limit_price))

        if status == OrderStatus.FILLED:
            avg_price = Decimal(self._parse_avg_price(order_data))
        else:
            avg_price = Decimal()

        ts_init = self._clock.timestamp_ns()

        ts_accepted = ts_init
        entered_time_str = order_data.get("enteredTime")
        if entered_time_str:
            ts_accepted = int(
                pd.to_datetime(
                    entered_time_str,
                ).timestamp()
                * 1e9,
            )

        ts_last = ts_init
        close_time_str = order_data.get("closeTime")
        if close_time_str:
            ts_last = int(pd.to_datetime(close_time_str).timestamp() * 1e9)

        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            venue_order_id=venue_order_id,
            client_order_id=client_order_id,
            order_side=self._parse_order_side(order_data),
            order_type=order_type_enum,
            time_in_force=time_in_force,
            order_status=status,
            quantity=Quantity.from_str(str(total_qty)),
            filled_qty=Quantity.from_str(str(filled_qty)),
            avg_px=avg_price,
            report_id=UUID4(),
            ts_accepted=ts_accepted,
            ts_last=ts_last,
            ts_init=ts_init,
            price=limit_price,
            trigger_price=stop_price,
            trigger_type=stop_type,
        )
        return report

    def _parse_avg_price(self, order_data: Mapping[str, Any]) -> float:
        # TODO: need to support options and orders that are filled in separate times
        return order_data.get("orderActivityCollection").get("executionLegs")[0].get("price")

    def _parse_order_side(self, order_data: Mapping[str, Any]) -> OrderSide:
        legs = order_data.get("orderLegCollection")
        if isinstance(legs, list) and legs:
            instruction = str(legs[0].get("instruction", "")).upper()
            if "SELL" in instruction:
                return OrderSide.SELL
        return OrderSide.BUY

    def _extract_fills(self, order_data: Mapping[str, Any]) -> list[Mapping[str, Any]]:
        activities = order_data.get("orderActivityCollection", [])
        fills: list[Mapping[str, Any]] = []
        if isinstance(activities, list):
            for activity in activities:
                execution_legs = (
                    activity.get("executionLegs")
                    if isinstance(
                        activity,
                        Mapping,
                    )
                    else None
                )
                if isinstance(execution_legs, list):
                    fills.extend(
                        [
                            leg
                            for leg in execution_legs
                            if isinstance(
                                leg,
                                Mapping,
                            )
                        ],
                    )
        return fills

    # def _build_fill_report(
    #     self,
    #     fill: Mapping[str, Any],
    #     *,
    #     instrument: Instrument,
    #     instrument_id: InstrumentId,
    #     venue_order_id: VenueOrderId | None,
    #     client_order_id: ClientOrderId,
    # ) -> FillReport:
    #     qty = float(fill.get("quantity", 0.0))
    #     price = float(fill.get("price", 0.0))
    #     ts = fill.get("time")
    #     ts_event = self._clock.timestamp_ns()
    #     if isinstance(ts, int | float):
    #         ts_event = self._coerce_time_number(ts)
    #
    #     return FillReport(
    #         client_order_id=client_order_id,
    #         instrument_id=instrument_id,
    #         account_id=self.account_id,
    #         venue_order_id=venue_order_id,
    #         venue_position_id=None,
    #         order_side=self._parse_order_side({"orderLegCollection": [fill]}),
    #         trade_id=TradeId(str(fill.get("executionId", UUID4()))),
    #         last_qty=instrument.make_qty(qty),
    #         last_px=instrument.make_price(price),
    #         commission=Money(
    #             0.0,
    #             (
    #                 instrument.currency
    #                 if hasattr(instrument, "currency")
    #                 else Currency.from_str("USD")
    #             ),
    #         ),
    #         liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
    #         report_id=UUID4(),
    #         ts_event=ts_event,
    #         ts_init=ts_event,
    #     )
    #
    # def _build_position_report(
    #     self,
    #     position: Mapping[str, Any],
    #     ts: int,
    #     requested_instrument: InstrumentId | None,
    # ) -> PositionStatusReport | None:
    #     instrument_info = (
    #         position.get("instrument")
    #         if isinstance(
    #             position,
    #             Mapping,
    #         )
    #         else None
    #     )
    #     if not isinstance(instrument_info, Mapping):
    #         return None
    #
    #     symbol = instrument_info.get("symbol")
    #     asset_type = instrument_info.get("assetType", "EQUITY")
    #     if not symbol:
    #         return None
    #
    #     instrument_id = self._instrument_id_from_symbol(symbol, asset_type)
    #     if requested_instrument and instrument_id != requested_instrument:
    #         return None
    #
    #     instrument = self._cache.instrument(instrument_id)
    #     if instrument is None:
    #         return None
    #
    #     long_qty = float(position.get("longQuantity", 0.0))
    #     short_qty = float(position.get("shortQuantity", 0.0))
    #     net_qty = long_qty - short_qty
    #
    #     if net_qty > 0:
    #         side = PositionSide.LONG
    #         qty = instrument.make_qty(net_qty)
    #     elif net_qty < 0:
    #         side = PositionSide.SHORT
    #         qty = instrument.make_qty(abs(net_qty))
    #     else:
    #         side = PositionSide.FLAT
    #         qty = instrument.make_qty(0.0)
    #
    #     return PositionStatusReport(
    #         account_id=self.account_id,
    #         instrument_id=instrument_id,
    #         position_side=side,
    #         quantity=qty,
    #         report_id=UUID4(),
    #         ts_last=ts,
    #         ts_init=ts,
    #     )

    def _instrument_id_from_symbol(self, symbol: str, asset_type: str) -> InstrumentId:
        venue = None
        if asset_type.upper() == "OPTION":
            option_exchange = SCHWAB_OPTION_VENUE.value
            provider_config = getattr(
                self._config.instrument_provider,
                "option_exchange",
                None,
            )
            if isinstance(provider_config, str) and provider_config:
                option_exchange = provider_config
            venue = option_exchange
        else:
            # TODO: should not be hardcoded
            venue = "XNAS"
        return InstrumentId.from_str(f"{symbol}.{venue}")

    def _infer_instrument_id(self, order_data: Mapping[str, Any]) -> InstrumentId | None:
        legs = order_data.get("orderLegCollection")
        if not isinstance(legs, list) or not legs:
            return None
        instrument_payload = legs[0].get("instrument")
        if not isinstance(instrument_payload, Mapping):
            return None
        symbol = instrument_payload.get("symbol")
        asset_type = instrument_payload.get("assetType", "EQUITY")
        if not symbol:
            return None
        return self._instrument_id_from_symbol(symbol, asset_type)

    # def _coerce_time_number(self, value: float) -> int:
    #     value_int = int(value)
    #     if value_int > 10**18:
    #         return value_int
    #     if value_int > 10**12:
    #         return value_int * 1_000_000
    #     if value_int > 10**9:
    #         return value_int * 1_000
    #     return value_int * 1_000_000_000


__all__ = ["SchwabExecutionClient"]
