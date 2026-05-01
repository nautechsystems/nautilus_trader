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


def test_historic_client_alias_matches_legacy_name():
    from nautilus_trader.adapters.interactive_brokers_pyo3 import HistoricalInteractiveBrokersClient
    from nautilus_trader.adapters.interactive_brokers_pyo3 import HistoricInteractiveBrokersClient

    assert HistoricInteractiveBrokersClient is HistoricalInteractiveBrokersClient


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
