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

from __future__ import annotations

import asyncio
import sys
from collections.abc import Mapping
from datetime import timedelta
from datetime import timezone
from types import SimpleNamespace
from typing import Any
from unittest.mock import MagicMock

import pandas as pd
import pytest

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


pytestmark = pytest.mark.skipif(
    sys.version_info >= (3, 14),
    reason="Interactive Brokers adapter requires Python < 3.14 (nautilus-ibapi incompatibility)",
)


def _make_instrument(instrument_id: InstrumentId) -> Instrument:
    instrument = MagicMock(spec=Instrument)
    instrument.id = instrument_id
    return instrument


class _StubConfig:
    def __init__(
        self,
        load_ids: list[InstrumentId] | None = None,
        load_contracts: list[IBContract | Mapping[str, Any]] | None = None,
        filter_sec_types: frozenset[str] | None = None,
    ) -> None:
        self.load_all = False
        self.load_ids = load_ids
        self.filters = None
        self.load_contracts = load_contracts or []
        self.filter_sec_types = filter_sec_types or frozenset()


class _FakeRustProvider:
    def __init__(self, _config: Any) -> None:
        self._instruments: dict[InstrumentId, Instrument] = {}
        self._details: dict[InstrumentId, SimpleNamespace] = {}
        self._contract_id_to_instrument_id: dict[int, InstrumentId] = {}

    async def initialize(self) -> None:
        return None

    def get_all(self) -> list[Instrument]:
        return list(self._instruments.values())

    def find_by_contract_id(self, contract_id: int) -> Instrument | None:
        instrument_id = self._contract_id_to_instrument_id.get(contract_id)
        if instrument_id is None:
            return None
        return self._instruments.get(instrument_id)

    def get_instrument_id_by_contract_id(self, contract_id: int) -> InstrumentId | None:
        return self._contract_id_to_instrument_id.get(contract_id)

    def get_price_magnifier(self, _instrument_id: InstrumentId) -> int:
        return 10

    def determine_venue(self, _contract: Any) -> str:
        return "XCBO"

    def instrument_id_to_ib_contract_details(
        self,
        instrument_id: InstrumentId,
    ) -> SimpleNamespace | None:
        return self._details.get(instrument_id)


class _FakeRustDataClient:
    def __init__(self, rust_provider: _FakeRustProvider) -> None:
        self._provider = rust_provider
        self.option_chain_calls: list[tuple[dict[str, Any], str | None, str | None]] = []
        self.futures_chain_calls: list[
            tuple[str, str | None, str | None, int | None, int | None]
        ] = []

    async def load_with_return_async(
        self,
        instrument_id: InstrumentId,
        _filters: Any,
    ) -> InstrumentId:
        self._store_instrument(instrument_id, 101)
        return instrument_id

    async def load_ids_with_return_async(
        self,
        instrument_ids: list[InstrumentId],
        _filters: Any,
    ) -> list[InstrumentId]:
        for index, instrument_id in enumerate(instrument_ids, start=1):
            self._store_instrument(instrument_id, 200 + index)
        return instrument_ids

    async def load_all_async(
        self,
        instrument_ids: list[InstrumentId] | None,
        contracts: list[Mapping[str, Any]] | None,
        _force_instrument_update: bool,
    ) -> list[InstrumentId]:
        loaded_ids: list[InstrumentId] = []

        for index, instrument_id in enumerate(instrument_ids or [], start=1):
            self._store_instrument(instrument_id, 300 + index)
            loaded_ids.append(instrument_id)

        for index, contract in enumerate(contracts or [], start=1):
            ib_contract = _as_ib_contract(contract, 400 + index)
            instrument_id = InstrumentId.from_str(
                f"{ib_contract.symbol}.{ib_contract.exchange}",
            )
            self._store_instrument(instrument_id, 400 + index, contract=ib_contract)
            loaded_ids.append(instrument_id)

        return loaded_ids

    async def get_instrument(self, contract: Mapping[str, Any]) -> Instrument:
        ib_contract = _as_ib_contract(contract, 501)
        instrument_id = InstrumentId.from_str(
            f"{ib_contract.symbol}.{ib_contract.exchange}",
        )
        self._store_instrument(instrument_id, 501, contract=ib_contract)
        return self._provider._instruments[instrument_id]

    async def py_fetch_option_chain_by_range_for_contract(
        self,
        contract: dict[str, Any],
        expiry_min: str | None = None,
        expiry_max: str | None = None,
    ) -> int:
        self.option_chain_calls.append(
            (contract, expiry_min, expiry_max),
        )
        return 0

    async def py_fetch_futures_chain(
        self,
        symbol: str,
        exchange: str | None = None,
        currency: str | None = None,
        min_expiry_days: int | None = None,
        max_expiry_days: int | None = None,
    ) -> int:
        instrument_id = InstrumentId.from_str(f"{symbol}M6.{exchange or 'CME'}")
        contract = IBContract(
            secType="FUT",
            conId=600,
            symbol=symbol,
            localSymbol=f"{symbol}M6",
            exchange=exchange or "CME",
            currency=currency or "USD",
        )
        self._store_instrument(instrument_id, 600, contract=contract)
        self.futures_chain_calls.append(
            (symbol, exchange, currency, min_expiry_days, max_expiry_days),
        )
        return 0

    def _store_instrument(
        self,
        instrument_id: InstrumentId,
        contract_id: int,
        contract: IBContract | None = None,
    ) -> None:
        instrument = _make_instrument(instrument_id)
        contract = contract or IBContract(
            secType="STK",
            conId=contract_id,
            symbol=instrument_id.symbol.value,
            exchange=instrument_id.venue.value,
        )
        details = SimpleNamespace(contract=contract, priceMagnifier=10)
        self._provider._instruments[instrument_id] = instrument
        self._provider._details[instrument_id] = details
        self._provider._contract_id_to_instrument_id[contract.conId] = instrument_id


