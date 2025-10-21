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
Unit tests for the websocket messages of dYdX.
"""

from decimal import Decimal
from pathlib import Path

import msgspec
import pytest

from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderType
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsBlockHeightChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsBlockHeightSubscribedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsCandlesChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsCandlesSubscribedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMarketChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMarketSubscribedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsMessageGeneral
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookBatchedData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsOrderbookSnapshotChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsSubaccountsChannelData
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsSubaccountsSubscribed
from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsTradeChannelData
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarAggregation
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@pytest.mark.parametrize(
    "file_path",
    [
        "tests/test_data/dydx/websocket/connected.json",
        "tests/test_data/dydx/websocket/unsubscribed.json",
        "tests/test_data/dydx/websocket/v4_candles_channel_data.json",
        "tests/test_data/dydx/websocket/v4_candles_subscribed.json",
        "tests/test_data/dydx/websocket/v4_candles.json",
        "tests/test_data/dydx/websocket/v4_orderbook_batched_data.json",
        "tests/test_data/dydx/websocket/v4_orderbook_snapshot.json",
        "tests/test_data/dydx/websocket/v4_orderbook.json",
        "tests/test_data/dydx/websocket/v4_trades.json",
        "tests/test_data/dydx/websocket/trade_deleveraged.json",
        "tests/test_data/dydx/websocket/v4_accounts_subscribed.json",
        "tests/test_data/dydx/websocket/v4_accounts_channel_data.json",
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order.json",
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_canceled.json",
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_best_effort_canceled.json",
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_opened.json",
        "tests/test_data/dydx/websocket/error.json",
        "tests/test_data/dydx/websocket/v4_accounts_rewards.json",
        "tests/test_data/dydx/websocket/v4_accounts_fills.json",
        "tests/test_data/dydx/websocket/v4_markets_subscribed.json",
        "tests/test_data/dydx/websocket/v4_markets_cross.json",
        "tests/test_data/dydx/websocket/v4_markets_channel_data.json",
        "tests/test_data/dydx/websocket/v4_markets_channel_data_v8.json",
        "tests/test_data/dydx/websocket/v4_block_height_subscribed.json",
        "tests/test_data/dydx/websocket/v4_block_height_channel_data.json",
    ],
)
def test_general_message(file_path: str) -> None:
    """
    Test the general message parser.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMessageGeneral)

    # Act
    with Path(file_path).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.type is not None


def test_block_height_subscribed_message() -> None:
    """
    Test parsing the block height subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsBlockHeightSubscribedData)

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_block_height_subscribed.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_block_height"


def test_block_height_channel_data_message() -> None:
    """
    Test parsing the block height channel data message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsBlockHeightChannelData)

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_block_height_channel_data.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_block_height"


def test_account_subscribed_message() -> None:
    """
    Test parsing the account subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsSubscribed)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_accounts_subscribed.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"


def test_markets_subscribed_message() -> None:
    """
    Test parsing the account subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMarketSubscribedData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_markets_subscribed.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_markets"
    assert msg.type == "subscribed"


def test_markets_channel_message() -> None:
    """
    Test parsing the account subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMarketChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_markets_channel_data.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_markets"
    assert msg.type == "channel_data"
    assert msg.contents.oraclePrices is not None


def test_markets_channel_message_v8() -> None:
    """
    Test parsing the account subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMarketChannelData)

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_markets_channel_data_v8.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_markets"
    assert msg.type == "channel_data"
    assert msg.contents.trading is not None
    assert msg.contents.trading["TRY-USD"].defaultFundingRate1H == "0"


def test_markets_channel_market_type() -> None:
    """
    Test parsing the account subscribed message with a CROSS market type.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMarketChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_markets_cross.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_markets"
    assert msg.type == "channel_data"
    assert msg.contents.trading is not None
    assert msg.contents.trading["FTM-USD"].marketType == "CROSS"


def test_markets_channel_trade_message() -> None:
    """
    Test parsing the account subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMarketChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_markets_trading_data.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_markets"
    assert msg.type == "channel_data"
    assert msg.contents.oraclePrices is None
    assert msg.contents.trading is not None


