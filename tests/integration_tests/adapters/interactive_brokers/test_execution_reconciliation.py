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
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from ibapi.const import UNSET_DOUBLE

from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import new_generic_spread_id
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.instruments import OptionSpread
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs


def instrument_setup(exec_client, cache, instrument=None, contract_details=None):
    instrument = instrument or IBTestContractStubs.aapl_instrument()
    contract_details = contract_details or IBTestContractStubs.aapl_equity_contract_details()
    exec_client._instrument_provider.contract_details[instrument.id] = contract_details
    exec_client._instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    exec_client._instrument_provider.add(instrument)
    cache.add_instrument(instrument)


@pytest.mark.asyncio
async def test_generate_position_status_reports_with_zero_quantity(exec_client, cache):
    """
    Test that zero-quantity positions generate FLAT PositionStatusReport.

    Verifies fix for issue #3023 where IB adapter should emit FLAT reports when
    positions are closed externally.

    """
    # Arrange
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    zero_position = IBPosition(
        account_id="DU123456",
        contract=IBTestContractStubs.aapl_equity_ib_contract(),
        quantity=Decimal(0),
        avg_cost=100.0,
    )

    exec_client._client.get_positions = AsyncMock(return_value=[zero_position])

    command = GeneratePositionStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    assert reports[0].position_side == PositionSide.FLAT
    assert reports[0].quantity.as_decimal() == Decimal(0)
    assert reports[0].instrument_id == instrument.id


@pytest.mark.asyncio
async def test_generate_position_status_reports_flat_when_no_positions(exec_client, cache):
    """
    Test that FLAT report is generated when specific instrument has no positions.

    Verifies fix for issue #3023 where reconciliation requests position for specific
    instrument but IB returns no positions.

    """
    # Arrange
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    exec_client._client.get_positions = AsyncMock(return_value=None)

    command = GeneratePositionStatusReports(
        instrument_id=instrument.id,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_position_status_reports(command)

    # Assert
    assert len(reports) == 1
    assert reports[0].position_side == PositionSide.FLAT
    assert reports[0].quantity.as_decimal() == Decimal(0)
    assert reports[0].instrument_id == instrument.id


@pytest.mark.asyncio
async def test_generate_order_status_reports_parses_trailing_stop_market_fields(
    exec_client,
    cache,
):
    """
    Test that trailing stop reconciliation preserves the trailing offset fields.

    Verifies fix for IB live restart reconciliation where open TRAIL orders crashed
    during unpacking because the OrderStatusReport omitted trailing_offset.

    """
    # Arrange
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    ib_order = IBTestExecStubs.aapl_buy_ib_order(total_quantity="5")
    ib_order.contract = IBTestContractStubs.aapl_equity_ib_contract()
    ib_order.orderType = "TRAIL"
    ib_order.tif = "GTC"
    ib_order.lmtPrice = UNSET_DOUBLE
    ib_order.auxPrice = 2.5
    ib_order.trailStopPrice = 185.5
    ib_order.filledQuantity = Decimal(0)
    ib_order.order_state = SimpleNamespace(status="Submitted")

    exec_client._client.get_open_orders = AsyncMock(return_value=[ib_order])

    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=True,
        command_id=UUID4(),
        ts_init=0,
    )

    # Act
    reports = await exec_client.generate_order_status_reports(command)

    # Assert
    assert len(reports) == 1
    assert reports[0].order_type == OrderType.TRAILING_STOP_MARKET
    assert reports[0].quantity.as_decimal() == Decimal(5)
    assert reports[0].price is None
    assert reports[0].trigger_price == Price.from_str("185.50")
    assert reports[0].trailing_offset == Decimal("2.5")
    assert reports[0].trailing_offset_type == TrailingOffsetType.PRICE
    assert reports[0].limit_offset is None