def _as_ib_contract(contract: IBContract | Mapping, default_con_id: int) -> IBContract:
    if isinstance(contract, IBContract):
        return contract

    payload = dict(contract)
    payload.setdefault("conId", default_con_id)
    return IBContract(**payload)


class _FakePyO3InstrumentId:
    @staticmethod
    def from_str(value: str) -> str:
        return f"pyo3:{value}"


class _FakeHistoricalRustClient:
    def __init__(self, _provider: Any, _config: Any) -> None:
        self.request_instruments_kwargs: dict[str, Any] | None = None
        self.request_bars_kwargs: dict[str, Any] | None = None
        self.request_ticks_kwargs: dict[str, Any] | None = None

    async def request_instruments(self, **kwargs: Any) -> list[Any]:
        self.request_instruments_kwargs = kwargs
        return [SimpleNamespace(kind="instrument")]

    async def request_bars(self, **kwargs: Any) -> list[Any]:
        self.request_bars_kwargs = kwargs
        return ["raw-bar"]

    async def request_ticks(self, **kwargs: Any) -> list[Any]:
        self.request_ticks_kwargs = kwargs

        class PyCapsule:
            pass

        return [PyCapsule()]


@pytest.mark.asyncio
async def test_pyo3_provider_behaves_like_standard_instrument_provider(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )

    config = _StubConfig(filter_sec_types=frozenset({"OPT"}))
    provider = providers_module.InteractiveBrokersInstrumentProvider(config=config)
    provider._attach_loader(_FakeRustDataClient(provider._rust_provider))

    instrument_id = InstrumentId.from_str("EUR/USD.IDEALPRO")
    loaded_ids = await provider.load_with_return_async(instrument_id)

    assert isinstance(provider, InstrumentProvider)
    assert loaded_ids == [instrument_id]
    assert provider.find(instrument_id) is provider.get_all()[instrument_id]
    assert provider.get_price_magnifier(instrument_id) == 10
    assert provider.filter_sec_types == {"OPT"}
    assert provider.contract[instrument_id].conId == 101
    assert provider.contract_details[instrument_id].priceMagnifier == 10
    assert provider.contract_id_to_instrument_id[101] == instrument_id


