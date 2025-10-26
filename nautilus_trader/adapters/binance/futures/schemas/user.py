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
from nautilus_trader.adapters.binance.common.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEventType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionUpdateReason
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesWorkingType
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
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


class BinanceFuturesUserMsgData(msgspec.Struct, frozen=True):
    """
    Inner struct for execution WebSocket messages from Binance.
    """

    e: BinanceFuturesEventType


class BinanceFuturesUserMsgWrapper(msgspec.Struct, frozen=True):
    """
    Provides a wrapper for execution WebSocket messages from Binance.
    """

    data: BinanceFuturesUserMsgData | None = None
    stream: str | None = None


class MarginCallPosition(msgspec.Struct, frozen=True):
    """
    Inner struct position for Binance Futures Margin Call events.
    """

    s: str  # Symbol
    ps: BinanceFuturesPositionSide  # Position Side
    pa: str  # Position  Amount
    mt: str  # Margin Type
    iw: str  # Isolated Wallet(if isolated position)
    mp: str  # MarkPrice
    up: str  # Unrealized PnL
    mm: str  # Maintenance Margin Required


class BinanceFuturesMarginCallMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Futures Margin Call events.
    """

    e: str  # Event Type
    E: int  # Event Time
    cw: float  # Cross Wallet Balance. Only pushed with crossed position margin call
    p: list[MarginCallPosition]


class BinanceFuturesBalance(msgspec.Struct, frozen=True):
    """
    Inner struct balance for Binance Futures Balance and Position update event.
    """

    a: str  # Asset
    wb: str  # Wallet Balance
    cw: str  # Cross Wallet Balance
    bc: str  # Balance Change except PnL and Commission

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.a)
        free = Decimal(self.wb)
        locked = Decimal(0)  # TODO: Pending refactoring of accounting
        total: Decimal = free + locked

        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )


class BinanceFuturesPosition(msgspec.Struct, frozen=True):
    """
    Inner struct position for Binance Futures Balance and Position update event.
    """

    s: str  # Symbol
    pa: str  # Position amount
    ep: str  # Entry price
    cr: str  # (Pre-free) Accumulated Realized
    up: str  # Unrealized PnL
    mt: str  # Margin type
    iw: str  # Isolated wallet
    ps: BinanceFuturesPositionSide


class BinanceFuturesAccountUpdateData(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Futures Balance and Position Update events.
    """

    m: BinanceFuturesPositionUpdateReason
    B: list[BinanceFuturesBalance]
    P: list[BinanceFuturesPosition]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.B]


class BinanceFuturesAccountUpdateMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Futures Balance and Position Update events.
    """

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time
    a: BinanceFuturesAccountUpdateData

    def handle_account_update(self, exec_client: BinanceCommonExecutionClient):
        """
        Handle BinanceFuturesAccountUpdateMsg as payload of ACCOUNT_UPDATE.
        """
        exec_client.generate_account_state(
            balances=self.a.parse_to_account_balances(),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(self.T),
        )


class BinanceFuturesAccountUpdateWrapper(msgspec.Struct, frozen=True):
    """
    WebSocket message wrapper for Binance Futures Balance and Position Update events.
    """

    stream: str
    data: BinanceFuturesAccountUpdateMsg


class BinanceFuturesOrderData(msgspec.Struct, kw_only=True, frozen=True):
    """
    WebSocket message 'inner struct' for Binance Futures Order Update events.

    Client Order ID 'c':
     - starts with "autoclose-": liquidation order/
     - starts with "adl_autoclose": ADL auto close order/

    """

    s: str  # Symbol
    c: str  # Client Order ID
    S: BinanceOrderSide
    o: BinanceOrderType
    f: BinanceTimeInForce
    q: str  # Original Quantity
    p: str  # Original Price
    ap: str  # Average Price
    sp: str | None = None  # Stop Price. Ignore with TRAILING_STOP_MARKET order
    x: BinanceExecutionType
    X: BinanceOrderStatus
    i: int  # Order ID
    l: str  # Order Last Filled Quantity
    z: str  # Order Filled Accumulated Quantity
    L: str  # Last Filled Price
    N: str | None = None  # Commission Asset, will not push if no commission
    n: str | None = None  # Commission, will not push if no commission
    T: int  # Order Trade Time
    t: int  # Trade ID
    b: str  # Bids Notional
    a: str  # Ask Notional
    m: bool  # Is trade the maker side
    R: bool  # Is reduce only
    wt: BinanceFuturesWorkingType
    ot: BinanceOrderType
    ps: BinanceFuturesPositionSide
    cp: bool | None = None  # If Close-All, pushed with conditional order
    AP: str | None = None  # Activation Price, only pushed with TRAILING_STOP_MARKET order
    cr: str | None = None  # Callback Rate, only pushed with TRAILING_STOP_MARKET order
    pP: bool  # ignore
    si: int  # ignore
    ss: int  # ignore
    rp: str  # Realized Profit of the trade
    gtd: int  # TIF GTD order auto cancel time
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
        trigger_price = Price.from_str(self.sp) if self.sp is not None else None
        trailing_offset = Decimal(self.cr) * 100 if self.cr is not None else None
        order_side = OrderSide.BUY if self.S == BinanceOrderSide.BUY else OrderSide.SELL
        post_only = self.f == BinanceTimeInForce.GTX
        expire_time = unix_nanos_to_dt(millis_to_nanos(self.gtd)) if self.gtd else None

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=order_side,
            order_type=enum_parser.parse_binance_order_type(self.o),
            time_in_force=enum_parser.parse_binance_time_in_force(self.f),
            order_status=enum_parser.parse_binance_order_status(self.X),
            expire_time=expire_time,
            price=price,
            trigger_price=trigger_price,
            trigger_type=enum_parser.parse_binance_trigger_type(self.wt.value),
            trailing_offset=trailing_offset,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            quantity=Quantity.from_str(self.q),
            filled_qty=Quantity.from_str(self.z),
            avg_px=None,
            post_only=post_only,
            reduce_only=self.R,
            report_id=UUID4(),
            ts_accepted=ts_event,
            ts_last=ts_event,
            ts_init=ts_init,
        )

    def handle_order_trade_update(  # noqa: C901 (too complex)
        self,
        exec_client: BinanceCommonExecutionClient,
    ) -> None:
        """
        Handle BinanceFuturesOrderData as payload of ORDER_TRADE_UPDATE event.
        """
        client_order_id = ClientOrderId(self.c) if self.c != "" else None
        ts_event = millis_to_nanos(self.T)
        venue_order_id = VenueOrderId(str(self.i))
        instrument_id = exec_client._get_cached_instrument_id(self.s)
        strategy_id = None

        # Check for exchange-generated liquidation/ADL orders
        is_liquidation = self.c.startswith("autoclose-") if self.c else False
        is_adl = self.c.startswith("adl_autoclose") if self.c else False
        is_settlement = self.c.startswith("settlement_autoclose-") if self.c else False

        if client_order_id:
            strategy_id = exec_client._cache.strategy_id_for_order(client_order_id)

        # Log exchange-generated liquidation/ADL/settlement orders
        if is_liquidation:
            exec_client._log.warning(
                f"Received liquidation order: {self.c}, "
                f"symbol={self.s}, side={self.S.value}, "
                f"exec_type={self.x.value}, status={self.X.value}",
            )
        elif is_adl:
            exec_client._log.warning(
                f"Received ADL order: {self.c}, "
                f"symbol={self.s}, side={self.S.value}, "
                f"exec_type={self.x.value}, status={self.X.value}",
            )
        elif is_settlement:
            exec_client._log.warning(
                f"Received settlement order: {self.c}, "
                f"symbol={self.s}, side={self.S.value}, "
                f"exec_type={self.x.value}, status={self.X.value}",
            )

        # For exchange-generated orders without strategy, still need to process fills
        if strategy_id is None and not (is_liquidation or is_adl or is_settlement):
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

        instrument = exec_client._instrument_provider.find(instrument_id=instrument_id)
        if instrument is None:
            raise ValueError(
                f"Cannot process event for {instrument_id}: instrument not found in cache",
            )

        price_precision = instrument.price_precision
        size_precision = instrument.size_precision

        # Handle exchange-generated liquidation/ADL/settlement orders that may not be in cache
        # Check for CALCULATED execution type (liquidation fills) OR special client order IDs
        # Binance sends liquidation/ADL fills with x=CALCULATED and X=FILLED
        if (is_liquidation or is_adl or is_settlement) and (
            self.x == BinanceExecutionType.CALCULATED
            or self.X == BinanceOrderStatus.NEW_ADL
            or self.X == BinanceOrderStatus.NEW_INSURANCE
        ):
            # These are special exchange-generated fills without a pre-existing order
            if Decimal(self.l) == 0:
                exec_client._log.warning(
                    f"Received {self.X.value} status with l=0 for "
                    f"{'liquidation' if is_liquidation else 'ADL' if is_adl else 'settlement'} "
                    f"order {venue_order_id}, skipping",
                )
                return

            # Send OrderStatusReport first to seed the cache
            order_report = self.parse_to_order_status_report(
                account_id=exec_client.account_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
                ts_init=exec_client._clock.timestamp_ns(),
                enum_parser=exec_client._enum_parser,
            )
            exec_client._send_order_status_report(order_report)

            # Generate fill report directly for exchange-generated liquidation/ADL
            liq_commission_asset: str | None = self.N
            liq_commission_amount: str | float | None = self.n
            liq_last_qty = Quantity(float(self.l), size_precision)
            liq_last_px = Price(float(self.L), price_precision)

            if liq_commission_asset is not None:
                liq_commission = Money.from_str(f"{liq_commission_amount} {liq_commission_asset}")
            else:
                # Liquidations and ADL are always taker
                liq_fee = instrument.taker_fee
                liq_commission_asset = instrument.quote_currency
                liq_commission_amount = float(liq_last_qty * liq_last_px * liq_fee)
                liq_commission = Money(liq_commission_amount, liq_commission_asset)

            liq_venue_position_id: PositionId | None = None
            if exec_client.use_position_ids:
                liq_venue_position_id = PositionId(f"{instrument_id}-{self.ps.value}")

            # Note: We cannot use generate_order_filled without strategy_id and cached order
            # Send FillReport directly for exchange-generated liquidation/ADL orders
            fill_report = FillReport(
                account_id=exec_client.account_id,
                instrument_id=instrument_id,
                venue_order_id=venue_order_id,
                trade_id=TradeId(str(self.t)),
                order_side=exec_client._enum_parser.parse_binance_order_side(self.S),
                last_qty=liq_last_qty,
                last_px=liq_last_px,
                commission=liq_commission,
                liquidity_side=LiquiditySide.TAKER,  # Liquidations/ADL are always taker
                report_id=UUID4(),
                ts_event=ts_event,
                ts_init=exec_client._clock.timestamp_ns(),
                venue_position_id=liq_venue_position_id,
                client_order_id=client_order_id,
            )
            exec_client._send_fill_report(fill_report)
            return

        order = exec_client._cache.order(client_order_id)
        if not order:
            # For non-special exchange orders, we need the order in cache
            if is_liquidation or is_adl or is_settlement:
                exec_client._log.warning(
                    f"Cannot find order for "
                    f"{'liquidation' if is_liquidation else 'ADL' if is_adl else 'settlement'} "
                    f"{client_order_id!r}, status={self.X.value}",
                )
            else:
                exec_client._log.error(f"Cannot find order {client_order_id!r}")
            return

        if self.x == BinanceExecutionType.NEW:
            if order.order_type == OrderType.TRAILING_STOP_MARKET and order.is_open:
                return  # Already accepted: this is an update

            exec_client.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )

            # Check if price changed (for price_match orders)
            if order.has_price:
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
                        venue_order_id_modified=True,  # Setting true to avoid spurious warning log
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
                    if order.order_type == OrderType.TRAILING_STOP_MARKET:
                        exec_client.generate_order_updated(
                            strategy_id=strategy_id,
                            instrument_id=instrument_id,
                            client_order_id=client_order_id,
                            venue_order_id=venue_order_id,
                            quantity=Quantity(float(self.q), size_precision),
                            price=Price(float(self.p), price_precision),
                            trigger_price=(
                                Price(float(self.sp), price_precision) if self.sp else None
                            ),
                            ts_event=ts_event,
                        )
                    else:
                        exec_client.generate_order_expired(
                            strategy_id=strategy_id,
                            instrument_id=instrument_id,
                            client_order_id=client_order_id,
                            venue_order_id=venue_order_id,
                            ts_event=ts_event,
                        )
                    return
                elif self.X == BinanceOrderStatus.CANCELED or (
                    exec_client.treat_expired_as_canceled and self.x == BinanceExecutionType.EXPIRED
                ):
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

            # Determine commission
            commission_asset: str | None = self.N
            commission_amount: str | float | None = self.n

            last_qty = Quantity(float(self.l), size_precision)
            last_px = Price(float(self.L), price_precision)

            if commission_asset is not None:
                commission = Money.from_str(f"{commission_amount} {commission_asset}")
            else:
                fee = instrument.maker_fee if self.m else instrument.taker_fee
                commission_asset = instrument.quote_currency
                commission_amount = float(last_qty * last_px * fee)
                commission = Money(commission_amount, commission_asset)

            venue_position_id: PositionId | None = None
            if exec_client.use_position_ids:
                venue_position_id = PositionId(f"{instrument_id}-{self.ps.value}")

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
                venue_position_id=venue_position_id,
                trade_id=TradeId(str(self.t)),  # Trade ID
                order_side=exec_client._enum_parser.parse_binance_order_side(self.S),
                order_type=exec_client._enum_parser.parse_binance_order_type(self.o),
                last_qty=last_qty,
                last_px=last_px,
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=liquidity_side,
                ts_event=ts_event,
            )
        elif self.x == BinanceExecutionType.CANCELED or (
            exec_client.treat_expired_as_canceled and self.x == BinanceExecutionType.EXPIRED
        ):
            exec_client.generate_order_canceled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif self.x == BinanceExecutionType.AMENDMENT:
            exec_client.generate_order_updated(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                quantity=Quantity(float(self.q), size_precision),
                price=Price(float(self.p), price_precision),
                trigger_price=None,
                ts_event=ts_event,
            )
        elif self.x == BinanceExecutionType.EXPIRED:
            if order.order_type == OrderType.TRAILING_STOP_MARKET:
                exec_client.generate_order_updated(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    quantity=Quantity(float(self.q), size_precision),
                    price=Price(float(self.p), price_precision),
                    trigger_price=(Price(float(self.sp), price_precision) if self.sp else None),
                    ts_event=ts_event,
                )
            else:
                exec_client.generate_order_expired(
                    strategy_id=strategy_id,
                    instrument_id=instrument_id,
                    client_order_id=client_order_id,
                    venue_order_id=venue_order_id,
                    ts_event=ts_event,
                )
        elif self.x == BinanceExecutionType.REJECTED:
            due_post_only = self.f == BinanceTimeInForce.GTX

            exec_client.generate_order_rejected(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                reason="REJECTED",  # Reason string not provided by futures WS
                ts_event=ts_event,
                due_post_only=due_post_only,
            )
        elif self.x == BinanceExecutionType.TRADE_PREVENTION:
            exec_client._log.info(
                f"Self-trade prevention triggered for order {venue_order_id}, "
                f"prevented qty={self.l} at price={self.L}",
            )
        else:
            # Event not handled
            exec_client._log.warning(f"Received unhandled {self}")


class BinanceFuturesOrderUpdateMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Futures Order Update events.
    """

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time
    o: BinanceFuturesOrderData


