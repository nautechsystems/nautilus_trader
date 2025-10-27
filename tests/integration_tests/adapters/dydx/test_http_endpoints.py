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
Unit tests for the HTTP endpoints.
"""

from decimal import Decimal
from pathlib import Path

import msgspec
import pytest

from nautilus_trader.adapters.dydx.common.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualPositionStatus
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.endpoints.market.instruments_info import DYDXListPerpetualMarketsResponse
from nautilus_trader.adapters.dydx.schemas.account.address import DYDXAddressResponse
from nautilus_trader.adapters.dydx.schemas.account.address import DYDXSubaccountResponse
from nautilus_trader.adapters.dydx.schemas.account.asset_positions import DYDXAssetPositionsResponse
from nautilus_trader.adapters.dydx.schemas.account.fills import DYDXFillsResponse
from nautilus_trader.adapters.dydx.schemas.account.orders import DYDXOrderResponse
from nautilus_trader.adapters.dydx.schemas.account.perpetual_positions import DYDXPerpetualPositionsResponse
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@pytest.fixture
def list_perpetual_markets_response() -> DYDXListPerpetualMarketsResponse:
    """
    Create an perpetual markets endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXListPerpetualMarketsResponse)

    with Path("tests/test_data/dydx/http/list_perpetual_markets.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def addresses_response() -> DYDXAddressResponse:
    """
    Create an addresses endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXAddressResponse)

    with Path("tests/test_data/dydx/http/addresses.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def subaccount_response() -> DYDXSubaccountResponse:
    """
    Create an subaccount endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXSubaccountResponse)

    with Path("tests/test_data/dydx/http/subaccount.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def asset_positions_response() -> DYDXAssetPositionsResponse:
    """
    Create an asset positions endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXAssetPositionsResponse)

    with Path("tests/test_data/dydx/http/asset_positions.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def perpetual_positions_response() -> DYDXPerpetualPositionsResponse:
    """
    Create an perpetual positions endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXPerpetualPositionsResponse)

    with Path("tests/test_data/dydx/http/list_perpetual_positions.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def orders_response() -> list[DYDXOrderResponse]:
    """
    Create an orders endpoint response.
    """
    with Path("tests/test_data/dydx/http/orders.json").open() as file_reader:
        return msgspec.json.decode(file_reader.read(), type=list[DYDXOrderResponse], strict=True)


@pytest.fixture
def order_response() -> DYDXOrderResponse:
    """
    Create an endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXOrderResponse)

    with Path("tests/test_data/dydx/http/order.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def conditional_order_response() -> DYDXOrderResponse:
    """
    Create an endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXOrderResponse)

    with Path("tests/test_data/dydx/http/conditional_order.json").open() as file_reader:
        return decoder.decode(file_reader.read())


@pytest.fixture
def fills_response() -> DYDXFillsResponse:
    """
    Create an addresses endpoint response.
    """
    decoder = msgspec.json.Decoder(DYDXFillsResponse)

    with Path("tests/test_data/dydx/http/fills.json").open() as file_reader:
        return decoder.decode(file_reader.read())


def test_addresses(addresses_response: DYDXAddressResponse) -> None:
    """
    Test parsing the address message.
    """
    # Assert
    assert len(addresses_response.subaccounts) == 1


def test_subaccount(subaccount_response: DYDXSubaccountResponse) -> None:
    """
    Test parsing the subaccount message.
    """
    # Assert
    assert subaccount_response.subaccount.marginEnabled


def test_asset_positions(asset_positions_response: DYDXAssetPositionsResponse) -> None:
    """
    Test parsing the subaccount message.
    """
    # Assert
    assert len(asset_positions_response.positions) == 1


def test_perpetual_positions(perpetual_positions_response: DYDXPerpetualPositionsResponse) -> None:
    """
    Test parsing the positions message.
    """
    # Assert
    assert len(perpetual_positions_response.positions) == 1
    assert perpetual_positions_response.positions[0].base_currency() == "ETH"
    assert perpetual_positions_response.positions[0].quote_currency() == "USDC"


def test_perpetual_positions_into_margin_balance(
    perpetual_positions_response: DYDXPerpetualPositionsResponse,
) -> None:
    """
    Test parsing the positions message into a margin balance report.
    """
    expected_result = MarginBalance(
        initial=Money(Decimal("0.00953330"), Currency.from_str("USDC")),
        maintenance=Money(Decimal("0.01906660"), Currency.from_str("USDC")),
    )

    # Act
    result = perpetual_positions_response.positions[0].parse_margin_balance(
        margin_init=Decimal("0.001"),
        margin_maint=Decimal("0.002"),
    )

    # Assert
    assert result == expected_result
    assert result.initial == expected_result.initial
    assert result.maintenance == expected_result.maintenance