@pytest.mark.asyncio
async def test_pyo3_historical_client_normalizes_results_for_v1(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import historical as historical_module

    converted_instrument = _make_instrument(InstrumentId.from_str("AAPL.NASDAQ"))
    converted_bar = object()
    converted_tick = object()

    class _FakeBar:
        @staticmethod
        def from_pyo3_list(values: list[Any]) -> list[Any]:
            assert values == ["raw-bar"]
            return [converted_bar]

    monkeypatch.setattr(
        historical_module,
        "RustHistoricalInteractiveBrokersClient",
        _FakeHistoricalRustClient,
    )
    monkeypatch.setattr(historical_module, "PyO3InstrumentId", _FakePyO3InstrumentId)
    monkeypatch.setattr(
        historical_module,
        "transform_instrument_from_pyo3",
        lambda value: (
            converted_instrument if getattr(value, "kind", None) == "instrument" else value
        ),
    )
    monkeypatch.setattr(historical_module, "Bar", _FakeBar)
    monkeypatch.setattr(
        historical_module,
        "capsule_to_data",
        lambda _value: converted_tick,
    )

    provider = SimpleNamespace(_rust_provider=object())
    config = SimpleNamespace()
    client = historical_module.HistoricalInteractiveBrokersClient(
        instrument_provider=provider,
        config=config,
    )

    instruments = await client.request_instruments(instrument_ids=["AAPL.NASDAQ"])
    bars = await client.request_bars(
        bar_specifications=["1-HOUR-LAST"],
        start_date_time=pd.Timestamp("2025-11-06 09:30:00").to_pydatetime(),
        end_date_time=pd.Timestamp("2025-11-06 10:30:00").to_pydatetime(),
        instrument_ids=["AAPL.NASDAQ"],
    )
    ticks = await client.request_ticks(
        tick_type="TRADES",
        start_date_time=pd.Timestamp("2025-11-06 10:00:00").to_pydatetime(),
        end_date_time=pd.Timestamp("2025-11-06 10:01:00").to_pydatetime(),
        instrument_ids=["AAPL.NASDAQ"],
    )

    assert instruments == [converted_instrument]
    assert bars == [converted_bar]
    assert ticks == [converted_tick]
    assert client._rust_client.request_instruments_kwargs == {
        "instrument_ids": ["pyo3:AAPL.NASDAQ"],
        "contracts": None,
    }
    assert client._rust_client.request_bars_kwargs["instrument_ids"] == ["pyo3:AAPL.NASDAQ"]
    assert client._rust_client.request_ticks_kwargs["instrument_ids"] == ["pyo3:AAPL.NASDAQ"]


@pytest.mark.asyncio
async def test_pyo3_historical_client_rejects_non_utc_aware_datetimes(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import historical as historical_module

    monkeypatch.setattr(
        historical_module,
        "RustHistoricalInteractiveBrokersClient",
        _FakeHistoricalRustClient,
    )
    monkeypatch.setattr(historical_module, "PyO3InstrumentId", _FakePyO3InstrumentId)

    client = historical_module.HistoricalInteractiveBrokersClient(
        instrument_provider=SimpleNamespace(_rust_provider=object()),
        config=SimpleNamespace(),
    )

    with pytest.raises(ValueError, match="must be UTC"):
        await client.request_bars(
            bar_specifications=["1-HOUR-LAST"],
            start_date_time=pd.Timestamp("2025-11-06 09:30:00").to_pydatetime(),
            end_date_time=pd.Timestamp("2025-11-06 10:30:00")
            .to_pydatetime()
            .replace(
                tzinfo=timezone(timedelta(hours=-5)),
            ),
            instrument_ids=["AAPL.NASDAQ"],
        )

    with pytest.raises(ValueError, match="must be UTC"):
        await client.request_ticks(
            tick_type="TRADES",
            start_date_time=pd.Timestamp("2025-11-06 10:00:00")
            .to_pydatetime()
            .replace(
                tzinfo=timezone(timedelta(hours=1)),
            ),
            end_date_time=pd.Timestamp("2025-11-06 10:01:00").to_pydatetime(),
            instrument_ids=["AAPL.NASDAQ"],
        )


@pytest.mark.asyncio
async def test_pyo3_provider_load_ids_supports_contracts_and_cached_details(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )

    config = _StubConfig()
    provider = providers_module.InteractiveBrokersInstrumentProvider(config=config)
    provider._attach_loader(_FakeRustDataClient(provider._rust_provider))

    existing_id = InstrumentId.from_str("AAPL.NASDAQ")
    contract = {
        "secType": "STK",
        "conId": 777,
        "symbol": "MSFT",
        "exchange": "NASDAQ",
        "build_options_chain": True,
        "min_expiry_days": 0,
        "max_expiry_days": 3,
    }

    loaded_ids = await provider.load_ids_with_return_async([existing_id, contract])
    contract_id = InstrumentId.from_str("MSFT.NASDAQ")
    details = await provider.instrument_id_to_ib_contract_details(contract_id)

    assert loaded_ids == [existing_id, contract_id]
    assert provider.find(existing_id) is not None
    assert provider.find(contract_id) is not None
    assert provider.get_instrument_id_by_contract_id(777) == contract_id
    assert await provider.instrument_id_to_ib_contract(contract_id) == IBContract(**contract)
    assert details.contract == IBContract(**contract)
    assert provider.determine_venue_from_contract(contract) == "XCBO"


@pytest.mark.asyncio
async def test_pyo3_provider_load_all_accepts_dict_contract_specs_from_config(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )

    config = _StubConfig(
        load_contracts=[
            {
                "secType": "STK",
                "conId": 901,
                "symbol": "SPY",
                "exchange": "SMART",
                "primaryExchange": "CBOE",
                "build_options_chain": True,
                "min_expiry_days": 0,
                "max_expiry_days": 3,
            },
        ],
    )
    provider = providers_module.InteractiveBrokersInstrumentProvider(config=config)
    loader = _FakeRustDataClient(provider._rust_provider)
    provider._attach_loader(loader)

    await provider.load_all_async()

    instrument_id = InstrumentId.from_str("SPY.SMART")
    contract = await provider.instrument_id_to_ib_contract(instrument_id)

    assert provider.find(instrument_id) is not None
    assert contract == IBContract(
        secType="STK",
        conId=901,
        symbol="SPY",
        exchange="SMART",
        primaryExchange="CBOE",
        build_options_chain=True,
        min_expiry_days=0,
        max_expiry_days=3,
    )
    assert len(loader.option_chain_calls) == 1
    assert loader.option_chain_calls[0][0]["symbol"] == "SPY"
    assert loader.option_chain_calls[0][0]["exchange"] == "SMART"
    assert loader.option_chain_calls[0][1] is not None
    assert loader.option_chain_calls[0][2] is not None


@pytest.mark.asyncio
async def test_pyo3_provider_load_all_accepts_futures_chain_contract_specs_from_config(
    monkeypatch,
):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )

    config = _StubConfig(
        load_contracts=[
            {
                "secType": "CONTFUT",
                "conId": 902,
                "symbol": "ES",
                "exchange": "CME",
                "currency": "USD",
                "build_futures_chain": True,
                "min_expiry_days": 5,
                "max_expiry_days": 12,
            },
        ],
    )
    provider = providers_module.InteractiveBrokersInstrumentProvider(config=config)
    loader = _FakeRustDataClient(provider._rust_provider)
    provider._attach_loader(loader)

    await provider.load_all_async()

    instrument_id = InstrumentId.from_str("ES.CME")
    contract = await provider.instrument_id_to_ib_contract(instrument_id)

    assert provider.find(instrument_id) is not None
    assert contract == IBContract(
        secType="CONTFUT",
        conId=902,
        symbol="ES",
        exchange="CME",
        currency="USD",
        build_futures_chain=True,
        min_expiry_days=5,
        max_expiry_days=12,
    )
    assert loader.futures_chain_calls == [("ES", "CME", "USD", 5, 12)]


@pytest.mark.asyncio
async def test_pyo3_provider_contfut_option_chain_loads_nearby_futures_then_options(
    monkeypatch,
):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )

    provider = providers_module.InteractiveBrokersInstrumentProvider(
        config=_StubConfig(),
    )
    loader = _FakeRustDataClient(provider._rust_provider)
    provider._attach_loader(loader)

    await provider.load_ids_with_return_async(
        [
            {
                "secType": "CONTFUT",
                "symbol": "ES",
                "exchange": "CME",
                "currency": "USD",
                "build_options_chain": True,
                "min_expiry_days": 0,
                "max_expiry_days": 5,
            },
        ],
    )

    assert loader.futures_chain_calls == [("ES", "CME", "USD", 0, 5)]
    assert len(loader.option_chain_calls) == 1
    assert loader.option_chain_calls[0][0]["secType"] == "FUT"
    assert loader.option_chain_calls[0][0]["localSymbol"] == "ESM6"


