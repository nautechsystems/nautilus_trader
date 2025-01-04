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
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
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
            order_status=OrderStatus.ACCEPTED,
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
        if client_order_id:
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

        instrument = exec_client._instrument_provider.find(instrument_id=instrument_id)
        if instrument is None:
            raise ValueError(f"Cannot handle trade: instrument {instrument_id} not found")

        price_precision = instrument.price_precision
        size_precision = instrument.size_precision

        order = exec_client._cache.order(client_order_id)
        if not order:
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
        elif self.x == BinanceExecutionType.TRADE:
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
                liquidity_side=LiquiditySide.MAKER if self.m else LiquiditySide.TAKER,
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
            instrument = exec_client._instrument_provider.find(instrument_id=instrument_id)
            if instrument is None:
                raise ValueError(f"Cannot handle amendment: instrument {instrument_id} not found")

            price_precision = instrument.price_precision
            size_precision = instrument.size_precision

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
