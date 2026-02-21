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

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# HTTP responses
################################################################################


class BinanceFuturesBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response 'inner struct' from Binance Futures GET /fapi/v2/account (HMAC
    SHA256).
    """

    asset: str  # asset name
    walletBalance: str  # wallet balance
    unrealizedProfit: str  # unrealized profit
    marginBalance: str  # margin balance
    maintMargin: str  # maintenance margin required
    initialMargin: str  # total initial margin required with current mark price
    positionInitialMargin: str  # initial margin required for positions with current mark price
    openOrderInitialMargin: str  # initial margin required for open orders with current mark price
    crossWalletBalance: str  # crossed wallet balance
    crossUnPnl: str  # unrealized profit of crossed positions
    availableBalance: str  # available balance
    maxWithdrawAmount: str  # maximum amount for transfer out
    # whether the asset can be used as margin in Multi - Assets mode
    marginAvailable: bool | None = None
    updateTime: int | None = None  # last update time

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        # This calculation is currently mixing wallet cash balance and the available balance after
        # considering margin collateral. As a temporary measure we're taking the `min` to
        # disregard free amounts above the cash balance, but still considering where not all
        # balance is available (so locked in some way, i.e. allocated as collateral).
        total = Money(Decimal(self.walletBalance), currency)
        free = Money(min(Decimal(self.availableBalance), Decimal(self.walletBalance)), currency)
        locked = total - free
        return AccountBalance(
            total=total,
            locked=locked,
            free=free,
        )

    def parse_to_margin_balance(self) -> MarginBalance:
        currency: Currency = Currency.from_str(self.asset)
        return MarginBalance(
            initial=Money(Decimal(self.initialMargin), currency),
            maintenance=Money(Decimal(self.maintMargin), currency),
        )


class BinanceFuturesAccountInfo(msgspec.Struct, kw_only=True, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v2/account (HMAC SHA256).
    """

    feeTier: int  # account commission tier
    canTrade: bool  # if can trade
    canDeposit: bool  # if can transfer in asset
    canWithdraw: bool  # if can transfer out asset
    updateTime: int
    totalInitialMargin: str | None = (
        None  # total initial margin required with current mark price (useless with isolated positions), only for USDT
    )
    totalMaintMargin: str | None = None  # total maintenance margin required, only for USDT asset
    totalWalletBalance: str | None = None  # total wallet balance, only for USDT asset
    totalUnrealizedProfit: str | None = None  # total unrealized profit, only for USDT asset
    totalMarginBalance: str | None = None  # total margin balance, only for USDT asset
    # initial margin required for positions with current mark price, only for USDT asset
    totalPositionInitialMargin: str | None = None
    # initial margin required for open orders with current mark price, only for USDT asset
    totalOpenOrderInitialMargin: str | None = None
    totalCrossWalletBalance: str | None = None  # crossed wallet balance, only for USDT asset
    # unrealized profit of crossed positions, only for USDT asset
    totalCrossUnPnl: str | None = None
    availableBalance: str | None = None  # available balance, only for USDT asset
    maxWithdrawAmount: str | None = None  # maximum amount for transfer out, only for USDT asset
    assets: list[BinanceFuturesBalanceInfo]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [asset.parse_to_account_balance() for asset in self.assets]

    def parse_to_margin_balances(self) -> list[MarginBalance]:
        return [asset.parse_to_margin_balance() for asset in self.assets]