@pytest.mark.asyncio
async def test_pyo3_data_request_instruments_uses_dict_contract_specs(monkeypatch):
    import asyncio

    from nautilus_trader.adapters.interactive_brokers_pyo3 import data as data_module
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.model.identifiers import TraderId
    from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase

    instrument_id = InstrumentId.from_str("SPY.SMART")
    loaded: dict[str, Any] = {}

    class _FakeRustClient:
        def __init__(self, *_args: Any) -> None:
            self.request_calls: list[tuple[Any, Any]] = []
            self.load_all_calls: list[tuple[Any, Any, Any]] = []
            self.get_instrument_calls: list[dict[str, Any]] = []
            self.option_chain_calls: list[tuple[dict[str, Any], str | None, str | None]] = []
            self.client_id = SimpleNamespace(value="IB")

        def request_instruments(self, venue: Any, params: Any) -> None:
            self.request_calls.append((venue, params))

        async def load_all_async(
            self,
            instrument_ids: Any,
            contracts: Any,
            force_instrument_update: Any,
        ) -> list[InstrumentId]:
            self.load_all_calls.append((instrument_ids, contracts, force_instrument_update))
            return [instrument_id]

        async def get_instrument(self, contract: dict[str, Any]) -> Any:
            self.get_instrument_calls.append(contract)
            return None

        async def py_fetch_option_chain_by_range_for_contract(
            self,
            contract: dict[str, Any],
            expiry_min: str | None = None,
            expiry_max: str | None = None,
        ) -> int:
            self.option_chain_calls.append(
                (contract, expiry_min, expiry_max),
            )
            return 0

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )
    monkeypatch.setattr(
        data_module,
        "RustInteractiveBrokersDataClient",
        _FakeRustClient,
    )

    provider = providers_module.InteractiveBrokersInstrumentProvider(config=_StubConfig())
    cache = Cache(database=MockCacheDatabase())
    client = data_module.InteractiveBrokersDataClient(
        loop=asyncio.get_running_loop(),
        msgbus=MessageBus(trader_id=TraderId("TEST-001"), clock=LiveClock()),
        cache=cache,
        clock=LiveClock(),
        instrument_provider=provider,
        config=SimpleNamespace(),
    )
    client._handle_instruments = lambda **kwargs: loaded.update(kwargs)

    request = SimpleNamespace(
        venue="INTERACTIVE_BROKERS",
        params={
            "ib_contracts": [
                {
                    "secType": "STK",
                    "symbol": "SPY",
                    "exchange": "SMART",
                    "primaryExchange": "CBOE",
                    "build_options_chain": True,
                    "min_expiry_days": 0,
                    "max_expiry_days": 3,
                },
            ],
        },
        id="req-1",
        start=None,
        end=None,
    )

    await data_module.InteractiveBrokersDataClient._request_instruments(client, request)

    assert client._rust_client.request_calls == []
    assert client._rust_client.load_all_calls == []
    assert client._rust_client.get_instrument_calls == [
        {
            "secType": "STK",
            "symbol": "SPY",
            "exchange": "SMART",
            "primaryExchange": "CBOE",
            "build_options_chain": True,
            "min_expiry_days": 0,
            "max_expiry_days": 3,
        },
    ]
    assert len(client._rust_client.option_chain_calls) == 1
    assert client._rust_client.option_chain_calls[0][0]["symbol"] == "SPY"
    assert client._rust_client.option_chain_calls[0][0]["exchange"] == "SMART"
    assert client._rust_client.option_chain_calls[0][1] is not None
    assert client._rust_client.option_chain_calls[0][2] is not None
    assert loaded["venue"] == "INTERACTIVE_BROKERS"
    assert loaded["instruments"] == []
    assert loaded["params"] == request.params