@pytest.mark.asyncio
@pytest.mark.parametrize(
    (
        "ib_order_type",
        "lmt_price",
        "aux_price",
        "trail_stop_price",
        "lmt_price_offset",
        "expected_order_type",
        "expected_price",
        "expected_trigger_price",
        "expected_limit_offset",
        "expected_trailing_offset",
        "expected_trailing_offset_type",
    ),
    [
        (
            "MKT",
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            OrderType.MARKET,
            None,
            None,
            None,
            None,
            None,
        ),
        (
            "LMT",
            185.5,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            OrderType.LIMIT,
            Price.from_str("185.50"),
            None,
            None,
            None,
            None,
        ),
        (
            "MIT",
            UNSET_DOUBLE,
            180.25,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            OrderType.MARKET_IF_TOUCHED,
            None,
            Price.from_str("180.25"),
            None,
            None,
            None,
        ),
        (
            "LIT",
            179.5,
            180.25,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            OrderType.LIMIT_IF_TOUCHED,
            Price.from_str("179.50"),
            Price.from_str("180.25"),
            None,
            None,
            None,
        ),
        (
            "STP",
            UNSET_DOUBLE,
            180.25,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            OrderType.STOP_MARKET,
            None,
            Price.from_str("180.25"),
            None,
            None,
            None,
        ),
        (
            "STP LMT",
            179.5,
            180.25,
            UNSET_DOUBLE,
            UNSET_DOUBLE,
            OrderType.STOP_LIMIT,
            Price.from_str("179.50"),
            Price.from_str("180.25"),
            None,
            None,
            None,
        ),
        (
            "TRAIL LIMIT",
            UNSET_DOUBLE,
            2.5,
            185.5,
            0.25,
            OrderType.TRAILING_STOP_LIMIT,
            None,
            Price.from_str("185.50"),
            Decimal("0.25"),
            Decimal("2.5"),
            TrailingOffsetType.PRICE,
        ),
    ],
)
async def test_parse_ib_order_to_order_status_report_maps_pricing_fields_by_order_type(
    exec_client,
    cache,
    ib_order_type,
    lmt_price,
    aux_price,
    trail_stop_price,
    lmt_price_offset,
    expected_order_type,
    expected_price,
    expected_trigger_price,
    expected_limit_offset,
    expected_trailing_offset,
    expected_trailing_offset_type,
):
    # Arrange
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    ib_order = IBTestExecStubs.aapl_buy_ib_order(total_quantity="5")
    ib_order.contract = IBTestContractStubs.aapl_equity_ib_contract()
    ib_order.orderType = ib_order_type
    ib_order.tif = "GTC"
    ib_order.lmtPrice = lmt_price
    ib_order.auxPrice = aux_price
    ib_order.trailStopPrice = trail_stop_price
    ib_order.lmtPriceOffset = lmt_price_offset
    ib_order.filledQuantity = Decimal(0)
    ib_order.order_state = SimpleNamespace(status="Submitted")

    # Act
    report = await exec_client._parse_ib_order_to_order_status_report(ib_order)

    # Assert
    assert report.order_type == expected_order_type
    assert report.price == expected_price
    assert report.trigger_price == expected_trigger_price
    assert report.limit_offset == expected_limit_offset
    assert report.trailing_offset == expected_trailing_offset
    assert report.trailing_offset_type == expected_trailing_offset_type


# ---------------------------------------------------------------------------
# Helpers for spread pre-load tests
# ---------------------------------------------------------------------------


def _make_option_contract(symbol_str: str, venue_str: str, kind: OptionKind) -> OptionContract:
    return OptionContract(
        instrument_id=InstrumentId(Symbol(symbol_str), Venue(venue_str)),
        raw_symbol=Symbol(symbol_str),
        asset_class=AssetClass.EQUITY,
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        underlying="SPY",
        option_kind=kind,
        activation_ns=0,
        expiration_ns=1640995200000000000,
        strike_price=Price.from_str("400.0")
        if kind == OptionKind.CALL
        else Price.from_str("390.0"),
        ts_event=0,
        ts_init=0,
    )


def _make_spread(call: OptionContract, put: OptionContract) -> OptionSpread:
    spread_id = new_generic_spread_id([(call.id, 1), (put.id, -1)])
    return OptionSpread(
        instrument_id=spread_id,
        raw_symbol=spread_id.symbol,
        asset_class=AssetClass.EQUITY,
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        underlying="SPY",
        strategy_type="VERTICAL",
        activation_ns=0,
        expiration_ns=1640995200000000000,
        ts_event=0,
        ts_init=0,
    )


def _make_spread_order(spread: OptionSpread, tag: str = "001") -> MarketOrder:
    return MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=spread.id,
        client_order_id=ClientOrderId(f"O-TEST-{tag}"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(1),
        time_in_force=TimeInForce.DAY,
        init_id=UUID4(),
        ts_init=0,
    )


# ---------------------------------------------------------------------------
# Tests for _preload_spread_instruments
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_preload_spread_instruments_loads_from_cached_orders(exec_client, cache):
    """
    Test that spread instruments referenced by cached orders are pre-loaded into the
    instrument provider via _fetch_spread_instrument.

    Verifies fix for issue #3752 where spread instruments are not loaded on restart,
    causing 'instrument not found' errors during reconciliation.

    """
    # Arrange
    call = _make_option_contract("SPY C400", "SMART", OptionKind.CALL)
    put = _make_option_contract("SPY P390", "SMART", OptionKind.PUT)
    spread = _make_spread(call, put)

    # Add a spread order to the cache (simulates restart with cached orders)
    order = _make_spread_order(spread)
    cache.add_instrument(spread)  # needed for cache.add_order to accept instrument_id
    cache.add_order(order, None)
    # Remove the spread from the provider so it needs to be re-loaded
    # (simulates fresh provider on restart)
    exec_client._instrument_provider._instruments.pop(spread.id, None)

    # Mock _fetch_spread_instrument to track the call
    exec_client.instrument_provider._fetch_spread_instrument = AsyncMock(return_value=True)

    # Act
    await exec_client._preload_spread_instruments()

    # Assert
    exec_client.instrument_provider._fetch_spread_instrument.assert_called_once_with(spread.id)


