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

import asyncio
from unittest.mock import AsyncMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest
from ibapi.contract import ContractDetails

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.config import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


def mock_ib_contract_calls(mocker, instrument_provider, contract_details: ContractDetails):
    mocker.patch.object(
        instrument_provider._client,
        "get_contract_details",
        side_effect=AsyncMock(return_value=[contract_details]),
    )


def make_instrument_provider(ib_client, *, load_ids=None, load_contracts=None):
    return InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=InteractiveBrokersInstrumentProviderConfig(
            load_ids=frozenset(load_ids or []),
            load_contracts=frozenset(load_contracts or []),
        ),
    )


@pytest.mark.asyncio
async def test_initialize_loads_configured_ids_and_contracts(ib_client):
    # Arrange
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    contract = IBContract(secType="STK", symbol="MSFT", exchange="NASDAQ")
    provider = make_instrument_provider(
        ib_client,
        load_ids=[instrument_id],
        load_contracts=[contract],
    )

    async def load(item, _filters):
        if isinstance(item, InstrumentId):
            return [item]
        return [InstrumentId.from_str("MSFT.NASDAQ")]

    provider.load_with_return_async = AsyncMock(side_effect=load)

    # Act
    await provider.initialize()

    # Assert
    assert provider._loaded is True
    assert {call.args[0] for call in provider.load_with_return_async.await_args_list} == {
        instrument_id,
        contract,
    }


@pytest.mark.asyncio
async def test_initialize_fails_closed_for_partial_load(ib_client):
    # Arrange
    loaded_id = InstrumentId.from_str("AAPL.NASDAQ")
    unresolved_id = InstrumentId.from_str("MSFT.NASDAQ")
    provider = make_instrument_provider(
        ib_client,
        load_ids=[loaded_id, unresolved_id],
    )

    async def load(item, _filters):
        return [item] if item == loaded_id else None

    provider.load_with_return_async = AsyncMock(side_effect=load)

    # Act / Assert
    with pytest.raises(RuntimeError, match=r"MSFT\.NASDAQ"):
        await provider.initialize()

    assert provider._loaded is False
    assert provider.load_with_return_async.await_count == 2


@pytest.mark.asyncio
async def test_initialize_can_retry_after_failed_required_load(ib_client):
    # Arrange
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    provider = make_instrument_provider(ib_client, load_ids=[instrument_id])
    provider.load_with_return_async = AsyncMock(side_effect=[None, [instrument_id]])

    # Act / Assert
    with pytest.raises(RuntimeError, match=r"AAPL\.NASDAQ"):
        await provider.initialize()

    await provider.initialize()

    assert provider._loaded is True
    assert provider.load_with_return_async.await_count == 2


@pytest.mark.asyncio
@pytest.mark.parametrize("error", [ConnectionError("Socket disconnected"), TimeoutError()])
async def test_initialize_propagates_client_transport_errors(ib_client, error):
    # Arrange
    ib_client._request_id_seq = 1
    ib_client._eclient.reqContractDetails = Mock()
    contract = IBContract(secType="STK", symbol="AAPL", exchange="NASDAQ")
    provider = make_instrument_provider(ib_client, load_contracts=[contract])

    # Act / Assert
    with (
        patch("asyncio.wait_for", side_effect=error),
        pytest.raises(type(error)),
    ):
        await provider.initialize()

    assert provider._loaded is False
    assert ib_client._requests.get(req_id=1) is None


@pytest.mark.asyncio
async def test_initialize_is_idempotent_and_serializes_concurrent_calls(ib_client):
    # Arrange
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    provider = make_instrument_provider(ib_client, load_ids=[instrument_id])

    async def load(item, _filters):
        await asyncio.sleep(0)
        return [item]

    provider.load_with_return_async = AsyncMock(side_effect=load)

    # Act
    await asyncio.gather(provider.initialize(), provider.initialize())
    await provider.initialize()

    # Assert
    assert provider._loaded is True
    provider.load_with_return_async.assert_awaited_once_with(instrument_id, None)


@pytest.mark.asyncio
async def test_data_client_registers_only_after_instrument_initialization(data_client):
    # Arrange
    data_client._client.wait_until_ready = AsyncMock()
    data_client._client.set_market_data_type = AsyncMock()
    data_client.instrument_provider.initialize = AsyncMock(
        side_effect=RuntimeError("Required instrument did not load"),
    )

    # Act / Assert
    with pytest.raises(RuntimeError, match="Required instrument did not load"):
        await data_client._connect()

    assert data_client.id not in data_client._client.registered_nautilus_clients

    data_client.instrument_provider.initialize = AsyncMock()
    await data_client._connect()

    assert data_client.id in data_client._client.registered_nautilus_clients
    data_client._client.registered_nautilus_clients.discard(data_client.id)