def test_historic_client_alias_matches_legacy_name():
    from nautilus_trader.adapters.interactive_brokers_pyo3 import HistoricalInteractiveBrokersClient
    from nautilus_trader.adapters.interactive_brokers_pyo3 import HistoricInteractiveBrokersClient

    assert HistoricInteractiveBrokersClient is HistoricalInteractiveBrokersClient


def test_v1_factory_aliases_are_explicit():
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersLiveDataClientFactory,
    )
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersLiveExecClientFactory,
    )
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersV1LiveDataClientFactory,
    )
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersV1LiveExecClientFactory,
    )

    assert InteractiveBrokersLiveDataClientFactory is InteractiveBrokersV1LiveDataClientFactory
    assert InteractiveBrokersLiveExecClientFactory is InteractiveBrokersV1LiveExecClientFactory


def test_v1_data_factory_honors_dockerized_gateway(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import DockerizedIBGatewayConfig
    from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersInstrumentProviderConfig,
    )
    from nautilus_trader.adapters.interactive_brokers_pyo3 import factories as factories_module

    started: list[tuple[str, int | None]] = []
    created: dict[str, Any] = {}

    class _FakeGateway:
        def __init__(self, config: Any) -> None:
            self.config = config
            self.port = 4002

        async def safe_start(self, wait: int | None = None) -> None:
            started.append((self.config.trading_mode, wait))

    class _FakeDataClient:
        def __init__(self, **kwargs: Any) -> None:
            created.update(kwargs)

    monkeypatch.setattr(factories_module, "DockerizedIBGateway", _FakeGateway)
    monkeypatch.setattr(factories_module, "GATEWAYS", {})
    monkeypatch.setattr(factories_module, "_build_provider", lambda config: "provider")
    monkeypatch.setattr(factories_module, "InteractiveBrokersDataClient", _FakeDataClient)

    gateway_config = DockerizedIBGatewayConfig(trading_mode="paper", timeout=11)
    config = InteractiveBrokersDataClientConfig(
        ibg_host="127.0.0.1",
        ibg_port=None,
        dockerized_gateway=gateway_config,
        instrument_provider=InteractiveBrokersInstrumentProviderConfig(),
    )

    loop = asyncio.new_event_loop()
    try:
        factories_module.InteractiveBrokersV1LiveDataClientFactory.create(
            loop=loop,
            name="IB",
            config=config,
            msgbus=MagicMock(),
            cache=MagicMock(),
            clock=MagicMock(),
        )
    finally:
        loop.close()

    assert started == [(gateway_config.trading_mode, 11)]
    assert created["config"].host == "127.0.0.1"
    assert created["config"].port == 4002
    assert created["instrument_provider"] == "provider"