def test_closed_perpetual_positions_into_margin_balance(
    perpetual_positions_response: DYDXPerpetualPositionsResponse,
) -> None:
    """
    Test parsing the positions message into a margin balance report.
    """
    perpetual_positions_response.positions[0].status = DYDXPerpetualPositionStatus.CLOSED
    expected_result = MarginBalance(
        initial=Money(Decimal("0"), Currency.from_str("USDC")),
        maintenance=Money(Decimal("0"), Currency.from_str("USDC")),
    )

    # Act
    result = perpetual_positions_response.positions[0].parse_margin_balance(
        margin_init=Decimal("0.001"),
        margin_maint=Decimal("0.002"),
    )

    # Assert
    assert result == expected_result
    assert result.initial == expected_result.initial
    assert result.maintenance == expected_result.maintenance


def test_orders(orders_response: list[DYDXOrderResponse]) -> None:
    """
    Test parsing the orders response.
    """
    # Assert
    assert len(orders_response) == 1
    assert orders_response[0].base_currency() == "ETH"
    assert orders_response[0].quote_currency() == "USDC"


def test_order(order_response: DYDXOrderResponse) -> None:
    """
    Test parsing the orders response.
    """
    # Assert
    assert order_response.clientId == "2043599281"


def test_fills(fills_response: DYDXFillsResponse) -> None:
    """
    Test parsing the fills message.
    """
    # Prepare
    expected_num_fills = 2

    # Assert
    assert len(fills_response.fills) == expected_num_fills


def test_fills_with_affiliate_rev_share() -> None:
    """
    Test parsing the fills message with the new affiliateRevShare field.
    """
    # Arrange
    expected_num_fills = 1
    decoder = msgspec.json.Decoder(DYDXFillsResponse)

    # Act
    with Path("tests/test_data/dydx/http/v4_fills.json").open() as file_reader:
        fills_response = decoder.decode(file_reader.read())

    # Assert
    assert len(fills_response.fills) == expected_num_fills


def test_parse_to_position_status_report(
    perpetual_positions_response: DYDXPerpetualPositionsResponse,
) -> None:
    """
    Test the parse_to_position_status_report.
    """
    # Prepare
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = PositionStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        position_side=PositionSide.LONG,
        quantity=Quantity.from_str("0.003"),
        report_id=report_id,
        ts_init=1,
        ts_last=1722496165767000000,
    )

    # Act
    result = perpetual_positions_response.positions[0].parse_to_position_status_report(
        account_id=account_id,
        report_id=report_id,
        enum_parser=DYDXEnumParser(),
        size_precision=5,
        ts_init=1,
    )

    # Assert
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.position_side == expected_result.position_side
    assert result.quantity == expected_result.quantity
    assert result.id == expected_result.id
    assert result.ts_init == expected_result.ts_init
    assert result.ts_last == expected_result.ts_last
    assert result == expected_result


def test_parse_to_fill_report(fills_response: DYDXFillsResponse) -> None:
    """
    Test the parse_to_fill_report method.
    """
    # Prepare
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = FillReport(
        client_order_id=ClientOrderId("client-05009670-3fba-5ec7-8447-efb81a03cd9f"),
        venue_order_id=VenueOrderId("05009670-3fba-5ec7-8447-efb81a03cd9f"),
        trade_id=TradeId("55d7fc68-4b92-5c81-a73d-a3395e0124cb"),
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        order_side=OrderSide.BUY,
        last_qty=Quantity(0.002, 8),
        last_px=Price(3178.5, 7),
        commission=Money(Decimal("0.003179"), Currency.from_str("USDC")),
        liquidity_side=LiquiditySide.TAKER,
        report_id=report_id,
        ts_event=1722496165767000000,
        ts_init=1,
    )

    # Act
    result = fills_response.fills[0].parse_to_fill_report(
        account_id=account_id,
        client_order_id=ClientOrderId("client-05009670-3fba-5ec7-8447-efb81a03cd9f"),
        report_id=report_id,
        enum_parser=DYDXEnumParser(),
        size_precision=8,
        price_precision=7,
        ts_init=1,
    )

    # Assert
    assert result.client_order_id == expected_result.client_order_id
    assert result.venue_order_id == expected_result.venue_order_id
    assert result.trade_id == expected_result.trade_id
    assert result.account_id == expected_result.account_id
    assert result.instrument_id == expected_result.instrument_id
    assert result.order_side == expected_result.order_side
    assert result.last_qty == expected_result.last_qty
    assert result.last_qty.precision == expected_result.last_qty.precision
    assert result.last_px == expected_result.last_px
    assert result.last_px.precision == expected_result.last_px.precision
    assert result.liquidity_side == expected_result.liquidity_side
    assert result.commission == expected_result.commission
    assert result.id == expected_result.id
    assert result.ts_event == expected_result.ts_event
    assert result.ts_init == expected_result.ts_init
    assert result == expected_result


