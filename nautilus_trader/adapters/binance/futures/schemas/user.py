# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEventType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesPositionUpdateReason
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesWorkingType
from nautilus_trader.adapters.binance.futures.execution import BinanceFuturesExecutionClient
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# WebSocket messages
################################################################################


class BinanceFuturesUserMsgData(msgspec.Struct):
    """
    Inner struct for execution WebSocket messages from `Binance`
    """

    e: BinanceFuturesEventType


class BinanceFuturesUserMsgWrapper(msgspec.Struct):
    """
    Provides a wrapper for execution WebSocket messages from `Binance`.
    """

    stream: str
    data: BinanceFuturesUserMsgData


class MarginCallPosition(msgspec.Struct):
    """Inner struct position for `Binance Futures` Margin Call events."""

    s: BinanceSymbol  # Symbol
    ps: BinanceFuturesPositionSide  # Position Side
    pa: str  # Position  Amount
    mt: str  # Margin Type
    iw: str  # Isolated Wallet(if isolated position)
    mp: str  # MarkPrice
    up: str  # Unrealized PnL
    mm: str  # Maintenance Margin Required


class BinanceFuturesMarginCallMsg(msgspec.Struct):
    """WebSocket message for `Binance Futures` Margin Call events."""

    e: str  # Event Type
    E: int  # Event Time
    cw: float  # Cross Wallet Balance. Only pushed with crossed position margin call
    p: list[MarginCallPosition]


class BinanceFuturesBalance(msgspec.Struct):
    """Inner struct balance for `Binance Futures` Balance and Position update event."""

    a: str  # Asset
    wb: str  # Wallet Balance
    cw: str  # Cross Wallet Balance
    bc: str  # Balance Change except PnL and Commission

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.a)
        free = Decimal(self.wb)
        locked = Decimal(0)  # TODO(cs): Pending refactoring of accounting
        total: Decimal = free + locked

        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )


class BinanceFuturesPosition(msgspec.Struct):
    """Inner struct position for `Binance Futures` Balance and Position update event."""

    s: BinanceSymbol  # Symbol
    pa: str  # Position amount
    ep: str  # Entry price
    cr: str  # (Pre-free) Accumulated Realized
    up: str  # Unrealized PnL
    mt: str  # Margin type
    iw: str  # Isolated wallet
    ps: BinanceFuturesPositionSide


class BinanceFuturesAccountUpdateData(msgspec.Struct):
    """WebSocket message for `Binance Futures` Balance and Position Update events."""

    m: BinanceFuturesPositionUpdateReason
    B: list[BinanceFuturesBalance]
    P: list[BinanceFuturesPosition]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.B]


class BinanceFuturesAccountUpdateMsg(msgspec.Struct):
    """WebSocket message for `Binance Futures` Balance and Position Update events."""

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time
    a: BinanceFuturesAccountUpdateData

    def handle_account_update(self, exec_client: BinanceFuturesExecutionClient):
        """Handle BinanceFuturesAccountUpdateMsg as payload of ACCOUNT_UPDATE."""
        exec_client.generate_account_state(
            balances=self.a.parse_to_account_balances(),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(self.T),
        )


class BinanceFuturesAccountUpdateWrapper(msgspec.Struct):
    """WebSocket message wrapper for `Binance Futures` Balance and Position Update events."""

    stream: str
    data: BinanceFuturesAccountUpdateMsg