def test_v1_data_factory_prefers_blocking_gateway_start(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import DockerizedIBGatewayConfig
    from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersInstrumentProviderConfig,
    )
    from nautilus_trader.adapters.interactive_brokers_pyo3 import factories as factories_module

    started: list[tuple[Any, int | None]] = []
    created: dict[str, Any] = {}

    class _FakeGateway:
        def __init__(self, config: Any) -> None:
            self.config = config
            self.port = 4002

        def safe_start_blocking(self, wait: int | None = None) -> None:
            started.append((self.config.trading_mode, wait))

        async def safe_start(self, wait: int | None = None) -> None:
            raise AssertionError(
                "safe_start should not be used when safe_start_blocking is available",
            )

    class _FakeDataClient:
        def __init__(self, **kwargs: Any) -> None:
            created.update(kwargs)

    monkeypatch.setattr(factories_module, "DockerizedIBGateway", _FakeGateway)
    monkeypatch.setattr(factories_module, "GATEWAYS", {})
    monkeypatch.setattr(factories_module, "_build_provider", lambda config: "provider")
    monkeypatch.setattr(factories_module, "InteractiveBrokersDataClient", _FakeDataClient)

    gateway_config = DockerizedIBGatewayConfig(trading_mode="paper", timeout=9)
    config = InteractiveBrokersDataClientConfig(
        ibg_host="127.0.0.1",
        ibg_port=None,
        dockerized_gateway=gateway_config,
        instrument_provider=InteractiveBrokersInstrumentProviderConfig(),
    )

    loop = asyncio.new_event_loop()
    try:
        factories_module.InteractiveBrokersV1LiveDataClientFactory.create(
            loop=loop,
            name="IB",
            config=config,
            msgbus=MagicMock(),
            cache=MagicMock(),
            clock=MagicMock(),
        )
    finally:
        loop.close()

    assert started == [(gateway_config.trading_mode, 9)]
    assert created["config"].port == 4002
    assert created["instrument_provider"] == "provider"


def test_v1_factories_reuse_cached_provider(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
    from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersExecClientConfig
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersInstrumentProviderConfig,
    )
    from nautilus_trader.adapters.interactive_brokers_pyo3 import factories as factories_module

    created_providers: list[Any] = []
    attached_providers: list[Any] = []

    class _FakeProvider:
        def __init__(self, config: Any) -> None:
            self.config = config
            created_providers.append(self)

    class _FakeDataClient:
        def __init__(self, **kwargs: Any) -> None:
            attached_providers.append(kwargs["instrument_provider"])

    class _FakeExecClient:
        def __init__(self, **kwargs: Any) -> None:
            attached_providers.append(kwargs["instrument_provider"])

    monkeypatch.setattr(factories_module, "InteractiveBrokersInstrumentProvider", _FakeProvider)
    monkeypatch.setattr(factories_module, "InteractiveBrokersDataClient", _FakeDataClient)
    monkeypatch.setattr(factories_module, "InteractiveBrokersExecutionClient", _FakeExecClient)
    monkeypatch.setattr(factories_module, "IB_INSTRUMENT_PROVIDERS", {})

    provider_config = InteractiveBrokersInstrumentProviderConfig()
    data_config = InteractiveBrokersDataClientConfig(
        ibg_host="127.0.0.1",
        ibg_port=4002,
        ibg_client_id=7,
        instrument_provider=provider_config,
    )
    exec_config = InteractiveBrokersExecClientConfig(
        ibg_host="127.0.0.1",
        ibg_port=4002,
        ibg_client_id=7,
        account_id="DU123456",
        instrument_provider=provider_config,
    )

    factories_module.InteractiveBrokersV1LiveDataClientFactory.create(
        loop=asyncio.new_event_loop(),
        name="IB-DATA",
        config=data_config,
        msgbus=MagicMock(),
        cache=MagicMock(),
        clock=MagicMock(),
    )
    factories_module.InteractiveBrokersV1LiveExecClientFactory.create(
        loop=asyncio.new_event_loop(),
        name="IB-EXEC",
        config=exec_config,
        msgbus=MagicMock(),
        cache=MagicMock(),
        clock=MagicMock(),
    )

    assert len(created_providers) == 1
    assert attached_providers[0] is attached_providers[1]