@pytest.mark.asyncio
async def test_load_equity_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.aapl_equity_contract_details(),
    )

    # Act
    await instrument_provider.load_async(
        IBContract(secType="STK", symbol="AAPL", exchange="NASDAQ"),
    )
    equity = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")) == equity.id
    assert equity.asset_class == AssetClass.EQUITY
    assert equity.instrument_class == InstrumentClass.SPOT
    assert equity.multiplier == 1
    assert Price.from_str("0.01") == equity.price_increment
    assert 2, equity.price_precision


@pytest.mark.asyncio
async def test_load_futures_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("CLZ3.NYMEX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.cl_future_contract_details(),
    )

    # Act
    await instrument_provider.load_async(IBContract(secType="FUT", symbol="CLZ3", exchange="NYMEX"))
    future = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert future.id == instrument_id
    assert future.asset_class == AssetClass.INDEX
    assert future.multiplier == 1000
    assert future.price_increment == Price.from_str("0.01")
    assert future.price_precision == 2


@pytest.mark.asyncio
async def test_load_option_contract_instrument(mocker, instrument_provider):
    # Arrange
    # OCC format preserves space padding between symbol and expiry
    instrument_id = InstrumentId.from_str("TSLA  230120C00100000.MIAX")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.tsla_option_contract_details(),
    )

    # Act
    await instrument_provider.load_async(
        IBContract(secType="OPT", symbol="TSLA  230120C00100000", exchange="MIAX"),
    )
    option = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert option.id == instrument_id
    assert option.asset_class == AssetClass.EQUITY
    assert option.multiplier == 100
    assert option.expiration_ns == 1674248400000000000
    assert option.strike_price == Price.from_str("100.0")
    assert option.option_kind == OptionKind.CALL
    assert option.price_increment == Price.from_str("0.01")
    assert option.price_precision == 2


@pytest.mark.asyncio
async def test_load_forex_contract_instrument(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.eurusd_forex_contract_details(),
    )

    # Act
    await instrument_provider.load_async(instrument_id)
    fx = instrument_provider.find(instrument_id)
    instrument_provider._client.stop()

    # Assert
    assert fx.id == instrument_id
    assert fx.asset_class == AssetClass.FX
    assert fx.multiplier == 1
    assert fx.price_increment == Price.from_str("0.00005")
    assert fx.price_precision == 5


@pytest.mark.asyncio
async def test_contract_id_to_instrument_id(mocker, instrument_provider):
    # Arrange
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.cl_future_contract_details(),
    )

    # Act
    await instrument_provider.load_async(IBContract(secType="FUT", symbol="CLZ3", exchange="NYMEX"))
    instrument_provider._client.stop()

    # Assert
    expected = {174230596: InstrumentId.from_str("CLZ3.NYMEX")}
    assert instrument_provider.contract_id_to_instrument_id == expected


@pytest.mark.asyncio
async def test_load_instrument_using_contract_id(mocker, instrument_provider):
    # Arrange
    instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
    mock_ib_contract_calls(
        mocker=mocker,
        instrument_provider=instrument_provider,
        contract_details=IBTestContractStubs.eurusd_forex_contract_details(),
    )

    # Act
    fx = await instrument_provider.get_instrument(IBContract(conId=12087792))
    instrument_provider._client.stop()

    # Assert
    assert fx.id == instrument_id
    assert fx.asset_class == AssetClass.FX
    assert fx.multiplier == 1
    assert fx.price_increment == Price.from_str("0.00005")
    assert fx.price_precision == 5


@pytest.mark.asyncio
async def test_bag_contract_loading_invalid_no_combo_legs(instrument_provider):
    """
    Test that loading BAG contract without combo legs raises error.
    """
    # Arrange
    bag_contract = IBContract(
        conId=12345,
        secType="BAG",
        symbol="ES",
        exchange="SMART",
        currency="USD",
    )

    # Act & Assert
    with pytest.raises(ValueError, match="Invalid BAG contract"):
        await instrument_provider._load_bag_contract(bag_contract)


@pytest.mark.asyncio
async def test_bag_contract_venue_determination(instrument_provider):
    """
    Test venue determination for BAG contracts.
    """
    # Test with SMART exchange (should use primaryExchange)
    bag_contract_smart = IBContract(
        conId=12345,
        secType="BAG",
        symbol="ES",
        exchange="SMART",
        primaryExchange="CME",
        currency="USD",
    )

    # Test with direct exchange
    bag_contract_direct = IBContract(
        conId=12346,
        secType="BAG",
        symbol="SPY",
        exchange="ARCA",
        currency="USD",
    )

    # Act
    venue_smart = instrument_provider.determine_venue_from_contract(bag_contract_smart)
    venue_direct = instrument_provider.determine_venue_from_contract(bag_contract_direct)

    # Assert
    assert venue_smart == "CME"  # Should use primaryExchange when exchange is SMART
    assert venue_direct == "ARCA"  # Should use exchange directly