def test_order_parse_to_order_status_report(order_response: DYDXOrderResponse) -> None:
    """
    Test creating an order status report from an order message.
    """
    # Prepare
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("2043599281"),
        venue_order_id=VenueOrderId("05009670-3fba-5ec7-8447-efb81a03cd9f"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.FILLED,
        price=Price(3335.3, 4),
        quantity=Quantity(0.003, 5),
        filled_qty=Quantity(0.003, 5),
        avg_px=None,
        post_only=False,
        reduce_only=False,
        ts_last=1722496165767000000,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    result = order_response.parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("2043599281"),
        price_precision=4,
        size_precision=5,
        report_id=report_id,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
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
    assert result.trigger_price == expected_result.trigger_price
    assert result.trigger_type == expected_result.trigger_type
    assert result == expected_result


def test_order_parse_to_conditional_order_status_report(
    conditional_order_response: DYDXOrderResponse,
) -> None:
    """
    Test creating an order status report from an order message.
    """
    # Prepare
    report_id = UUID4()
    account_id = AccountId(f"{DYDX_VENUE.value}-001")
    expected_result = OrderStatusReport(
        account_id=account_id,
        instrument_id=DYDXSymbol("ETH-USD").to_instrument_id(),
        client_order_id=ClientOrderId("2043599281"),
        venue_order_id=VenueOrderId("05009670-3fba-5ec7-8447-efb81a03cd9f"),
        order_side=OrderSide.BUY,
        order_type=OrderType.STOP_LIMIT,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.FILLED,
        price=Price(3335.3, 4),
        trigger_price=Price(2791.9, 4),
        trigger_type=TriggerType.DEFAULT,
        quantity=Quantity(0.003, 5),
        filled_qty=Quantity(0.003, 5),
        avg_px=None,
        post_only=False,
        reduce_only=False,
        ts_last=1722496165767000000,
        report_id=report_id,
        ts_accepted=0,
        ts_init=1,
    )

    # Act
    result = conditional_order_response.parse_to_order_status_report(
        account_id=account_id,
        client_order_id=ClientOrderId("2043599281"),
        price_precision=4,
        size_precision=5,
        report_id=report_id,
        enum_parser=DYDXEnumParser(),
        ts_init=1,
    )

    # Assert
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
    assert result.trigger_price == expected_result.trigger_price
    assert result.trigger_type == expected_result.trigger_type
    assert result == expected_result


def test_list_perpetual_markets(
    list_perpetual_markets_response: DYDXListPerpetualMarketsResponse,
) -> None:
    """
    Test decoding the /v4/perpetualMarkets endpoint.
    """
    # Prepare
    expected_num_markets = 125

    # Assert
    assert len(list_perpetual_markets_response.markets) == expected_num_markets


def test_list_perpetual_markets_v8() -> None:
    """
    Test decoding the /v4/perpetualMarkets endpoint for protocol v8.
    """
    # Prepare
    expected_num_markets = 1
    decoder = msgspec.json.Decoder(DYDXListPerpetualMarketsResponse)

    # Act
    with Path("tests/test_data/dydx/http/list_perpetual_markets_v8.json").open() as file_reader:
        list_perpetual_markets_response = decoder.decode(file_reader.read())

    # Assert
    assert len(list_perpetual_markets_response.markets) == expected_num_markets
    assert list_perpetual_markets_response.markets["BTC-USD"].defaultFundingRate1H == "0"


def test_list_perpetual_markets_null_oracle_price() -> None:
    """
    Test decoding the /v4/perpetualMarkets endpoint with a null oracle price.
    """
    # Prepare
    expected_num_markets = 101
    decoder = msgspec.json.Decoder(DYDXListPerpetualMarketsResponse)

    # Act
    with Path(
        "tests/test_data/dydx/http/list_perpetual_markets_empty_oracle_price.json",
    ).open() as file_reader:
        list_perpetual_markets_response = decoder.decode(file_reader.read())

    # Assert
    assert len(list_perpetual_markets_response.markets) == expected_num_markets
    assert (
        list_perpetual_markets_response.markets[
            "BUFFI,UNISWAP_V3,0X4C1B1302220D7DE5C22B495E78B72F2DD2457D45-USD"
        ].oraclePrice
        is None
    )


def test_list_perpetual_markets_base_currency(
    list_perpetual_markets_response: DYDXListPerpetualMarketsResponse,
) -> None:
    """
    Test decoding the /v4/perpetualMarkets endpoint and creating the base currency.
    """
    # Prepare
    expected_result = Currency(
        code="BTC",
        name="BTC",
        currency_type=CurrencyType.CRYPTO,
        precision=8,
        iso4217=0,
    )

    # Act
    result = list_perpetual_markets_response.markets["BTC-USD"].parse_base_currency()

    # Assert
    assert result == expected_result
    assert result.precision == expected_result.precision
    assert result.name == expected_result.name
    assert result.code == expected_result.code
    assert result.currency_type == expected_result.currency_type


def test_list_perpetual_markets_quote_currency(
    list_perpetual_markets_response: DYDXListPerpetualMarketsResponse,
) -> None:
    """
    Test decoding the /v4/perpetualMarkets endpoint and retrieving the quote currency.
    """
    # Prepare
    expected_result = Currency(
        code="USDC",
        name="USDC",
        currency_type=CurrencyType.CRYPTO,
        precision=8,
        iso4217=0,
    )

    result = list_perpetual_markets_response.markets["BTC-USD"].parse_quote_currency()

    # Assert
    assert result == expected_result
    assert result.precision == expected_result.precision
    assert result.name == expected_result.name
    assert result.code == expected_result.code
    assert result.currency_type == expected_result.currency_type


def test_parse_to_instrument(
    list_perpetual_markets_response: DYDXListPerpetualMarketsResponse,
    instrument_id: InstrumentId,
) -> None:
    """
    Test creating a crypto instrument.
    """
    # Prepare
    base_currency = Currency(
        code="BTC",
        name="BTC",
        currency_type=CurrencyType.CRYPTO,
        precision=8,
        iso4217=0,
    )
    quote_currency = Currency(
        code="USD",
        name="USD",
        currency_type=CurrencyType.CRYPTO,
        precision=8,
        iso4217=0,
    )
    expected_result = CryptoPerpetual(
        instrument_id=instrument_id,
        raw_symbol=Symbol("BTC-USD"),
        base_currency=base_currency,
        quote_currency=quote_currency,
        settlement_currency=quote_currency,
        is_inverse=False,
        price_precision=0,
        size_precision=4,
        price_increment=Price(Decimal("1"), 0),
        size_increment=Quantity(Decimal("0.0001"), 4),
        max_quantity=None,
        min_quantity=None,
        max_notional=None,
        min_notional=None,
        max_price=None,
        min_price=None,
        margin_init=Decimal("0.05"),
        margin_maint=Decimal("0.03"),
        maker_fee=Decimal("0.00020"),
        taker_fee=Decimal("0.00050"),
        ts_event=0,
        ts_init=1,
        info={
            "atomicResolution": -10,
            "baseOpenInterest": "717.1169",
            "clobPairId": "0",
            "initialMarginFraction": "0.05",
            "maintenanceMarginFraction": "0.03",
            "marketType": "CROSS",
            "nextFundingRate": "0.00003494207317073171",
            "openInterest": "717.0347",
            "openInterestLowerCap": "0",
            "openInterestUpperCap": "0",
            "oraclePrice": "68526.44763",
            "priceChange24H": "1189.34763",
            "quantumConversionExponent": -9,
            "status": "ACTIVE",
            "stepBaseQuantums": 1000000,
            "stepSize": "0.0001",
            "subticksPerTick": 100000,
            "tickSize": "1",
            "ticker": "BTC-USD",
            "trades24H": 65786,
            "volume24H": "224881682.4025",
            "defaultFundingRate1H": None,
        },
    )

    # Act
    result = list_perpetual_markets_response.markets["BTC-USD"].parse_to_instrument(
        base_currency=base_currency,
        quote_currency=quote_currency,
        maker_fee=Decimal("0.00020"),
        taker_fee=Decimal("0.00050"),
        ts_event=0,
        ts_init=1,
    )

    # Assert
    assert result == expected_result
    assert result.maker_fee == expected_result.maker_fee
    assert result.taker_fee == expected_result.taker_fee
    assert result.margin_init == expected_result.margin_init
    assert result.margin_maint == expected_result.margin_maint
    assert result.price_precision == expected_result.price_precision
    assert result.price_increment == expected_result.price_increment
    assert result.size_increment == expected_result.size_increment
    assert result.size_precision == expected_result.size_precision
    assert result.base_currency == expected_result.base_currency
    assert result.quote_currency == expected_result.quote_currency
    assert result.ts_event == expected_result.ts_event
    assert result.ts_init == expected_result.ts_init
    assert result.info == expected_result.info
    assert result.raw_symbol == expected_result.raw_symbol
    assert result.id == expected_result.id
    assert result.is_inverse == expected_result.is_inverse
    assert result.max_quantity == expected_result.max_quantity
    assert result.min_quantity == expected_result.min_quantity
    assert result.max_notional == expected_result.max_notional
    assert result.min_notional == expected_result.min_notional
    assert result.margin_init == expected_result.margin_init
    assert result.margin_maint == expected_result.margin_maint