def test_pyo3_provider_config_accepts_raw_symbology():
    from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
    from nautilus_trader.adapters.interactive_brokers_pyo3 import (
        InteractiveBrokersInstrumentProviderConfig,
    )

    config = InteractiveBrokersInstrumentProviderConfig(
        symbology_method=SymbologyMethod.IB_RAW,
    )
    rust_symbology_enum = type(InteractiveBrokersInstrumentProviderConfig().symbology_method)

    assert config.symbology_method == rust_symbology_enum.Raw
    assert config.legacy_symbology_method == SymbologyMethod.IB_RAW


@pytest.mark.asyncio
async def test_pyo3_data_client_converts_bar_types_and_timestamps(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import data as data_module
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.core.uuid import UUID4
    from nautilus_trader.model.data import BarType
    from nautilus_trader.model.identifiers import TraderId
    from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
    from nautilus_trader.test_kit.providers import TestInstrumentProvider

    class _FakeRustClient:
        def __init__(self, *_args: Any) -> None:
            self.client_id = SimpleNamespace(value="IB")
            self.request_bars_calls: list[tuple[Any, Any, Any, Any, Any]] = []
            self.subscribe_bars_calls: list[Any] = []

        def set_event_callback(self, _callback: Any) -> None:
            return None

        def connect(self) -> None:
            return None

        def disconnect(self) -> None:
            return None

        def request_bars(
            self,
            bar_type: Any,
            limit: Any,
            start: Any,
            end: Any,
            request_id: Any,
        ) -> None:
            self.request_bars_calls.append((bar_type, limit, start, end, request_id))

        def subscribe_bars(self, bar_type: Any) -> None:
            self.subscribe_bars_calls.append(bar_type)

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )
    monkeypatch.setattr(
        data_module,
        "RustInteractiveBrokersDataClient",
        _FakeRustClient,
    )

    provider = providers_module.InteractiveBrokersInstrumentProvider(config=_StubConfig())
    client = data_module.InteractiveBrokersDataClient(
        loop=asyncio.get_running_loop(),
        msgbus=MessageBus(trader_id=TraderId("TEST-001"), clock=LiveClock()),
        cache=Cache(database=MockCacheDatabase()),
        clock=LiveClock(),
        instrument_provider=provider,
        config=SimpleNamespace(),
    )

    instrument = TestInstrumentProvider.future()
    instrument_id = instrument.id
    bar_type = BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")
    client._cache.add_instrument(instrument)

    request = SimpleNamespace(
        bar_type=bar_type,
        limit=30,
        start=pd.Timestamp("2026-03-28T15:00:00Z"),
        end=pd.Timestamp("2026-03-28T15:30:00Z"),
        id=UUID4(),
        params={},
    )

    await client._request_bars(request)
    await client._subscribe_bars(SimpleNamespace(bar_type=bar_type))

    request_call = client._rust_client.request_bars_calls[0]
    assert request_call[1] == 30
    assert request_call[2] == request.start.value
    assert request_call[3] == request.end.value
    assert request_call[4] == str(request.id)
    assert request_call[0].__class__.__module__.startswith("nautilus_trader.core.nautilus_pyo3")
    assert client._rust_client.subscribe_bars_calls[0].__class__.__module__.startswith(
        "nautilus_trader.core.nautilus_pyo3",
    )