class BinanceFuturesPositionRisk(msgspec.Struct, kw_only=True, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v3/positionRisk (HMAC SHA256).

    Supports both v2 and v3 schemas. v2 fields (marginType, isAutoAddMargin,
    leverage, maxNotionalValue) are optional for backward compatibility.
    v3 adds: breakEvenPrice, notional, marginAsset, isolatedWallet, initialMargin,
    maintMargin, positionInitialMargin, openOrderInitialMargin, adl, bidNotional,
    askNotional.

    """

    # Core fields (present in both v2 and v3)
    symbol: str
    positionSide: BinanceFuturesPositionSide
    positionAmt: str
    entryPrice: str
    markPrice: str
    unRealizedProfit: str
    liquidationPrice: str
    isolatedMargin: str
    updateTime: int

    # v2 fields (may not be present in v3)
    marginType: str | None = None
    isAutoAddMargin: str | None = None
    leverage: str | None = None
    maxNotionalValue: str | None = None

    # v3-specific fields
    breakEvenPrice: str | None = None
    notional: str | None = None
    marginAsset: str | None = None
    isolatedWallet: str | None = None
    initialMargin: str | None = None
    maintMargin: str | None = None
    positionInitialMargin: str | None = None
    openOrderInitialMargin: str | None = None
    adl: int | None = None
    bidNotional: str | None = None
    askNotional: str | None = None

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        enum_parser: BinanceFuturesEnumParser,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        net_size = Decimal(self.positionAmt)

        venue_position_id: PositionId | None = None

        if self.positionSide in (
            BinanceFuturesPositionSide.LONG,
            BinanceFuturesPositionSide.SHORT,
        ):
            position_side = (
                PositionSide.LONG
                if self.positionSide == BinanceFuturesPositionSide.LONG
                else PositionSide.SHORT
            )
            venue_position_id = PositionId(f"{instrument_id}-{self.positionSide.value}")
        else:
            position_side = enum_parser.parse_futures_position_side(net_size)

        avg_px_open = Decimal(self.entryPrice) if self.entryPrice else None

        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=Quantity.from_str(str(abs(net_size))),
            report_id=report_id,
            ts_last=ts_init,
            ts_init=ts_init,
            venue_position_id=venue_position_id,
            avg_px_open=avg_px_open,
        )


class BinanceFuturesDualSidePosition(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v1/positionSide/dual (HMAC SHA256).
    """

    dualSidePosition: bool


class BinanceFuturesFeeRates(msgspec.Struct, frozen=True):
    """
    Represents a Binance Futures fee tier.

    https://www.binance.com/en/fee/futureFee

    """

    feeTier: int
    maker: str
    taker: str


class BinanceFuturesLeverage(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures POST /fapi/v1/leverage.
    """

    leverage: int
    maxNotionalValue: str
    symbol: str


class BinanceFuturesSymbolConfig(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures GET /fapi/v1/symbolConfig.

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/account/rest-api/Symbol-Config

    """

    symbol: str
    marginType: str
    isAutoAddMargin: bool
    leverage: int
    maxNotionalValue: str


class BinanceFuturesMarginTypeResponse(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures `POST /fapi/v1/marginType`.
    """

    code: int
    msg: str


_ALGO_STATUS_MAP: dict[str, OrderStatus] = {
    "NEW": OrderStatus.ACCEPTED,
    "TRIGGERED": OrderStatus.ACCEPTED,
    "TRIGGERING": OrderStatus.ACCEPTED,
    "CANCELED": OrderStatus.CANCELED,
    "FINISHED": OrderStatus.FILLED,
    "EXPIRED": OrderStatus.EXPIRED,
    "REJECTED": OrderStatus.REJECTED,
}


class BinanceFuturesAlgoOrder(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures `POST /fapi/v1/algoOrder` and `GET
    /fapi/v1/openAlgoOrders`.

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/New-Algo-Order
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Current-All-Algo-Open-Orders

    """

    algoId: int
    clientAlgoId: str
    algoType: str
    orderType: str
    symbol: str
    side: str
    positionSide: str | None = None
    timeInForce: str | None = None
    quantity: str | None = None
    algoStatus: str | None = None
    triggerPrice: str | None = None
    price: str | None = None
    workingType: str | None = None
    priceMatch: str | None = None
    closePosition: bool | None = None
    priceProtect: bool | None = None
    reduceOnly: bool | None = None
    selfTradePreventionMode: str | None = None
    activatePrice: str | None = None
    callbackRate: str | None = None
    createTime: int | None = None
    updateTime: int | None = None
    triggerTime: int | None = None
    goodTillDate: int | None = None

    # Fields populated for triggered orders from openAlgoOrders endpoint
    actualOrderId: str | None = None

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        enum_parser: BinanceFuturesEnumParser,
        ts_init: int,
    ) -> OrderStatusReport:
        """
        Parse the algo order to an OrderStatusReport.

        Parameters
        ----------
        account_id : AccountId
            The account ID for the report.
        instrument_id : InstrumentId
            The instrument ID for the report.
        report_id : UUID4
            The report ID.
        enum_parser : BinanceFuturesEnumParser
            The enum parser.
        ts_init : int
            The initialization timestamp (UNIX nanoseconds).

        Returns
        -------
        OrderStatusReport

        Raises
        ------
        ValueError
            If quantity is missing (e.g., close-position orders).

        """
        # Close-position orders don't have quantity (determined at execution)
        if not self.quantity:
            raise ValueError(
                f"Cannot create OrderStatusReport for algo order {self.algoId} "
                f"without quantity (closePosition={self.closePosition})",
            )

        client_order_id = ClientOrderId(self.clientAlgoId) if self.clientAlgoId else None
        venue_order_id_str = self.actualOrderId or str(self.algoId)
        venue_order_id = VenueOrderId(venue_order_id_str)

        trigger_type = TriggerType.NO_TRIGGER
        if self.workingType is not None:
            trigger_type = enum_parser.parse_binance_trigger_type(self.workingType)
        elif self.triggerPrice and Decimal(self.triggerPrice) > 0:
            trigger_type = TriggerType.LAST_PRICE

        # Binance sends callbackRate in percent (e.g., 1.0 = 1%), convert to basis points
        trailing_offset = None
        trailing_offset_type = TrailingOffsetType.NO_TRAILING_OFFSET
        if self.callbackRate is not None:
            trailing_offset = Decimal(self.callbackRate) * 100
            trailing_offset_type = TrailingOffsetType.BASIS_POINTS

        order_status = OrderStatus.ACCEPTED
        if self.algoStatus:
            order_status = _ALGO_STATUS_MAP.get(self.algoStatus.upper(), OrderStatus.ACCEPTED)

        binance_order_type = BinanceOrderType(self.orderType)
        order_type = enum_parser.parse_binance_order_type(binance_order_type)

        binance_order_side = BinanceOrderSide(self.side)
        order_side = enum_parser.parse_binance_order_side(binance_order_side)

        price_str = self.price or "0"
        trigger_price_str = self.triggerPrice or self.activatePrice or "0"
        reduce_only = self.reduceOnly if self.reduceOnly is not None else False

        time_in_force = TimeInForce.GTC
        if self.timeInForce:
            binance_tif = BinanceTimeInForce(self.timeInForce)
            time_in_force = enum_parser.parse_binance_time_in_force(binance_tif)

        ts_accepted = millis_to_nanos(self.createTime) if self.createTime else ts_init
        ts_last = millis_to_nanos(self.updateTime) if self.updateTime else ts_accepted

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_list_id=None,
            venue_order_id=venue_order_id,
            order_side=order_side,
            order_type=order_type,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            time_in_force=time_in_force,
            order_status=order_status,
            price=Price.from_str(price_str),
            trigger_price=Price.from_str(trigger_price_str),
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            quantity=Quantity.from_str(self.quantity),
            filled_qty=Quantity.from_str("0"),  # Algo orders don't have fill info here
            avg_px=None,
            post_only=False,
            reduce_only=reduce_only,
            ts_accepted=ts_accepted,
            ts_last=ts_last,
            report_id=report_id,
            ts_init=ts_init,
        )


class BinanceFuturesAlgoOrderCancelResponse(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Futures `DELETE /fapi/v1/algoOrder`.

    References
    ----------
    https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Cancel-Algo-Order

    """

    algoId: int
    clientAlgoId: str
    code: str
    msg: str