def test_markets_channel_oracle_price_message() -> None:
    """
    Test parsing the account subscribed message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsMarketChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_markets_oracle_price.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_markets"
    assert msg.type == "channel_data"
    assert msg.contents.oraclePrices is not None
    assert msg.contents.trading is None


def test_account_parse_to_account_balances() -> None:
    """
    Test computing the account balances.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsSubscribed)
    expected_result = [
        AccountBalance(
            total=Money(Decimal("11.62332500"), Currency.from_str("USDC")),
            locked=Money(Decimal("0"), Currency.from_str("USDC")),
            free=Money(Decimal("11.62332500"), Currency.from_str("USDC")),
        ),
    ]

    with Path("tests/test_data/dydx/websocket/v4_accounts_subscribed.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    result = msg.contents.parse_to_account_balances()

    # Assert
    assert result == expected_result


def test_account_parse_to_account_balances_order_best_effort_canceled() -> None:
    """
    Test computing the account balances with BEST_EFFORT_CANCELED orders.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsSubscribed)
    expected_result = [
        AccountBalance(
            total=Money(Decimal("11.62332500"), Currency.from_str("USDC")),
            locked=Money(Decimal("0"), Currency.from_str("USDC")),
            free=Money(Decimal("11.62332500"), Currency.from_str("USDC")),
        ),
    ]

    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_subscribed_best_effort_canceled.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    result = msg.contents.parse_to_account_balances()

    # Assert
    assert result == expected_result


def test_account_parse_to_margin_balances() -> None:
    """
    Test computing the margin balances.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsSubscribed)
    expected_result = MarginBalance(
        initial=Money(Decimal("0.00261880"), Currency.from_str("USDC")),
        maintenance=Money(Decimal("0.00157128"), Currency.from_str("USDC")),
    )

    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_subscribed_negative_initial_margin.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.subaccount is not None

    result = msg.contents.subaccount.openPerpetualPositions["ETH-USD"].parse_margin_balance(
        margin_init=Decimal("0.0005"),
        margin_maint=Decimal("0.0003"),
    )

    # Assert
    assert result == expected_result