@pytest.mark.asyncio
async def test_pyo3_data_client_converts_index_price_subscription_for_rust(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import data as data_module
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.model.identifiers import TraderId
    from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
    from nautilus_trader.test_kit.providers import TestInstrumentProvider

    class _FakeRustClient:
        def __init__(self, *_args: Any) -> None:
            self.client_id = SimpleNamespace(value="IB")
            self.subscribe_index_prices_calls: list[Any] = []
            self.unsubscribe_index_prices_calls: list[Any] = []

        def set_event_callback(self, _callback: Any) -> None:
            return None

        def connect(self) -> None:
            return None

        def disconnect(self) -> None:
            return None

        def subscribe_index_prices(self, instrument_id: Any) -> None:
            self.subscribe_index_prices_calls.append(instrument_id)

        def unsubscribe_index_prices(self, instrument_id: Any) -> None:
            self.unsubscribe_index_prices_calls.append(instrument_id)

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )
    monkeypatch.setattr(
        data_module,
        "RustInteractiveBrokersDataClient",
        _FakeRustClient,
    )

    provider = providers_module.InteractiveBrokersInstrumentProvider(config=_StubConfig())
    client = data_module.InteractiveBrokersDataClient(
        loop=asyncio.get_running_loop(),
        msgbus=MessageBus(trader_id=TraderId("TEST-001"), clock=LiveClock()),
        cache=Cache(database=MockCacheDatabase()),
        clock=LiveClock(),
        instrument_provider=provider,
        config=SimpleNamespace(),
    )

    instrument = TestInstrumentProvider.index_instrument()
    instrument_id = instrument.id
    client._cache.add_instrument(instrument)

    await client._subscribe_index_prices(SimpleNamespace(instrument_id=instrument_id))
    await client._unsubscribe_index_prices(SimpleNamespace(instrument_id=instrument_id))

    subscribe_call = client._rust_client.subscribe_index_prices_calls[0]
    unsubscribe_call = client._rust_client.unsubscribe_index_prices_calls[0]
    assert subscribe_call.__class__.__module__.startswith("nautilus_trader.core.nautilus_pyo3")
    assert unsubscribe_call.__class__.__module__.startswith("nautilus_trader.core.nautilus_pyo3")


@pytest.mark.asyncio
async def test_pyo3_execution_client_converts_orders_before_submission(monkeypatch):
    from nautilus_trader.adapters.interactive_brokers_pyo3 import execution as execution_module
    from nautilus_trader.adapters.interactive_brokers_pyo3 import providers as providers_module
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.model.identifiers import TraderId
    from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase

    class _FakeRustExecutionClient:
        def __init__(self, *_args: Any) -> None:
            self.client_id = SimpleNamespace(value="IB")
            self.submit_order_list_calls: list[tuple[Any, Any, Any, Any, Any, Any]] = []

        def set_event_callback(self, _callback: Any) -> None:
            return None

        def connect(self) -> None:
            return None

        def disconnect(self) -> None:
            return None

        def submit_order_list(
            self,
            trader_id: Any,
            strategy_id: Any,
            orders: list[Any],
            exec_algorithm_id: Any,
            position_id: Any,
            params: Any,
        ) -> None:
            self.submit_order_list_calls.append(
                (trader_id, strategy_id, orders, exec_algorithm_id, position_id, params),
            )

    monkeypatch.setattr(
        providers_module,
        "RustInteractiveBrokersInstrumentProvider",
        _FakeRustProvider,
    )
    monkeypatch.setattr(
        execution_module,
        "RustInteractiveBrokersExecutionClient",
        _FakeRustExecutionClient,
    )
    monkeypatch.setattr(
        execution_module,
        "transform_order_to_pyo3",
        lambda order: f"pyo3:{order.client_order_id.value}",
    )

    provider = providers_module.InteractiveBrokersInstrumentProvider(config=_StubConfig())
    client = execution_module.InteractiveBrokersExecutionClient(
        loop=asyncio.get_running_loop(),
        msgbus=MessageBus(trader_id=TraderId("TEST-001"), clock=LiveClock()),
        cache=Cache(database=MockCacheDatabase()),
        clock=LiveClock(),
        instrument_provider=provider,
        config=SimpleNamespace(),
    )

    order_1 = SimpleNamespace(client_order_id=SimpleNamespace(value="O-1"))
    order_2 = SimpleNamespace(client_order_id=SimpleNamespace(value="O-2"))
    command = SimpleNamespace(
        trader_id=SimpleNamespace(value="TESTER-001"),
        strategy_id=SimpleNamespace(value="DemoStrategy-000"),
        order_list=SimpleNamespace(orders=[order_1, order_2]),
        exec_algorithm_id=None,
        position_id=None,
        params=None,
    )

    await client._submit_order_list(command)

    submit_call = client._rust_client.submit_order_list_calls[0]
    assert submit_call[2] == ["pyo3:O-1", "pyo3:O-2"]