@pytest.mark.asyncio
async def test_determine_venue_from_contract_opt_smart_uses_symbol_to_mic_venue(ib_client):
    """
    When _symbol_to_mic_venue is configured, OPT contract with exchange SMART returns
    the symbol-specific MIC venue (e.g. SPX -> XCBO).
    """
    from nautilus_trader.common.component import LiveClock

    config = InteractiveBrokersInstrumentProviderConfig(
        symbol_to_mic_venue={"SPX": "XCBO"},
    )
    provider = InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=config,
    )
    contract = IBContract(
        secType="OPT",
        symbol="SPX",
        exchange="SMART",
        localSymbol="SPXW  260120P06835000",
        currency="USD",
    )
    venue = provider.determine_venue_from_contract(contract)
    assert venue == "XCBO"


@pytest.mark.asyncio
async def test_determine_venue_from_contract_opt_smart_uses_first_non_smart_from_details(
    instrument_provider,
):
    """
    When OPT has exchange SMART and no primaryExchange, passing contract_details with
    validExchanges (e.g. SMART,CBOE) yields first non-SMART as exchange, then venue CBOE
    when convert_exchange_to_mic_venue is False.
    """
    contract = IBContract(
        secType="OPT",
        symbol="SPX",
        exchange="SMART",
        localSymbol="SPXW  260120P06835000",
        currency="USD",
    )
    details = IBContractDetails(
        contract=contract,
        validExchanges="SMART,CBOE",
        minTick=0.01,
    )
    venue = instrument_provider.determine_venue_from_contract(
        contract,
        contract_details=details,
    )
    assert venue == "CBOE"


def test_determine_venue_from_contract_stk_uses_primary_exchange_over_fill_exchange(
    instrument_provider,
):
    contract = IBContract(
        secType="STK",
        symbol="META",
        exchange="IBEOS",
        primaryExchange="NASDAQ",
        currency="USD",
    )

    venue = instrument_provider.determine_venue_from_contract(contract)

    assert venue == "NASDAQ"


def test_determine_venue_from_contract_stk_reuses_cached_mic_venue_over_primary_exchange(
    ib_client,
):
    from nautilus_trader.common.component import LiveClock

    provider = InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=InteractiveBrokersInstrumentProviderConfig(
            convert_exchange_to_mic_venue=True,
        ),
    )
    provider._process_contract_details(
        [IBTestContractStubs.aapl_equity_contract_details()],
    )
    contract = IBContract(
        secType="STK",
        symbol="AAPL",
        exchange="IBEOS",
        primaryExchange="NASDAQ",
        currency="USD",
    )

    venue = provider.determine_venue_from_contract(contract)

    assert venue == "XNAS"


@pytest.mark.parametrize(
    "valid_exchanges",
    [
        "SMART,AMEX,NYSE,CBOE,PSX,ARCA",
        "SMART,ARCA,AMEX,NYSE",
    ],
)
def test_determine_venue_from_contract_stk_smart_reuses_cached_symbol_venue(
    instrument_provider,
    valid_exchanges,
):
    instrument_provider._process_contract_details(
        [IBTestContractStubs.aapl_equity_contract_details()],
    )

    contract = IBContract(
        secType="STK",
        symbol="AAPL",
        exchange="SMART",
        primaryExchange="",
        currency="USD",
    )
    details = IBContractDetails(
        contract=contract,
        validExchanges=valid_exchanges,
        minTick=0.01,
    )

    venue = instrument_provider.determine_venue_from_contract(
        contract,
        contract_details=details,
    )

    assert venue == "NASDAQ"


@pytest.mark.asyncio
async def test_determine_venue_from_contract_opt_smart_maps_to_mic_when_convert_enabled(
    ib_client,
):
    """
    When convert_exchange_to_mic_venue is True and OPT SMART gets exchange from
    validExchanges (first non-SMART), venue is mapped to MIC via VENUE_MEMBERS (e.g.
    CBOE -> XCBO).
    """
    from nautilus_trader.common.component import LiveClock

    config = InteractiveBrokersInstrumentProviderConfig(
        convert_exchange_to_mic_venue=True,
    )
    provider = InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=config,
    )
    contract = IBContract(
        secType="OPT",
        symbol="SPX",
        exchange="SMART",
        localSymbol="SPXW  260120P06835000",
        currency="USD",
    )
    details = IBContractDetails(
        contract=contract,
        validExchanges="SMART,CBOE",
        minTick=0.01,
    )
    venue = provider.determine_venue_from_contract(
        contract,
        contract_details=details,
    )
    assert venue == "XCBO"