class BinanceFuturesOrderUpdateWrapper(msgspec.Struct, frozen=True):
    """
    WebSocket message wrapper for Binance Futures Order Update events.
    """

    stream: str
    data: BinanceFuturesOrderUpdateMsg


class BinanceFuturesTradeLiteMsg(msgspec.Struct, frozen=True):
    """
    WebSocket message for Binance Futures Trade Lite events.
    """

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time

    s: str  # Symbol
    c: str  # Client Order ID
    S: BinanceOrderSide
    q: str  # Original Quantity
    p: str  # Original Price
    i: int  # Order ID
    l: str  # Order Last Filled Quantity
    L: str  # Last Filled Price
    t: int  # Trade ID
    m: bool  # Is trade the maker side

    def to_order_data(self) -> BinanceFuturesOrderData:
        """
        Convert TradeLite message to OrderData format.
        """
        return BinanceFuturesOrderData(
            s=self.s,
            c=self.c,
            S=self.S,
            o=BinanceOrderType.LIMIT,
            f=BinanceTimeInForce.GTC,
            q=self.q,
            p=self.p,
            ap="0",
            x=BinanceExecutionType.TRADE,
            X=BinanceOrderStatus.FILLED,
            i=self.i,
            l=self.l,
            z=self.l,
            L=self.L,
            N=None,
            n=None,
            T=self.T,
            t=self.t,
            b="0",
            a="0",
            m=self.m,
            R=False,
            wt=BinanceFuturesWorkingType.CONTRACT_PRICE,
            ot=BinanceOrderType.LIMIT,
            ps=BinanceFuturesPositionSide.BOTH,
            rp="0",
            gtd=0,
            pP=False,
            si=0,
            ss=0,
        )


class BinanceFuturesTradeLiteWrapper(msgspec.Struct, frozen=True):
    """
    WebSocket message wrapper for Binance Futures Trade Lite events.
    """

    stream: str
    data: BinanceFuturesTradeLiteMsg