class BinanceFuturesOrderData(msgspec.Struct, kw_only=True):
    """
    WebSocket message 'inner struct' for `Binance Futures` Order Update events.

    Client Order ID 'c':
     - starts with "autoclose-": liquidation order/
     - starts with "adl_autoclose": ADL auto close order/
    """

    s: BinanceSymbol  # Symbol
    c: str  # Client Order ID
    S: BinanceOrderSide
    o: BinanceOrderType
    f: BinanceTimeInForce
    q: str  # Original Quantity
    p: str  # Original Price
    ap: str  # Average Price
    sp: Optional[str] = None  # Stop Price. Please ignore with TRAILING_STOP_MARKET order
    x: BinanceExecutionType
    X: BinanceOrderStatus
    i: int  # Order ID
    l: str  # Order Last Filled Quantity
    z: str  # Order Filled Accumulated Quantity
    L: str  # Last Filled Price
    N: Optional[str] = None  # Commission Asset, will not push if no commission
    n: Optional[str] = None  # Commission, will not push if no commission
    T: int  # Order Trade Time
    t: int  # Trade ID
    b: str  # Bids Notional
    a: str  # Ask Notional
    m: bool  # Is trade the maker side
    R: bool  # Is reduce only
    wt: BinanceFuturesWorkingType
    ot: BinanceOrderType
    ps: BinanceFuturesPositionSide
    cp: Optional[bool] = None  # If Close-All, pushed with conditional order
    AP: Optional[str] = None  # Activation Price, only pushed with TRAILING_STOP_MARKET order
    cr: Optional[str] = None  # Callback Rate, only pushed with TRAILING_STOP_MARKET order
    pP: bool  # ignore
    si: int  # ignore
    ss: int  # ignore
    rp: str  # Realized Profit of the trade

    def resolve_internal_variables(self, exec_client: BinanceFuturesExecutionClient) -> None:
        self.client_order_id = ClientOrderId(self.c) if self.c != "" else None
        self.ts_event = millis_to_nanos(self.T)
        self.venue_order_id = VenueOrderId(str(self.i))
        self.instrument_id = exec_client._get_cached_instrument_id(self.s)
        self.strategy_id = exec_client._cache.strategy_id_for_order(self.client_order_id)
        self.resolved = True

    def parse_to_order_status_report(
        self,
        exec_client: BinanceFuturesExecutionClient,
    ) -> OrderStatusReport:
        if self.resolved is not True:
            self.resolve_internal_variables(exec_client)
        price = Price.from_str(self.p) if self.p is not None else None
        trigger_price = Price.from_str(self.sp) if self.sp is not None else None
        trailing_offset = Decimal(self.cr) * 100 if self.cr is not None else None
        order_side = (OrderSide.BUY if self.S == BinanceOrderSide.BUY else OrderSide.SELL,)
        post_only = self.f == BinanceTimeInForce.GTX

        return OrderStatusReport(
            account_id=exec_client.account_id,
            instrument_id=self.instrument_id,
            client_order_id=self.client_order_id,
            venue_order_id=self.venue_order_id,
            order_side=order_side,
            order_type=exec_client._enum_parser.parse_binance_order_type(self.o),
            time_in_force=exec_client._enum_parser.parse_binance_time_in_force(self.f),
            order_status=OrderStatus.ACCEPTED,
            price=price,
            trigger_price=trigger_price,
            trigger_type=exec_client._enum_parser.parse_binance_trigger_type(self.wt.value),
            trailing_offset=trailing_offset,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            quantity=Quantity.from_str(self.q),
            filled_qty=Quantity.from_str(self.z),
            avg_px=None,
            post_only=post_only,
            reduce_only=self.R,
            report_id=UUID4(),
            ts_accepted=self.ts_event,
            ts_last=self.ts_event,
            ts_init=exec_client._clock.timestamp_ns(),
        )

    def handle_order_trade_update(
        self,
        exec_client: BinanceFuturesExecutionClient,
    ):
        """Handle BinanceFuturesOrderData as payload of ORDER_TRADE_UPDATE event."""
        if self.resolved is not True:
            self.resolve_internal_variables(exec_client)
        if self.strategy_id is None:
            report = self.parse_to_order_status_report(exec_client)
            exec_client._send_order_status_report(report)
        elif self.x == BinanceExecutionType.NEW:
            exec_client.generate_order_accepted(
                strategy_id=self.strategy_id,
                instrument_id=self.instrument_id,
                client_order_id=self.client_order_id,
                venue_order_id=self.venue_order_id,
                ts_event=self.ts_event,
            )
        elif self.x == BinanceExecutionType.TRADE:
            instrument = exec_client._instrument_provider.find(instrument_id=self.instrument_id)

            # Determine commission
            commission_asset: str = self.N
            commission_amount: str = self.n
            if commission_asset is not None:
                commission = Money.from_str(f"{commission_amount} {commission_asset}")
            else:
                # Commission in margin collateral currency
                commission = Money(0, instrument.quote_currency)

            exec_client.generate_order_filled(
                strategy_id=self.strategy_id,
                instrument_id=self.instrument_id,
                client_order_id=self.client_order_id,
                venue_order_id=self.venue_order_id,
                venue_position_id=PositionId(f"{self.instrument_id}-{self.ps.value}"),
                trade_id=TradeId(str(self.t)),  # Trade ID
                order_side=exec_client._enum_parser.parse_binance_order_side(self.S),
                order_type=exec_client._enum_parser.parse_binance_order_type(self.o),
                last_qty=Quantity.from_str(self.l),
                last_px=Price.from_str(self.L),
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.MAKER if self.m else LiquiditySide.TAKER,
                ts_event=self.ts_event,
            )
        elif self.x == BinanceExecutionType.CANCELED:
            exec_client.generate_order_canceled(
                strategy_id=self.strategy_id,
                instrument_id=self.instrument_id,
                client_order_id=self.client_order_id,
                venue_order_id=self.venue_order_id,
                ts_event=self.ts_event,
            )
        elif self.x == BinanceExecutionType.EXPIRED:
            exec_client.generate_order_expired(
                strategy_id=self.strategy_id,
                instrument_id=self.instrument_id,
                client_order_id=self.client_order_id,
                venue_order_id=self.venue_order_id,
                ts_event=self.ts_event,
            )
        else:
            # Event not handled
            exec_client._log.warning(f"Received unhandled {self}")


class BinanceFuturesOrderUpdateMsg(msgspec.Struct):
    """WebSocket message for `Binance Futures` Order Update events."""

    e: str  # Event Type
    E: int  # Event Time
    T: int  # Transaction Time
    o: BinanceFuturesOrderData


class BinanceFuturesOrderUpdateWrapper(msgspec.Struct):
    """WebSocket message wrapper for `Binance Futures` Order Update events."""

    stream: str
    data: BinanceFuturesOrderUpdateMsg
