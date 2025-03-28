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
"""
Define websocket message of the dYdX venue.
"""

# ruff: noqa: N815

import datetime
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.dydx.common.constants import DEFAULT_CURRENCY
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXFillType
from nautilus_trader.adapters.dydx.common.enums import DYDXLiquidity
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderSide
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderType
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualMarketStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualPositionStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXPositionSide
from nautilus_trader.adapters.dydx.common.enums import DYDXTimeInForce
from nautilus_trader.adapters.dydx.common.enums import DYDXTransferType
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol

# fmt: off
from nautilus_trader.adapters.dydx.endpoints.market.instruments_info import DYDXListPerpetualMarketsResponse

# fmt: on
from nautilus_trader.adapters.dydx.schemas.account.address import DYDXSubaccount
from nautilus_trader.adapters.dydx.schemas.account.orders import DYDXOrderResponse
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
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


class DYDXCandle(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the candles data.
    """

    baseTokenVolume: str
    close: str
    high: str
    low: str
    open: str
    resolution: str
    startedAt: datetime.datetime
    startingOpenInterest: str
    ticker: str
    trades: int
    usdVolume: str
    orderbookMidPriceClose: str | None = None
    orderbookMidPriceOpen: str | None = None

    def parse_to_bar(
        self,
        bar_type: BarType,
        price_precision: int,
        size_precision: int,
        ts_init: int,
    ) -> Bar:
        """
        Parse the kline message into a nautilus Bar.
        """
        open_price = Price(Decimal(self.open), price_precision)
        high_price = Price(Decimal(self.high), price_precision)
        low_price = Price(Decimal(self.low), price_precision)
        close_price = Price(Decimal(self.close), price_precision)
        volume = Quantity(Decimal(self.baseTokenVolume), size_precision)
        return Bar(
            bar_type=bar_type,
            open=open_price,
            high=high_price,
            low=low_price,
            close=close_price,
            volume=volume,
            ts_event=dt_to_unix_nanos(self.startedAt),
            ts_init=ts_init,
        )


class DYDXWsCandlesChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the candles channel data message from dYdX.
    """

    channel: str
    connection_id: str
    contents: DYDXCandle
    id: str
    message_id: int
    type: str
    version: str


class DYDXWsCandlesMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the candles contents.
    """

    candles: list[DYDXCandle]


class DYDXWsCandlesSubscribedData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the candles channel data message from dYdX.
    """

    id: str
    channel: str
    connection_id: str
    contents: DYDXWsCandlesMessageContents
    message_id: int
    type: str


class DYDXWsMessageGeneral(msgspec.Struct):
    """
    Define a general websocket message from dYdX.
    """

    type: str | None = None
    connection_id: str | None = None
    message_id: int | None = None
    channel: str | None = None
    id: str | None = None
    message: str | None = None


class DYDXTrade(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define a trade tick.
    """

    id: str
    side: DYDXOrderSide
    size: str
    price: str
    createdAt: datetime.datetime
    type: DYDXOrderType
    createdAtHeight: str | None = None

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_init: int,
    ) -> TradeTick:
        """
        Parse the trade message to a TradeTick.
        """
        aggressor_side_map = {
            DYDXOrderSide.SELL: AggressorSide.SELLER,
            DYDXOrderSide.BUY: AggressorSide.BUYER,
        }
        return TradeTick(
            instrument_id=instrument_id,
            price=Price(Decimal(self.price), price_precision),
            size=Quantity(Decimal(self.size), size_precision),
            aggressor_side=aggressor_side_map[self.side],
            trade_id=TradeId(self.id),
            ts_event=dt_to_unix_nanos(self.createdAt),
            ts_init=ts_init,
        )


class DYDXWsTradeMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the trade message contents struct.
    """

    trades: list[DYDXTrade]


class DYDXWsTradeChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define a trade websocket message.
    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    id: str
    contents: DYDXWsTradeMessageContents
    version: str | None = None
    clobPairId: str | None = None


# Price level: the first string indicates the price, the second string indicates the size
class DYDXWsOrderbookMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the order book message contents.
    """

    bids: list[list[str]] | None = None
    asks: list[list[str]] | None = None


class DYDXWsOrderbookChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the order book messages.
    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    id: str
    contents: DYDXWsOrderbookMessageContents
    clobPairId: str | None = None
    version: str | None = None

    def parse_to_deltas(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDeltas:
        """
        Parse the order book message into OrderBookDeltas.
        """
        if self.contents.bids is None:
            self.contents.bids = []

        if self.contents.asks is None:
            self.contents.asks = []

        deltas: list[OrderBookDelta] = []

        bids_len = len(self.contents.bids)
        asks_len = len(self.contents.asks)

        for idx, bid in enumerate(self.contents.bids):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            size = Quantity(Decimal(bid[1]), size_precision)
            action = BookAction.DELETE if size == 0 else BookAction.UPDATE
            delta = OrderBookDelta(
                instrument_id=instrument_id,
                action=action,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price(Decimal(bid[0]), price_precision),
                    size=size,
                    order_id=0,
                ),
                flags=flags,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        for idx, ask in enumerate(self.contents.asks):
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            size = Quantity(Decimal(ask[1]), size_precision)
            action = BookAction.DELETE if size == 0 else BookAction.UPDATE
            delta = OrderBookDelta(
                instrument_id=instrument_id,
                action=action,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price(Decimal(ask[0]), price_precision),
                    size=size,
                    order_id=0,
                ),
                flags=flags,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class PriceLevel(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define an order book level.
    """

    price: str
    size: str


class DYDXWsOrderbookMessageSnapshotContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the order book message contents.
    """

    bids: list[PriceLevel] | None = None
    asks: list[PriceLevel] | None = None

    def parse_to_snapshot(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDeltas:
        """
        Parse the order book message into OrderBookDeltas.
        """
        deltas: list[OrderBookDelta] = []

        # Add initial clear
        clear = OrderBookDelta.clear(
            instrument_id=instrument_id,
            sequence=0,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        deltas.append(clear)

        if self.bids is None:
            self.bids = []

        if self.asks is None:
            self.asks = []

        bids_len = len(self.bids)
        asks_len = len(self.asks)

        for idx, bid in enumerate(self.bids):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            order = BookOrder(
                side=OrderSide.BUY,
                price=Price(Decimal(bid.price), price_precision),
                size=Quantity(Decimal(bid.size), size_precision),
                order_id=0,
            )

            delta = OrderBookDelta(
                instrument_id=instrument_id,
                action=BookAction.ADD,
                order=order,
                flags=flags,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            )

            deltas.append(delta)

        for idx, ask in enumerate(self.asks):
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = OrderBookDelta(
                instrument_id=instrument_id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price(Decimal(ask.price), price_precision),
                    size=Quantity(Decimal(ask.size), size_precision),
                    order_id=0,
                ),
                flags=flags,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class DYDXWsOrderbookSnapshotChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the order book snapshot messages.
    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    id: str
    contents: DYDXWsOrderbookMessageSnapshotContents
    version: str | None = None

    def parse_to_snapshot(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDeltas:
        """
        Parse the order book message into OrderBookDeltas.
        """
        return self.contents.parse_to_snapshot(
            instrument_id,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        )


class DYDXWsOrderbookBatchedData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the order book batched deltas message.
    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    id: str
    contents: list[DYDXWsOrderbookMessageContents]
    clobPairId: str | None = None
    version: str | None = None

    def parse_to_deltas(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDeltas:
        """
        Parse the order book message into OrderBookDeltas.
        """
        deltas: list[OrderBookDelta] = []
        num_delta_messages = len(self.contents)

        for delta_message_id, deltas_message in enumerate(self.contents):
            if deltas_message.bids is None:
                deltas_message.bids = []

            if deltas_message.asks is None:
                deltas_message.asks = []

            bids_len = len(deltas_message.bids)
            asks_len = len(deltas_message.asks)

            for idx, bid in enumerate(deltas_message.bids):
                flags = 0
                if (
                    delta_message_id == num_delta_messages - 1
                    and idx == bids_len - 1
                    and asks_len == 0
                ):
                    # F_LAST, 1 << 7
                    # Last message in the book event or packet from the venue for a given `instrument_id`
                    flags = RecordFlag.F_LAST

                size = Quantity(Decimal(bid[1]), size_precision)
                action = BookAction.DELETE if size == 0 else BookAction.UPDATE
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=action,
                    order=BookOrder(
                        side=OrderSide.BUY,
                        price=Price(Decimal(bid[0]), price_precision),
                        size=size,
                        order_id=0,
                    ),
                    flags=flags,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
                deltas.append(delta)

            for idx, ask in enumerate(deltas_message.asks):
                flags = 0
                if delta_message_id == num_delta_messages - 1 and idx == asks_len - 1:
                    # F_LAST, 1 << 7
                    # Last message in the book event or packet from the venue for a given `instrument_id`
                    flags = RecordFlag.F_LAST

                size = Quantity(Decimal(ask[1]), size_precision)
                action = BookAction.DELETE if size == 0 else BookAction.UPDATE
                delta = OrderBookDelta(
                    instrument_id=instrument_id,
                    action=action,
                    order=BookOrder(
                        side=OrderSide.SELL,
                        price=Price(Decimal(ask[0]), price_precision),
                        size=size,
                        order_id=0,
                    ),
                    flags=flags,
                    sequence=0,
                    ts_event=ts_event,
                    ts_init=ts_init,
                )
                deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class DYDXWsSubaccountsSubscribedContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the contents of the sub accounts subscribed message.
    """

    subaccount: DYDXSubaccount | None = None
    orders: list[DYDXOrderResponse] | None = None
    blockHeight: str | None = None

    def parse_to_account_balances(self) -> list[AccountBalance]:
        """
        Create an account balance report.
        """
        account_balances: list[AccountBalance] = []

        if self.subaccount is not None:
            currency = Currency.from_str(DEFAULT_CURRENCY)
            free = Decimal(self.subaccount.freeCollateral)
            total = Decimal(self.subaccount.equity)
            locked = total - free

            return [
                AccountBalance(
                    total=Money(total, currency),
                    locked=Money(locked, currency),
                    free=Money(free, currency),
                ),
            ]

        return account_balances


class DYDXWsSubaccountsSubscribed(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the schema for the subaccounts initial response message.

    This channel provides realtime information about orders, fills, transfers,
    perpetual positions, and perpetual assets for a subaccount.

    The initial response returns everything from the
    /v4/addresses/:address/subaccountNumber/:subaccountNumber, and
    /v4/orders?addresses=${address}&subaccountNumber=${subaccountNumber}&status=OPEN.

    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    id: str
    contents: DYDXWsSubaccountsSubscribedContents


class DYDXWalletAddress(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define a wallet address object.
    """

    address: str
    subaccountNumber: int | None = None


class DYDXWsTransferSubaccountMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define a transfer subaccount message.
    """

    sender: DYDXWalletAddress
    recipient: DYDXWalletAddress
    symbol: str
    size: str
    type: DYDXTransferType
    createdAt: datetime.datetime
    createdAtHeight: str
    transactionHash: str


class DYDXWsFillEventId(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the event id object of a fill message.
    """

    data: list[int]
    type: str


class DYDXWsFillSubaccountMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define a fill update message.
    """

    id: str
    subaccountId: str
    side: DYDXOrderSide
    liquidity: DYDXLiquidity
    type: DYDXFillType
    clobPairId: str
    size: str
    price: str
    quoteAmount: str
    eventId: DYDXWsFillEventId | str
    transactionHash: str
    createdAt: datetime.datetime
    createdAtHeight: str
    ticker: str
    orderId: str | None = None
    clientMetadata: str | None = None
    fee: str | None = None
    affiliateRevShare: str | None = None


class DYDXWsOrderSubaccountMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define an order update message.
    """

    id: str
    ticker: str
    status: DYDXOrderStatus
    orderFlags: str
    reduceOnly: bool | None = None
    postOnly: bool | None = None
    timeInForce: DYDXTimeInForce | None = None
    type: DYDXOrderType | None = None
    price: str | None = None
    size: str | None = None
    side: DYDXOrderSide | None = None
    clientMetadata: str | None = None
    clobPairId: str | None = None
    clientId: str | None = None
    subaccountId: str | None = None
    totalFilled: str | None = None
    totalOptimisticFilled: str | None = None
    goodTilBlock: str | None = None
    goodTilBlockTime: str | None = None
    removalReason: str | None = None
    createdAtHeight: str | None = None
    triggerPrice: str | None = None
    updatedAt: datetime.datetime | None = None
    updatedAtHeight: str | None = None

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        client_order_id: ClientOrderId | None,
        price_precision: int,
        size_precision: int,
        report_id: UUID4,
        enum_parser: DYDXEnumParser,
        ts_init: int,
    ) -> OrderStatusReport:
        """
        Create an order status report from the order message.
        """
        filled_qty = (
            Quantity(Decimal(self.totalFilled), size_precision)
            if self.totalFilled is not None
            else Quantity(0, size_precision)
        )
        ts_last = dt_to_unix_nanos(self.updatedAt) if self.updatedAt is not None else ts_init

        # Quantity cannot be set to zero or None. This most probably occurs when an order is canceled.
        quantity = (
            Quantity(Decimal(self.size), size_precision)
            if self.size is not None
            else Quantity(1, size_precision)
        )

        price = (
            Price(Decimal(self.price), price_precision)
            if self.price is not None
            else Price(0, price_precision)
        )

        trigger_type = (
            TriggerType.DEFAULT if self.triggerPrice is not None else TriggerType.NO_TRIGGER
        )
        trigger_price = (
            Price(Decimal(self.triggerPrice), price_precision)
            if self.triggerPrice is not None
            else None
        )

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=DYDXSymbol(self.ticker).to_instrument_id(),
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(self.id),
            order_side=enum_parser.parse_dydx_order_side(self.side),
            order_type=(
                enum_parser.parse_dydx_order_type(self.type) if self.type is not None else None
            ),
            time_in_force=(
                enum_parser.parse_dydx_time_in_force(self.timeInForce)
                if self.timeInForce is not None
                else None
            ),
            order_status=enum_parser.parse_dydx_order_status(self.status),
            price=price,
            quantity=quantity,
            filled_qty=filled_qty,
            avg_px=price,  # Assume only 1 price is used
            post_only=self.postOnly if self.postOnly is not None else False,
            reduce_only=self.reduceOnly if self.reduceOnly is not None else False,
            ts_last=ts_last,
            report_id=report_id,
            ts_accepted=0,
            ts_init=ts_init,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
        )


class DYDXWsAssetPositionSubaccountMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define an asset position update message.
    """

    address: str
    subaccountNumber: int
    positionId: str
    assetId: str
    symbol: str
    side: DYDXPositionSide
    size: str


class DYDXWsPerpetualPositionSubaccountMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define a perpetual position update message.
    """

    address: str
    subaccountNumber: int
    positionId: str
    market: str
    side: DYDXPositionSide
    status: DYDXPerpetualPositionStatus
    size: str
    maxSize: str
    netFunding: str
    entryPrice: str
    sumOpen: str
    sumClose: str
    exitPrice: str | None = None
    realizedPnl: str | None = None
    unrealizedPnl: str | None = None


class DYDXTradingReward(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the trading rewards message.
    """

    createdAt: datetime.datetime
    createdAtHeight: str
    tradingReward: str


class DYDXWsSubaccountMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the contents of a subaccount message.
    """

    perpetualPositions: list[DYDXWsPerpetualPositionSubaccountMessageContents] | None = None

    # Asset position updates on the subaccount
    assetPositions: list[DYDXWsAssetPositionSubaccountMessageContents] | None = None

    # Order updates on the subaccount
    orders: list[DYDXWsOrderSubaccountMessageContents] | None = None

    # Fills that occur on the subaccount
    fills: list[DYDXWsFillSubaccountMessageContents] | None = None

    # Transfers that occur on the subaccount
    transfers: DYDXWsTransferSubaccountMessageContents | None = None

    blockHeight: str | None = None

    tradingReward: DYDXTradingReward | None = None


class DYDXWsSubaccountsChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the schema for subaccounts updates.

    Responses will contain any update to open orders, changes in account, changes in
    open positions, and/or transfers in a single message.

    """

    channel: str
    id: str
    contents: DYDXWsSubaccountMessageContents
    eventIndex: int | None = None
    clobPairId: str | None = None
    transactionIndex: int | None = None
    blockHeight: str | None = None
    connection_id: str | None = None
    message_id: int | None = None
    type: str | None = None
    version: str | None = None


class DYDXOraclePriceMarket(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the oracle price market message.
    """

    oraclePrice: str
    effectiveAt: datetime.datetime
    effectiveAtHeight: str
    marketId: int


class DYDXTradingPerpetualMarketMessage(msgspec.Struct, forbid_unknown_fields=True):
    """
    Represent the dYdX perpetual market response object.
    """

    id: str | None = None
    clobPairId: str | None = None
    ticker: str | None = None
    marketId: int | None = None
    status: DYDXPerpetualMarketStatus | None = None
    baseAsset: str | None = None
    quoteAsset: str | None = None
    initialMarginFraction: str | None = None
    maintenanceMarginFraction: str | None = None
    basePositionSize: str | None = None
    incrementalPositionSize: str | None = None
    maxPositionSize: str | None = None
    openInterest: str | None = None
    quantumConversionExponent: int | None = None
    atomicResolution: int | None = None
    subticksPerTick: int | None = None
    stepBaseQuantums: int | None = None
    priceChange24H: str | None = None
    volume24H: str | None = None
    trades24H: int | None = None
    nextFundingRate: str | None = None
    baseOpenInterest: str | None = None
    marketType: str | None = None
    openInterestLowerCap: str | None = None
    openInterestUpperCap: str | None = None
    tickSize: str | None = None
    stepSize: str | None = None
    defaultFundingRate1H: str | None = None


class DYDXMarketMessageContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the markets contents.
    """

    trading: dict[str, DYDXTradingPerpetualMarketMessage] | None = None
    oraclePrices: dict[str, DYDXOraclePriceMarket] | None = None


class DYDXWsMarketChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the markets channel data message from dYdX.
    """

    type: str
    channel: str
    contents: DYDXMarketMessageContents
    version: str
    message_id: int
    connection_id: str | None = None
    id: str | None = None


class DYDXWsMarketSubscribedData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the markets initial channel data message from dYdX.
    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    contents: DYDXListPerpetualMarketsResponse


class DYDXBlockHeightSubscribedContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the block height subscribed contents struct.
    """

    height: str
    time: datetime.datetime


class DYDXWsBlockHeightSubscribedData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the block height subscribed data.
    """

    type: str
    connection_id: str
    message_id: int
    channel: str
    id: str
    contents: DYDXBlockHeightSubscribedContents


class DYDXBlockHeightChannelContents(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the block height channel contents struct.
    """

    blockHeight: str
    time: datetime.datetime


class DYDXWsBlockHeightChannelData(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the block height channel data.
    """

    type: str
    connection_id: str
    message_id: int
    id: str
    channel: str
    version: str
    contents: DYDXBlockHeightChannelContents