def test_account_parse_to_position_status_report() -> None:
    """
    Test generating a position status report for a short position.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsSubscribed)
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = PositionStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        position_side=PositionSide.SHORT,
        quantity=Quantity.from_str("0.002"),
        report_id=report_id,
        ts_init=1,
        ts_last=1722496165767000000,
    )

    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_subscribed_negative_initial_margin.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.subaccount is not None

    result = msg.contents.subaccount.openPerpetualPositions[
        "ETH-USD"
    ].parse_to_position_status_report(
        account_id=account_id,
        report_id=report_id,
        size_precision=5,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
    assert result == expected_result
    assert result.quantity == expected_result.quantity


def test_account_channel_data_msg() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_accounts_channel_data.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.orders is not None
    assert len(msg.contents.orders) == 1


def test_account_channel_data_msg_affiliate_rev_share_fill() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_affiliate_rev_share.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.orders is not None
    assert msg.contents.fills is not None
    assert msg.contents.perpetualPositions is not None
    assert len(msg.contents.orders) == 1
    assert len(msg.contents.fills) == 1
    assert len(msg.contents.perpetualPositions) == 1


def test_account_channel_data_msg_order_expired() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    report_id = UUID4()
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("1964904710"),
        venue_order_id=VenueOrderId("5a1c10aa-66bb-5cf2-a106-32d6470eb43e"),
        order_side=OrderSide.NO_ORDER_SIDE,
        order_type=None,
        time_in_force=None,
        order_status=OrderStatus.CANCELED,
        price=Price(0, 4),
        quantity=Quantity(1, 5),
        filled_qty=Quantity(0, 5),
        avg_px=Price(0, 4),
        post_only=False,
        reduce_only=False,
        ts_last=1,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_expired.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.orders is not None

    result = msg.contents.orders[0].parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("1964904710"),
        price_precision=4,
        size_precision=5,
        report_id=report_id,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert len(msg.contents.orders) == 1
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.client_order_id == expected_result.client_order_id
    assert result.venue_order_id == expected_result.venue_order_id
    assert result.order_side == expected_result.order_side
    assert result.order_type == expected_result.order_type
    assert result.time_in_force == expected_result.time_in_force
    assert result.order_status == expected_result.order_status
    assert result.price == expected_result.price
    assert result.quantity == expected_result.quantity
    assert result.filled_qty == expected_result.filled_qty
    assert result.avg_px == expected_result.avg_px
    assert result.post_only is expected_result.post_only
    assert result.reduce_only is expected_result.reduce_only
    assert result.ts_last == expected_result.ts_last
    assert result.id == expected_result.id
    assert result.ts_accepted == expected_result.ts_accepted
    assert result.ts_init == expected_result.ts_init
    assert result == expected_result


def test_account_channel_data_transfers() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_transfers.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.transfers is not None


def test_account_channel_data_rewards() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_accounts_rewards.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.tradingReward is not None
    assert msg.contents.tradingReward.tradingReward == "0.000406735266535702"


def test_account_channel_data_fills() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    expected_num_fills = 2
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_accounts_fills.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.fills is not None
    assert len(msg.contents.fills) == expected_num_fills


def test_account_channel_data_new_order() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.orders is not None
    assert len(msg.contents.orders) == 1
    assert msg.contents.orders[0].status == DYDXOrderStatus.OPEN


def test_account_channel_data_new_order_opened() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    report_id = UUID4()
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("1885399800"),
        venue_order_id=VenueOrderId("09a4bd71-66b5-5eb6-886f-8c91b0e6d7bf"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.ACCEPTED,
        price=Price(2791.6, 4),
        quantity=Quantity(0.002, 5),
        filled_qty=Quantity(0, 5),
        avg_px=Price(2791.6, 4),
        post_only=False,
        reduce_only=False,
        ts_last=1,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_opened.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.orders is not None
    assert len(msg.contents.orders) == 1

    result = msg.contents.orders[0].parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("1885399800"),
        report_id=report_id,
        price_precision=4,
        size_precision=5,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.orders[0].status == DYDXOrderStatus.BEST_EFFORT_OPENED
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.client_order_id == expected_result.client_order_id
    assert result.venue_order_id == expected_result.venue_order_id
    assert result.order_side == expected_result.order_side
    assert result.order_type == expected_result.order_type
    assert result.time_in_force == expected_result.time_in_force
    assert result.order_status == expected_result.order_status
    assert result.price == expected_result.price
    assert result.trigger_price == expected_result.trigger_price
    assert result.trigger_type == expected_result.trigger_type
    assert result.quantity == expected_result.quantity
    assert result.filled_qty == expected_result.filled_qty
    assert result.avg_px == expected_result.avg_px
    assert result.post_only is expected_result.post_only
    assert result.reduce_only is expected_result.reduce_only
    assert result.ts_last == expected_result.ts_last
    assert result.id == expected_result.id
    assert result.ts_accepted == expected_result.ts_accepted
    assert result.ts_init == expected_result.ts_init
    assert result == expected_result


def test_account_channel_data_new_conditional_order_opened() -> None:
    """
    Test parsing the account channel data.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    report_id = UUID4()
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("1885399800"),
        venue_order_id=VenueOrderId("09a4bd71-66b5-5eb6-886f-8c91b0e6d7bf"),
        order_side=OrderSide.BUY,
        order_type=OrderType.STOP_LIMIT,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.ACCEPTED,
        price=Price(2791.6, 4),
        trigger_price=Price(2791.9, 4),
        trigger_type=TriggerType.DEFAULT,
        quantity=Quantity(0.002, 5),
        filled_qty=Quantity(0, 5),
        avg_px=Price(2791.6, 4),
        post_only=False,
        reduce_only=False,
        ts_last=1,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_opened_stop_limit.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.orders is not None
    assert len(msg.contents.orders) == 1

    result = msg.contents.orders[0].parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("1885399800"),
        report_id=report_id,
        price_precision=4,
        size_precision=5,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert msg.contents.orders[0].status == DYDXOrderStatus.BEST_EFFORT_OPENED
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.client_order_id == expected_result.client_order_id
    assert result.venue_order_id == expected_result.venue_order_id
    assert result.order_side == expected_result.order_side
    assert result.order_type == expected_result.order_type
    assert result.time_in_force == expected_result.time_in_force
    assert result.order_status == expected_result.order_status
    assert result.price == expected_result.price
    assert result.trigger_price == expected_result.trigger_price
    assert result.trigger_type == expected_result.trigger_type
    assert result.quantity == expected_result.quantity
    assert result.filled_qty == expected_result.filled_qty
    assert result.avg_px == expected_result.avg_px
    assert result.post_only is expected_result.post_only
    assert result.reduce_only is expected_result.reduce_only
    assert result.ts_last == expected_result.ts_last
    assert result.id == expected_result.id
    assert result.ts_accepted == expected_result.ts_accepted
    assert result.ts_init == expected_result.ts_init
    assert result == expected_result