@pytest.mark.asyncio
async def test_preload_spread_instruments_skips_already_loaded(exec_client, cache):
    """
    Test that spread instruments already in the provider are not re-loaded.
    """
    # Arrange
    call = _make_option_contract("SPY C400", "SMART", OptionKind.CALL)
    put = _make_option_contract("SPY P390", "SMART", OptionKind.PUT)
    spread = _make_spread(call, put)

    order = _make_spread_order(spread)
    cache.add_instrument(spread)
    cache.add_order(order, None)

    # Keep the spread in the provider (already loaded)
    exec_client._instrument_provider.add(spread)

    exec_client.instrument_provider._fetch_spread_instrument = AsyncMock(return_value=True)

    # Act
    await exec_client._preload_spread_instruments()

    # Assert - should not call fetch since the instrument is already loaded
    exec_client.instrument_provider._fetch_spread_instrument.assert_not_called()


@pytest.mark.asyncio
async def test_preload_spread_instruments_noop_when_no_orders(exec_client, cache):
    """
    Test that pre-loading is a no-op when cache has no orders.
    """
    # Arrange
    exec_client.instrument_provider._fetch_spread_instrument = AsyncMock(return_value=True)

    # Act
    await exec_client._preload_spread_instruments()

    # Assert
    exec_client.instrument_provider._fetch_spread_instrument.assert_not_called()


@pytest.mark.asyncio
async def test_preload_spread_instruments_ignores_non_spread_orders(exec_client, cache):
    """
    Test that non-spread orders in the cache are ignored during pre-loading.
    """
    # Arrange - add a regular (non-spread) instrument and order
    instrument = IBTestContractStubs.aapl_instrument()
    instrument_setup(exec_client, cache, instrument=instrument)

    from nautilus_trader.test_kit.stubs.execution import TestExecStubs

    order = TestExecStubs.limit_order(instrument=instrument)
    cache.add_order(order, None)

    exec_client.instrument_provider._fetch_spread_instrument = AsyncMock(return_value=True)

    # Act
    await exec_client._preload_spread_instruments()

    # Assert
    exec_client.instrument_provider._fetch_spread_instrument.assert_not_called()


@pytest.mark.asyncio
async def test_preload_spread_instruments_handles_fetch_failure_gracefully(exec_client, cache):
    """
    Test that a failure to fetch one spread instrument does not prevent others from
    being loaded and does not raise an exception.
    """
    # Arrange
    call1 = _make_option_contract("SPY C400", "SMART", OptionKind.CALL)
    put1 = _make_option_contract("SPY P390", "SMART", OptionKind.PUT)
    spread1 = _make_spread(call1, put1)

    call2 = _make_option_contract("QQQ C350", "SMART", OptionKind.CALL)
    put2 = _make_option_contract("QQQ P340", "SMART", OptionKind.PUT)
    spread2 = _make_spread(call2, put2)

    for spread in [spread1, spread2]:
        cache.add_instrument(spread)
        order = _make_spread_order(spread, tag=spread.id.symbol.value[:8])
        cache.add_order(order, None)
        # Remove from provider to force re-loading
        exec_client._instrument_provider._instruments.pop(spread.id, None)

    # First call raises, second succeeds
    exec_client.instrument_provider._fetch_spread_instrument = AsyncMock(
        side_effect=[Exception("IB connection lost"), True],
    )

    # Act - should not raise
    await exec_client._preload_spread_instruments()

    # Assert - both spread IDs were attempted
    assert exec_client.instrument_provider._fetch_spread_instrument.call_count == 2


@pytest.mark.asyncio
async def test_preload_spread_instruments_deduplicates_across_orders(exec_client, cache):
    """
    Test that multiple orders referencing the same spread instrument only trigger a
    single fetch.
    """
    # Arrange
    call = _make_option_contract("SPY C400", "SMART", OptionKind.CALL)
    put = _make_option_contract("SPY P390", "SMART", OptionKind.PUT)
    spread = _make_spread(call, put)

    cache.add_instrument(spread)

    # Add two different orders for the same spread
    order1 = _make_spread_order(spread, tag="001")
    order2 = _make_spread_order(spread, tag="002")
    cache.add_order(order1, None)
    cache.add_order(order2, None)

    # Remove from provider
    exec_client._instrument_provider._instruments.pop(spread.id, None)

    exec_client.instrument_provider._fetch_spread_instrument = AsyncMock(return_value=True)

    # Act
    await exec_client._preload_spread_instruments()

    # Assert - only one fetch despite two orders
    exec_client.instrument_provider._fetch_spread_instrument.assert_called_once_with(spread.id)
