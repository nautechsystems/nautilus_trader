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

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceEnumParser
from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEventType
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# WebSocket messages
################################################################################


class BinanceSpotUserMsgData(msgspec.Struct, frozen=True):
    """
    Inner struct for execution WebSocket messages from Binance.
    """

    e: BinanceSpotEventType


class BinanceSpotUserMsgWrapper(msgspec.Struct, frozen=True):
    """
    Provides a wrapper for execution WebSocket messages from Binance.
    """

    stream: str
    data: BinanceSpotUserMsgData


class BinanceSpotBalance(msgspec.Struct, frozen=True):
    """
    Inner struct for Binance Spot/Margin balances.
    """

    a: str  # Asset
    f: str  # Free
    l: str  # Locked

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.a)
        free = Decimal(self.f)
        locked = Decimal(self.l)
        total: Decimal = free + locked
        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )


class BinanceSpotAccountUpdateMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Spot/Margin Account Update events.
    """

    e: str  # Event Type
    E: int  # Event Time
    u: int  # Transaction Time
    B: list[BinanceSpotBalance]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.B]

    def handle_account_update(self, exec_client: BinanceCommonExecutionClient):
        """
        Handle BinanceSpotAccountUpdateMsg as payload of outboundAccountPosition.
        """
        exec_client.generate_account_state(
            balances=self.parse_to_account_balances(),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(self.u),
        )


class BinanceSpotAccountUpdateWrapper(msgspec.Struct, frozen=True):
    """
    WebSocket message wrapper for Binance Spot/Margin Account Update events.
    """

    stream: str
    data: BinanceSpotAccountUpdateMsg


class BinanceSpotOrderUpdateData(msgspec.Struct, kw_only=True):
    """
    WebSocket message 'inner struct' for Binance Spot/Margin Order Update events.
    """

    e: BinanceSpotEventType
    E: int  # Event time
    s: str  # Symbol
    c: str  # Client order ID
    S: BinanceOrderSide
    o: BinanceOrderType
    f: BinanceTimeInForce
    q: str  # Original Quantity
    p: str  # Original Price
    P: str  # Stop price
    F: str  # Iceberg quantity
    g: int  # Order list ID
    C: str  # Original client order ID; This is the ID of the order being canceled
    x: BinanceExecutionType
    X: BinanceOrderStatus
    r: str  # Order reject reason; will be an error code
    i: int  # Order ID
    l: str  # Order Last Filled Quantity
    z: str  # Order Filled Accumulated Quantity
    L: str  # Last Filled Price
    n: str | None = None  # Commission, will not push if no commission
    N: str | None = None  # Commission Asset, will not push if no commission
    T: int  # Order Trade Time
    t: int  # Trade ID
    I: int  # Ignore
    w: bool  # Is the order on the book?
    m: bool  # Is trade the maker side
    M: bool  # Ignore
    O: int  # Order creation time
    Z: str  # Cumulative quote asset transacted quantity
    Y: str  # Last quote asset transacted quantity (i.e. lastPrice * lastQty)
    Q: str  # Quote Order Qty
    W: int | None = None  # Working Time (when order was added to the book)
    V: str | None = None  # Self-Trade Prevention Mode

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: int,
        ts_init: int,
        enum_parser: BinanceEnumParser,
    ) -> OrderStatusReport:
        price = Price.from_str(self.p) if self.p is not None else None
        trigger_price = Price.from_str(self.P) if self.P is not None else None
        order_side = OrderSide.BUY if self.S == BinanceOrderSide.BUY else OrderSide.SELL
        post_only = self.f == BinanceTimeInForce.GTX
        display_qty = (
            Quantity.from_str(
                str(Decimal(self.q) - Decimal(self.F)),
            )
            if self.F is not None
            else None
        )

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=order_side,
            order_type=enum_parser.parse_binance_order_type(self.o),
            time_in_force=enum_parser.parse_binance_time_in_force(self.f),
            order_status=enum_parser.parse_binance_order_status(self.X),
            price=price,
            trigger_price=trigger_price,
            trigger_type=TriggerType.LAST_PRICE,
            trailing_offset=None,
            trailing_offset_type=TrailingOffsetType.NO_TRAILING_OFFSET,
            quantity=Quantity.from_str(self.q),
            filled_qty=Quantity.from_str(self.z),
            display_qty=display_qty,
            avg_px=None,
            post_only=post_only,
            reduce_only=False,
            report_id=UUID4(),
            ts_accepted=ts_event,
            ts_last=ts_event,
            ts_init=ts_init,
        )

    def handle_execution_report(  # noqa: C901 (too complex)
        self,
        exec_client: BinanceCommonExecutionClient,
    ):
        """
        Handle BinanceSpotOrderUpdateData as payload of executionReport event.
        """
        client_order_id_str: str = self.c
        if not client_order_id_str or not client_order_id_str.startswith("O"):
            client_order_id_str = self.C
        client_order_id = ClientOrderId(client_order_id_str or UUID4().value)
        ts_event = millis_to_nanos(self.T)
        venue_order_id = VenueOrderId(str(self.i))
        instrument_id = exec_client._get_cached_instrument_id(self.s)
        strategy_id = exec_client._cache.strategy_id_for_order(client_order_id)

        if strategy_id is None:
            report = self.parse_to_order_status_report(
                account_id=exec_client.account_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
                ts_init=exec_client._clock.timestamp_ns(),
                enum_parser=exec_client._enum_parser,
            )
            exec_client._send_order_status_report(report)
            return

        if self.x == BinanceExecutionType.NEW:
            exec_client.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )

            # Check if price changed (for price_match orders)
            order = exec_client._cache.order(client_order_id)
            if order and order.has_price:
                instrument = exec_client._instrument_provider.find(instrument_id=instrument_id)
                if instrument is not None:
                    price_precision = instrument.price_precision
                    binance_price = Price(float(self.p), price_precision)
                    if binance_price != order.price:
                        # Preserve trigger price for stop orders (priceMatch only affects limit price)
                        trigger_price = order.trigger_price if order.has_trigger_price else None
                        exec_client.generate_order_updated(
                            strategy_id=strategy_id,
                            instrument_id=instrument_id,
                            client_order_id=client_order_id,
                            venue_order_id=venue_order_id,
                            quantity=order.quantity,
                            price=binance_price,
                            trigger_price=trigger_price,
                            ts_event=ts_event,
                        )
        elif self.x == BinanceExecutionType.TRADE or self.x == BinanceExecutionType.CALCULATED:
            if self.x == BinanceExecutionType.CALCULATED:
                exec_client._log.info(
                    f"Received CALCULATED (liquidation) execution for order {venue_order_id}, "
                    f"generating OrderFilled event",
                )

            if Decimal(self.L) == 0:
                exec_client._log.warning(
                    f"Received {self.x.value} execution with L=0 for order {venue_order_id}, "
                    f"order status={self.X.value}",
                )

                # Route based on order status to ensure terminal events are generated
                if self.X == BinanceOrderStatus.EXPIRED:
                    exec_client.generate_order_expired(
                        strategy_id=strategy_id,
                        instrument_id=instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        ts_event=ts_event,
                    )
                    return
                elif self.X == BinanceOrderStatus.CANCELED:
                    exec_client.generate_order_canceled(
                        strategy_id=strategy_id,
                        instrument_id=instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        ts_event=ts_event,
                    )
                    return
                elif self.X in (BinanceOrderStatus.FILLED, BinanceOrderStatus.PARTIALLY_FILLED):
                    # Continue to generate fill with L=0 to close order
                    # Better to have bad price data than stuck order
                    exec_client._log.warning(
                        f"Generating OrderFilled with L=0 for terminal state {self.X.value} "
                        f"to prevent order from being stuck",
                    )
                else:
                    # Non-terminal status with L=0, skip fill generation
                    return

            instrument = exec_client._instrument_provider.find(instrument_id=instrument_id)
            if instrument is None:
                raise ValueError(
                    f"Cannot process fill for {instrument_id}: instrument not found in cache",
                )

            # Determine commission
            commission_asset = self.N
            commission_amount = self.n
            if commission_asset is not None:
                commission = Money.from_str(f"{commission_amount} {commission_asset}")
            else:
                # Binance typically charges commission as base asset or BNB
                commission = Money(0, instrument.base_currency)

            # Liquidations are always taker, regular trades use the 'm' field
            liquidity_side = (
                LiquiditySide.TAKER
                if self.x == BinanceExecutionType.CALCULATED
                else (LiquiditySide.MAKER if self.m else LiquiditySide.TAKER)
            )

            exec_client.generate_order_filled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                venue_position_id=None,  # NETTING accounts
                trade_id=TradeId(str(self.t)),  # Trade ID
                order_side=exec_client._enum_parser.parse_binance_order_side(self.S),
                order_type=exec_client._enum_parser.parse_binance_order_type(self.o),
                last_qty=Quantity.from_str(self.l),
                last_px=Price.from_str(self.L),
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=liquidity_side,
                ts_event=ts_event,
            )
        elif self.x == BinanceExecutionType.CANCELED:
            exec_client.generate_order_canceled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif self.x == BinanceExecutionType.REJECTED:
            # A rejection can occur for many reasons, but most commonly for
            # POST-ONLY (GTX) orders that would immediately take liquidity. We
            # flag these specifically so downstream components can distinguish
            # between generic rejections and those due to the post-only
            # constraint.
            due_post_only = self.f == BinanceTimeInForce.GTX

            exec_client.generate_order_rejected(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                reason=str(self.r),
                ts_event=ts_event,
                due_post_only=due_post_only,
            )
        elif self.x == BinanceExecutionType.EXPIRED:
            exec_client.generate_order_expired(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif self.x == BinanceExecutionType.TRADE_PREVENTION:
            # Self-trade prevention triggered - no actual trade occurred
            exec_client._log.info(
                f"Self-trade prevention triggered for order {venue_order_id}, "
                f"prevented qty={self.l} at price={self.L}",
            )
        else:
            # Event not handled
            exec_client._log.warning(f"Received unhandled {self}")


class BinanceSpotOrderUpdateWrapper(msgspec.Struct, frozen=True):
    """
    WebSocket message wrapper for Binance Spot/Margin Order Update events.
    """

    stream: str
    data: BinanceSpotOrderUpdateData