def test_account_channel_data_order_canceled() -> None:
    """
    Test parsing the account channel data and generating an order status report.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("1053905556"),
        venue_order_id=VenueOrderId("e628b62f-2623-5192-a22f-d9ce042bd5be"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTD,
        order_status=OrderStatus.CANCELED,
        price=Price(2666.4, 4),
        quantity=Quantity(0.001, 5),
        filled_qty=Quantity(0, 5),
        avg_px=Price(2666.4, 4),
        post_only=False,
        reduce_only=False,
        ts_last=1723462022651000000,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_canceled.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.orders is not None

    result = msg.contents.orders[0].parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("1053905556"),
        report_id=report_id,
        price_precision=4,
        size_precision=5,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert len(msg.contents.orders) == 1
    assert msg.contents.orders[0].status == DYDXOrderStatus.CANCELED
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.client_order_id == expected_result.client_order_id
    assert result.venue_order_id == expected_result.venue_order_id
    assert result.order_side == expected_result.order_side
    assert result.order_type == expected_result.order_type
    assert result.time_in_force == expected_result.time_in_force
    assert result.order_status == expected_result.order_status
    assert result.price == expected_result.price
    assert result.price is not None
    assert expected_result.price is not None
    assert result.price.precision == expected_result.price.precision
    assert result.quantity == expected_result.quantity
    assert result.quantity.precision == expected_result.quantity.precision
    assert result.filled_qty == expected_result.filled_qty
    assert result.filled_qty.precision == expected_result.filled_qty.precision
    assert result.avg_px == expected_result.avg_px
    assert result.post_only is expected_result.post_only
    assert result.reduce_only is expected_result.reduce_only
    assert result.ts_last == expected_result.ts_last
    assert result.id == expected_result.id
    assert result.ts_accepted == expected_result.ts_accepted
    assert result.ts_init == expected_result.ts_init
    assert result == expected_result


def test_account_channel_data_order_best_effort_canceled() -> None:
    """
    Test parsing the account channel data and generating an order status report.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsSubaccountsChannelData)
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("1560051747"),
        venue_order_id=VenueOrderId("1a28b05a-eb97-5945-a786-12ba7320eb30"),
        order_side=OrderSide.SELL,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.PENDING_CANCEL,
        price=Price(2519.4, 4),
        quantity=Quantity(0.003, 5),
        filled_qty=Quantity(0, 5),
        avg_px=Price(2519.4, 4),
        post_only=False,
        reduce_only=True,
        ts_last=1,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    with Path(
        "tests/test_data/dydx/websocket/v4_accounts_channel_data_order_best_effort_canceled.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    assert msg.contents.orders is not None

    result = msg.contents.orders[0].parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("1560051747"),
        report_id=report_id,
        price_precision=4,
        size_precision=5,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
    assert msg.channel == "v4_subaccounts"
    assert len(msg.contents.orders) == 1
    assert msg.contents.orders[0].status == DYDXOrderStatus.BEST_EFFORT_CANCELED
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.client_order_id == expected_result.client_order_id
    assert result.venue_order_id == expected_result.venue_order_id
    assert result.order_side == expected_result.order_side
    assert result.order_type == expected_result.order_type
    assert result.time_in_force == expected_result.time_in_force
    assert result.order_status == expected_result.order_status
    assert result.price == expected_result.price
    assert result.quantity == expected_result.quantity
    assert result.filled_qty == expected_result.filled_qty
    assert result.avg_px == expected_result.avg_px
    assert result.post_only is expected_result.post_only
    assert result.reduce_only is expected_result.reduce_only
    assert result.ts_last == expected_result.ts_last
    assert result.id == expected_result.id
    assert result.ts_accepted == expected_result.ts_accepted
    assert result.ts_init == expected_result.ts_init
    assert result == expected_result


def test_klines_subscribed_data(instrument_id: InstrumentId) -> None:
    """
    Test parsing a candle message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsCandlesSubscribedData)
    expected_bar = Bar(
        bar_type=BarType(
            instrument_id=instrument_id,
            bar_spec=BarSpecification(
                step=1,
                aggregation=BarAggregation.MINUTE,
                price_type=PriceType.LAST,
            ),
            aggregation_source=AggregationSource.EXTERNAL,
        ),
        open=Price.from_str("3248.7"),
        high=Price.from_str("3248.8"),
        low=Price.from_str("3248.1"),
        close=Price.from_str("3248.1"),
        volume=Quantity.from_str("2.015"),
        ts_event=1722016680000000000,
        ts_init=0,
    )

    with Path("tests/test_data/dydx/websocket/v4_candles.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Act
    result = msg.contents.candles[0].parse_to_bar(
        bar_type=BarType(
            instrument_id=instrument_id,
            bar_spec=BarSpecification(
                step=1,
                aggregation=BarAggregation.MINUTE,
                price_type=PriceType.LAST,
            ),
            aggregation_source=AggregationSource.EXTERNAL,
        ),
        price_precision=1,
        size_precision=3,
        ts_init=0,
    )

    # Assert
    assert msg.channel == "v4_candles"
    assert result == expected_bar
    assert result.open == expected_bar.open
    assert result.high == expected_bar.high
    assert result.low == expected_bar.low
    assert result.close == expected_bar.close
    assert result.ts_event == expected_bar.ts_event
    assert result.ts_init == expected_bar.ts_init
    assert result.volume == expected_bar.volume


@pytest.mark.parametrize(
    "file_path",
    [
        "tests/test_data/dydx/websocket/v4_candles.json",
        "tests/test_data/dydx/websocket/v4_candles_subscribed.json",
    ],
)
def test_klines_subscribed_data_parsing(file_path: str) -> None:
    """
    Test parsing the initial candle message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsCandlesSubscribedData)

    # Act
    with Path(file_path).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Assert
    assert len(msg.contents.candles) >= 1