def test_process_contract_details_resolves_venue_per_detail_when_not_provided(ib_client):
    from nautilus_trader.common.component import LiveClock

    provider = InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=InteractiveBrokersInstrumentProviderConfig(
            convert_exchange_to_mic_venue=True,
        ),
    )

    processed_ids = provider._process_contract_details(
        [
            IBTestContractStubs.aapl_equity_contract_details(),
            IBTestContractStubs.cl_future_contract_details(),
        ],
    )

    assert processed_ids == [
        InstrumentId.from_str("AAPL.XNAS"),
        InstrumentId.from_str("CLZ3.XNYM"),
    ]


def test_process_contract_details_uses_explicit_venue_when_provided(ib_client):
    """
    When venue is passed, that venue is used for all details (no per-detail resolution).
    """
    from nautilus_trader.common.component import LiveClock

    provider = InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=InteractiveBrokersInstrumentProviderConfig(
            convert_exchange_to_mic_venue=True,
        ),
    )

    processed_ids = provider._process_contract_details(
        [
            IBTestContractStubs.aapl_equity_contract_details(),
            IBTestContractStubs.cl_future_contract_details(),
        ],
        venue="XNAS",
    )

    assert processed_ids == [
        InstrumentId.from_str("AAPL.XNAS"),
        InstrumentId.from_str("CLZ3.XNAS"),
    ]


def test_determine_venue_from_contract_symbol_to_mic_venue_without_convert_exchange(ib_client):
    """
    symbol_to_mic_venue is applied regardless of convert_exchange_to_mic_venue.
    """
    from nautilus_trader.common.component import LiveClock

    config = InteractiveBrokersInstrumentProviderConfig(
        symbol_to_mic_venue={"SPX": "XCBO"},
        convert_exchange_to_mic_venue=False,
    )
    provider = InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=config,
    )
    contract = IBContract(
        secType="OPT",
        symbol="SPX",
        exchange="SMART",
        localSymbol="SPXW  260120P06835000",
        currency="USD",
    )
    venue = provider.determine_venue_from_contract(contract)
    assert venue == "XCBO"


@pytest.mark.asyncio
async def test_create_bag_contract_with_explicit_exchange(instrument_provider):
    """
    Test that _create_bag_contract uses explicit exchange parameter when provided.
    """
    from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails

    # Arrange - Create mock leg contract details
    leg1_contract = IBContract(
        secType="FUT",
        symbol="ES",
        conId=100,
        exchange="CME",
        currency="USD",
        multiplier="50",
    )
    leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.25, underSymbol="ES")

    leg2_contract = IBContract(
        secType="FUT",
        symbol="ES",
        conId=101,
        exchange="CME",
        currency="USD",
        multiplier="50",
    )
    leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.25, underSymbol="ES")

    leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]
    instrument_id = None

    # Act - Create BAG contract with explicit exchange
    bag_contract = await instrument_provider._create_bag_contract(
        leg_contract_details=leg_contract_details,
        instrument_id=instrument_id,
        exchange="CME",  # Explicit exchange
    )

    # Assert
    assert bag_contract.exchange == "CME"
    assert bag_contract.secType == "BAG"
    assert bag_contract.symbol == "ES"
    assert bag_contract.currency == "USD"
    assert len(bag_contract.comboLegs) == 2


@pytest.mark.asyncio
async def test_create_bag_contract_defaults_to_smart(instrument_provider):
    """
    Test that _create_bag_contract defaults to SMART exchange when not provided.
    """
    from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails

    # Arrange - Create mock leg contract details
    leg1_contract = IBContract(
        secType="FUT",
        symbol="ES",
        conId=100,
        exchange="CME",
        currency="USD",
        multiplier="50",
    )
    leg1_details = IBContractDetails(contract=leg1_contract, minTick=0.25, underSymbol="ES")

    leg2_contract = IBContract(
        secType="FUT",
        symbol="ES",
        conId=101,
        exchange="CME",
        currency="USD",
        multiplier="50",
    )
    leg2_details = IBContractDetails(contract=leg2_contract, minTick=0.25, underSymbol="ES")

    leg_contract_details = [(leg1_details, 1), (leg2_details, -1)]
    instrument_id = None

    # Act - Create BAG contract without exchange (empty string should default to SMART)
    bag_contract = await instrument_provider._create_bag_contract(
        leg_contract_details=leg_contract_details,
        instrument_id=instrument_id,
        exchange="",  # Empty string should default to SMART
    )

    # Assert
    assert bag_contract.exchange == "SMART"
    assert bag_contract.secType == "BAG"
    assert bag_contract.symbol == "ES"
    assert bag_contract.currency == "USD"
    assert len(bag_contract.comboLegs) == 2