def test_klines_channel_data(instrument_id: InstrumentId) -> None:
    """
    Test parsing a candle message.
    """
    # Prepare
    decoder = msgspec.json.Decoder(DYDXWsCandlesChannelData)
    expected_bar = Bar(
        bar_type=BarType(
            instrument_id=instrument_id,
            bar_spec=BarSpecification(
                step=1,
                aggregation=BarAggregation.MINUTE,
                price_type=PriceType.LAST,
            ),
            aggregation_source=AggregationSource.EXTERNAL,
        ),
        open=Price.from_str("3246.5"),
        high=Price.from_str("3247.6"),
        low=Price.from_str("3246.5"),
        close=Price.from_str("3247.6"),
        volume=Quantity.from_str("6.364"),
        ts_event=1722016500000000000,
        ts_init=0,
    )

    with Path("tests/test_data/dydx/websocket/v4_candles_channel_data.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Act
    result = msg.contents.parse_to_bar(
        bar_type=BarType(
            instrument_id=instrument_id,
            bar_spec=BarSpecification(
                step=1,
                aggregation=BarAggregation.MINUTE,
                price_type=PriceType.LAST,
            ),
            aggregation_source=AggregationSource.EXTERNAL,
        ),
        price_precision=1,
        size_precision=3,
        ts_init=0,
    )

    # Assert
    assert msg.channel == "v4_candles"
    assert msg.connection_id == "8c25ab80-2124-4f60-82bf-9040cabc03af"
    assert result == expected_bar
    assert result.open == expected_bar.open
    assert result.high == expected_bar.high
    assert result.low == expected_bar.low
    assert result.close == expected_bar.close
    assert result.ts_event == expected_bar.ts_event
    assert result.ts_init == expected_bar.ts_init
    assert result.volume == expected_bar.volume


def test_orderbook(instrument_id: InstrumentId) -> None:
    """
    Test parsing the orderbook.
    """
    # Prepare
    expected_num_deltas = 1
    decoder = msgspec.json.Decoder(DYDXWsOrderbookChannelData)
    expected_delta = OrderBookDelta(
        instrument_id=instrument_id,
        action=BookAction.DELETE,
        order=BookOrder(
            side=OrderSide.BUY,
            price=Price(Decimal("65920"), 0),
            size=Quantity(Decimal("0"), 4),
            order_id=0,
        ),
        flags=RecordFlag.F_LAST,
        sequence=0,
        ts_event=0,
        ts_init=1,
    )

    with Path("tests/test_data/dydx/websocket/v4_orderbook.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Act
    deltas = msg.parse_to_deltas(
        instrument_id=instrument_id,
        price_precision=0,
        size_precision=5,
        ts_event=0,
        ts_init=1,
    )

    # Assert
    assert deltas.is_snapshot is False
    assert len(deltas.deltas) == expected_num_deltas
    assert deltas.deltas[0].order.size == 0
    assert deltas.deltas[0].action == BookAction.DELETE
    assert deltas.deltas[0].ts_event == expected_delta.ts_event
    assert deltas.deltas[0].ts_init == expected_delta.ts_init
    assert deltas.deltas[0].order.size == expected_delta.order.size
    assert deltas.deltas[0].order.price == expected_delta.order.price
    assert deltas.deltas[0] == expected_delta

    for delta_id, delta in enumerate(deltas.deltas):
        if delta_id < len(deltas.deltas) - 1:
            assert delta.flags == 0
        else:
            assert delta.flags == RecordFlag.F_LAST


def test_orderbook_snapshot(instrument_id: InstrumentId) -> None:
    """
    Test parsing the orderbook snapshot.
    """
    # Prepare
    expected_num_deltas = 201
    expected_clear = OrderBookDelta.clear(
        instrument_id=instrument_id,
        sequence=0,
        ts_event=0,
        ts_init=0,
    )
    expected_delta = OrderBookDelta(
        instrument_id=instrument_id,
        action=BookAction.ADD,
        order=BookOrder(
            side=OrderSide.BUY,
            price=Price(3393.2, 1),
            size=Quantity(7.795, 3),
            order_id=0,
        ),
        flags=0,
        sequence=0,
        ts_event=0,
        ts_init=0,
    )
    decoder = msgspec.json.Decoder(DYDXWsOrderbookSnapshotChannelData)

    with Path("tests/test_data/dydx/websocket/v4_orderbook_snapshot.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Act
    deltas = msg.parse_to_snapshot(
        instrument_id=instrument_id,
        price_precision=1,
        size_precision=3,
        ts_event=0,
        ts_init=0,
    )

    # Assert
    assert deltas.is_snapshot
    assert len(deltas.deltas) == expected_num_deltas
    assert deltas.deltas[0] == expected_clear
    assert deltas.deltas[1] == expected_delta
    assert deltas.deltas[1].order.price == expected_delta.order.price
    assert deltas.deltas[1].order.size == expected_delta.order.size
    assert deltas.deltas[1].order.side == expected_delta.order.side

    for delta_id, delta in enumerate(deltas.deltas):
        if delta_id < len(deltas.deltas) - 1:
            assert delta.flags == 0
        else:
            assert delta.flags == RecordFlag.F_LAST


def test_orderbook_batched_data(instrument_id: InstrumentId) -> None:
    """
    Test parsing the orderbook batch deltas.
    """
    # Prepare
    expected_num_deltas = 41
    expected_delta = OrderBookDelta(
        instrument_id=instrument_id,
        action=BookAction.UPDATE,
        order=BookOrder(
            side=OrderSide.BUY,
            price=Price(2396.1, 1),
            size=Quantity(3.123, 3),
            order_id=0,
        ),
        flags=0,
        sequence=0,
        ts_event=0,
        ts_init=0,
    )
    decoder = msgspec.json.Decoder(DYDXWsOrderbookBatchedData)

    with Path(
        "tests/test_data/dydx/websocket/v4_orderbook_batched_data.json",
    ).open() as file_reader:
        msg = decoder.decode(file_reader.read())

    # Act
    deltas = msg.parse_to_deltas(
        instrument_id=instrument_id,
        price_precision=1,
        size_precision=3,
        ts_event=0,
        ts_init=0,
    )

    # Assert
    assert deltas.is_snapshot is False
    assert len(deltas.deltas) == expected_num_deltas
    assert deltas.deltas[0] == expected_delta
    assert deltas.deltas[0].order.price == expected_delta.order.price
    assert deltas.deltas[0].order.price.precision == expected_delta.order.price.precision
    assert deltas.deltas[0].order.size == expected_delta.order.size
    assert deltas.deltas[0].order.size.precision == expected_delta.order.size.precision
    assert deltas.deltas[0].order.side == expected_delta.order.side

    for delta_id, delta in enumerate(deltas.deltas):
        if delta_id < len(deltas.deltas) - 1:
            assert delta.flags == 0
        else:
            assert delta.flags == RecordFlag.F_LAST


def test_trades(instrument_id: InstrumentId) -> None:
    """
    Test parsing trade messages.
    """
    # Prepare
    expected_num_trades = 2
    expected_trade = TradeTick(
        instrument_id=instrument_id,
        price=Price(3393, 0),
        size=Quantity(0.01, 5),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("014206cf0000000200000002"),
        ts_event=1721848355705000000,
        ts_init=0,
    )
    decoder = msgspec.json.Decoder(DYDXWsTradeChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/v4_trades.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    trade_tick = msg.contents.trades[0].parse_to_trade_tick(
        instrument_id=instrument_id,
        price_precision=0,
        size_precision=5,
        ts_init=0,
    )

    # Assert
    assert len(msg.contents.trades) == expected_num_trades
    assert trade_tick == expected_trade
    assert trade_tick.instrument_id == expected_trade.instrument_id
    assert trade_tick.price == expected_trade.price
    assert trade_tick.size == expected_trade.size
    assert trade_tick.aggressor_side == expected_trade.aggressor_side
    assert trade_tick.trade_id == expected_trade.trade_id
    assert trade_tick.ts_event == expected_trade.ts_event
    assert trade_tick.ts_init == expected_trade.ts_init


def test_trades_deleveraged(instrument_id: InstrumentId) -> None:
    """
    Test parsing trade messages.
    """
    # Prepare
    expected_num_trades = 3
    expected_trade = TradeTick(
        instrument_id=instrument_id,
        price=Price(2340.7442700369913687, 0),
        size=Quantity(0.811, 5),
        aggressor_side=AggressorSide.SELLER,
        trade_id=TradeId("015034b90000000200000026"),
        ts_event=1722820168338000000,
        ts_init=0,
    )
    decoder = msgspec.json.Decoder(DYDXWsTradeChannelData)

    # Act
    with Path("tests/test_data/dydx/websocket/trade_deleveraged.json").open() as file_reader:
        msg = decoder.decode(file_reader.read())

    trade_tick = msg.contents.trades[2].parse_to_trade_tick(
        instrument_id=instrument_id,
        price_precision=0,
        size_precision=5,
        ts_init=0,
    )

    # Assert
    assert msg.contents.trades[2].type == DYDXOrderType.DELEVERAGED
    assert len(msg.contents.trades) == expected_num_trades
    assert trade_tick.instrument_id == expected_trade.instrument_id
    assert trade_tick.price == expected_trade.price
    assert trade_tick.size == expected_trade.size
    assert trade_tick.aggressor_side == expected_trade.aggressor_side
    assert trade_tick.trade_id == expected_trade.trade_id
    assert trade_tick.ts_event == expected_trade.ts_event
    assert trade_tick.ts_init == expected_trade.ts_init
    assert trade_tick == expected_trade
