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
"""
Provides a PyO3-based instrument provider for Interactive Brokers.

This adapter uses PyO3 bindings to call the Rust implementation of the Interactive
Brokers adapter, providing the same API as the Python adapter but with Rust performance.

"""

from __future__ import annotations

from collections.abc import Iterable
from contextlib import suppress
from types import ModuleType
from typing import Any
from typing import Protocol

import pandas as pd

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import IBContractSpec
from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import ib_contract_spec_to_dict
from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import ib_contract_specs_to_dicts
from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


try:
    from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
        InteractiveBrokersInstrumentProvider as RustInteractiveBrokersInstrumentProvider,
    )
except ImportError:
    RustInteractiveBrokersInstrumentProvider = None

nautilus_pyo3: ModuleType | None
try:
    import nautilus_trader.core.nautilus_pyo3 as nautilus_pyo3
except ImportError:
    nautilus_pyo3 = None


def _normalize_instrument_id(value: InstrumentId | Any) -> InstrumentId:
    if isinstance(value, InstrumentId):
        return value

    raw_value = getattr(value, "value", None)
    if raw_value is None:
        raw_value = str(value)

    return InstrumentId.from_str(raw_value)


def _same_instrument_id(left: Any, right: Any) -> bool:
    return str(left) == str(right)


def _normalize_instrument(value: Instrument | Any) -> Instrument:
    if isinstance(value, Instrument):
        instrument_id = getattr(value, "id", None)
        if instrument_id is not None:
            normalized_id = _normalize_instrument_id(instrument_id)
            if not _same_instrument_id(instrument_id, normalized_id):
                with suppress(Exception):
                    value.id = normalized_id
        return value

    instrument = transform_instrument_from_pyo3(value)
    if instrument is not None:
        instrument_id = getattr(instrument, "id", None)
        if instrument_id is not None:
            normalized_id = _normalize_instrument_id(instrument_id)
            if not _same_instrument_id(instrument_id, normalized_id):
                with suppress(Exception):
                    instrument.id = normalized_id
        return instrument

    instrument_id = getattr(value, "id", None)
    if instrument_id is not None:
        normalized_id = _normalize_instrument_id(instrument_id)
        with suppress(Exception):
            value.id = normalized_id
        return value

    return value


def _to_pyo3_instrument_id(value: InstrumentId | Any) -> Any:
    if nautilus_pyo3 is None or not isinstance(value, InstrumentId):
        return value

    return nautilus_pyo3.InstrumentId.from_str(value.value)


class _RustInstrumentLoader(Protocol):
    async def load_all_async(
        self,
        instrument_ids: list[Any] | None,
        contracts: list[dict[str, Any]] | None,
        force_instrument_update: bool,
    ) -> list[Any] | None: ...

    async def load_ids_with_return_async(
        self,
        instrument_ids: list[Any],
        filters: dict[str, str] | None,
    ) -> list[Any]: ...

    async def load_with_return_async(
        self,
        instrument_id: Any,
        filters: dict[str, str] | None,
    ) -> Any | None: ...

    async def get_instrument(self, contract: dict[str, Any]) -> Instrument | Any | None: ...

    async def py_fetch_option_chain_by_range(
        self,
        underlying_symbol: str,
        exchange: str | None = None,
        currency: str | None = None,
        expiry_min: str | None = None,
        expiry_max: str | None = None,
    ) -> int | None: ...

    async def py_fetch_option_chain_by_range_for_contract(
        self,
        contract: dict[str, Any],
        expiry_min: str | None = None,
        expiry_max: str | None = None,
    ) -> int | None: ...

    async def py_fetch_futures_chain(
        self,
        symbol: str,
        exchange: str | None = None,
        currency: str | None = None,
        min_expiry_days: int | None = None,
        max_expiry_days: int | None = None,
    ) -> int | None: ...


class InteractiveBrokersInstrumentProvider(InstrumentProvider):
    """
    Provides a PyO3-based instrument provider for Interactive Brokers.

    This class wraps the Rust implementation via PyO3 bindings, providing
    the same API as the Python adapter but using the Rust implementation.

    Parameters
    ----------
    config : InteractiveBrokersInstrumentProviderConfig
        Configuration for the provider.

    Raises
    ------
    ImportError
        If the PyO3 bindings are not available.

    """

    def __init__(
        self,
        config: InteractiveBrokersInstrumentProviderConfig,
    ) -> None:
        if RustInteractiveBrokersInstrumentProvider is None:
            raise ImportError(
                "PyO3 bindings for Interactive Brokers are not available. "
                "Please ensure the extension module is built with the 'extension-module' feature.",
            )

        super().__init__(config=config)

        # Initialize the Rust provider via PyO3
        self._rust_provider = RustInteractiveBrokersInstrumentProvider(config)
        self._loader: _RustInstrumentLoader | None = None
        self._filter_sec_types = set(config.filter_sec_types or [])
        self.contract: dict[InstrumentId, IBContract] = {}
        self.contract_details: dict = {}
        self.contract_id_to_instrument_id: dict[int, InstrumentId] = {}

    @property
    def filter_sec_types(self) -> set[str]:
        return self._filter_sec_types

    def _attach_loader(self, loader: _RustInstrumentLoader) -> None:
        self._loader = loader

    def _require_loader(self) -> _RustInstrumentLoader:
        if self._loader is None:
            raise RuntimeError(
                "InteractiveBrokersInstrumentProvider requires an attached Interactive Brokers "
                "loader for load operations. Construct it through the IB data client or factory.",
            )

        return self._loader

    def _sync_from_rust(self) -> None:
        instruments: dict[InstrumentId, Instrument] = {}

        for raw_instrument in self._rust_provider.get_all():
            instrument = _normalize_instrument(raw_instrument)
            instrument_id = _normalize_instrument_id(instrument.id)
            if not _same_instrument_id(instrument.id, instrument_id):
                with suppress(Exception):
                    instrument.id = instrument_id
            instruments[instrument_id] = instrument
        self._instruments = instruments

    async def _sync_loaded_ids(self, instrument_ids: Iterable[InstrumentId | Any]) -> None:
        self._sync_from_rust()

        for instrument_id in (_normalize_instrument_id(item) for item in instrument_ids):
            details = await self.instrument_id_to_ib_contract_details(instrument_id)
            if details is None:
                continue

            self.contract_details[instrument_id] = details
            self.contract[instrument_id] = details.contract
            self.contract_id_to_instrument_id[details.contract.conId] = instrument_id

    async def initialize(self, reload: bool = False) -> None:
        self._sync_from_rust()

        if reload:
            self._loaded = False

        if not reload and self._loaded:
            return

        if self._instruments:
            self._loaded = True
            return

        load_contracts = getattr(self._config, "load_contracts", None)
        if self._load_ids_on_start or load_contracts:
            await self.load_all_async(self._filters)

        self._loaded = True

    def find(self, instrument_id: InstrumentId) -> Instrument | None:
        instrument = super().find(instrument_id)
        if instrument is None:
            self._sync_from_rust()
            instrument = super().find(instrument_id)
        return instrument

    def find_by_contract_id(self, contract_id: int) -> Instrument | None:
        instrument = self._rust_provider.find_by_contract_id(contract_id)
        if instrument is not None:
            instrument = _normalize_instrument(instrument)
            self.add(instrument)
            if instrument_id := self._rust_provider.get_instrument_id_by_contract_id(contract_id):
                self.contract_id_to_instrument_id[contract_id] = _normalize_instrument_id(
                    instrument_id,
                )
        return instrument

    def get_price_magnifier(self, instrument_id: InstrumentId) -> int:
        return self._rust_provider.get_price_magnifier(_to_pyo3_instrument_id(instrument_id))

    def fetch_contract_details(self) -> None:
        self._rust_provider.fetch_contract_details()

    async def _load_contract_spec_via_loader(
        self,
        loader: _RustInstrumentLoader,
        contract_spec: IBContractSpec,
        force_instrument_update: bool,
    ) -> list[InstrumentId]:
        contract = ib_contract_spec_to_dict(contract_spec)
        sec_type = str(contract.get("secType", "")).upper()
        config_build_options_chain = getattr(self._config, "build_options_chain", None)
        config_build_futures_chain = getattr(self._config, "build_futures_chain", None)
        config_min_expiry_days = getattr(self._config, "min_expiry_days", None)
        config_max_expiry_days = getattr(self._config, "max_expiry_days", None)

        build_options_chain = bool(
            contract.get("build_options_chain") or config_build_options_chain,
        )
        build_futures_chain = bool(
            contract.get("build_futures_chain") or config_build_futures_chain,
        )

        min_expiry_days = contract.get("min_expiry_days")
        max_expiry_days = contract.get("max_expiry_days")
        min_days = (
            int(min_expiry_days) if min_expiry_days is not None else (config_min_expiry_days or 0)
        )
        max_days = (
            int(max_expiry_days) if max_expiry_days is not None else (config_max_expiry_days or 90)
        )

        if sec_type == "CONTFUT" and (build_futures_chain or build_options_chain):
            before_ids = set(self._instruments.keys())

            instrument = await loader.get_instrument(contract)
            if instrument is not None:
                instrument = _normalize_instrument(instrument)
                self.add(instrument)
                await self._sync_loaded_ids([instrument.id])

            underlying_contract = (
                await self.instrument_id_to_ib_contract(_normalize_instrument_id(instrument.id))
                if instrument is not None
                else None
            )
            contract_for_chain = ib_contract_spec_to_dict(underlying_contract or contract)
            await loader.py_fetch_futures_chain(
                contract_for_chain["symbol"],
                exchange=contract_for_chain.get("exchange") or "",
                currency=contract_for_chain.get("currency") or "USD",
                min_expiry_days=min_days,
                max_expiry_days=max_days,
            )

            self._sync_from_rust()

            if build_options_chain:
                future_ids = sorted(set(self._instruments.keys()) - before_ids, key=str)

                expiry = contract.get("lastTradeDateOrContractMonth")
                if expiry:
                    expiry_min = str(expiry)
                    expiry_max = str(expiry)
                else:
                    utc_now = pd.Timestamp.now(tz="UTC")
                    expiry_min = (utc_now + pd.Timedelta(days=min_days)).strftime("%Y%m%d")
                    expiry_max = (utc_now + pd.Timedelta(days=max_days)).strftime("%Y%m%d")

                for future_id in future_ids:
                    future_contract = await self.instrument_id_to_ib_contract(future_id)
                    if future_contract is None or future_contract.secType != "FUT":
                        continue

                    await loader.py_fetch_option_chain_by_range_for_contract(
                        ib_contract_spec_to_dict(future_contract),
                        expiry_min=expiry_min,
                        expiry_max=expiry_max,
                    )

                self._sync_from_rust()

            after_ids = set(self._instruments.keys())
            return sorted(after_ids - before_ids, key=str)

        if build_options_chain and sec_type in {"STK", "CONTFUT", "FUT", "IND"}:
            before_ids = set(self._instruments.keys())

            instrument = await loader.get_instrument(contract)
            if instrument is not None:
                instrument = _normalize_instrument(instrument)
                self.add(instrument)
                await self._sync_loaded_ids([instrument.id])

            underlying_contract = (
                await self.instrument_id_to_ib_contract(_normalize_instrument_id(instrument.id))
                if instrument is not None
                else None
            )
            contract_for_chain = ib_contract_spec_to_dict(underlying_contract or contract)

            expiry = contract.get("lastTradeDateOrContractMonth")
            if expiry:
                expiry_min = str(expiry)
                expiry_max = str(expiry)
            else:
                utc_now = pd.Timestamp.now(tz="UTC")
                expiry_min = (utc_now + pd.Timedelta(days=min_days)).strftime("%Y%m%d")
                expiry_max = (utc_now + pd.Timedelta(days=max_days)).strftime("%Y%m%d")

            await loader.py_fetch_option_chain_by_range_for_contract(
                contract_for_chain,
                expiry_min=expiry_min,
                expiry_max=expiry_max,
            )

            self._sync_from_rust()
            after_ids = set(self._instruments.keys())
            return sorted(after_ids - before_ids, key=str)

        loaded_ids = await loader.load_all_async(
            None,
            [contract],
            force_instrument_update,
        )
        return [_normalize_instrument_id(item) for item in (loaded_ids or [])]

    async def load_all_async(self, filters: dict | None = None) -> None:
        loader = self._require_loader()
        load_contracts = getattr(self._config, "load_contracts", None)
        instrument_ids = [
            _normalize_instrument_id(instrument_id)
            for instrument_id in (self._load_ids_on_start or [])
        ]
        contracts = ib_contract_specs_to_dicts(load_contracts or [])
        force_instrument_update = bool(filters and filters.get("force_instrument_update", False))
        loaded_ids = [
            _normalize_instrument_id(item)
            for item in await loader.load_all_async(
                [_to_pyo3_instrument_id(item) for item in instrument_ids] or None,
                None,
                force_instrument_update,
            )
            or []
        ]

        for contract in contracts or []:
            loaded_ids.extend(
                await self._load_contract_spec_via_loader(
                    loader,
                    contract,
                    force_instrument_update,
                ),
            )

        await self._sync_loaded_ids(loaded_ids or [])

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId | IBContractSpec],
        filters: dict | None = None,
    ) -> None:
        await self.load_ids_with_return_async(instrument_ids, filters)

    async def load_ids_with_return_async(
        self,
        instrument_ids: list[InstrumentId | IBContractSpec],
        filters: dict | None = None,
    ) -> list[InstrumentId]:
        loader = self._require_loader()
        rust_filters = {k: str(v) for k, v in (filters or {}).items()} or None

        ids = [item for item in instrument_ids if isinstance(item, InstrumentId)]
        contracts = [
            ib_contract_spec_to_dict(item)
            for item in instrument_ids
            if not isinstance(item, InstrumentId)
        ]
        loaded_ids: list[InstrumentId] = []

        if ids:
            loaded_ids.extend(
                [
                    _normalize_instrument_id(item)
                    for item in await loader.load_ids_with_return_async(
                        [_to_pyo3_instrument_id(item) for item in ids],
                        rust_filters,
                    )
                ],
            )
            await self._sync_loaded_ids(loaded_ids)

        if contracts:
            for contract in contracts:
                loaded_ids.extend(
                    await self._load_contract_spec_via_loader(
                        loader,
                        contract,
                        bool(filters and filters.get("force_instrument_update", False)),
                    ),
                )

        await self._sync_loaded_ids(loaded_ids)
        return loaded_ids

    async def load_async(
        self,
        instrument_id: InstrumentId | IBContractSpec,
        filters: dict | None = None,
    ) -> None:
        await self.load_with_return_async(instrument_id, filters)

    async def load_with_return_async(
        self,
        instrument_id: InstrumentId | IBContractSpec,
        filters: dict | None = None,
    ) -> list[InstrumentId] | None:
        loader = self._require_loader()
        rust_filters = {k: str(v) for k, v in (filters or {}).items()} or None

        if isinstance(instrument_id, InstrumentId):
            loaded_id = await loader.load_with_return_async(
                _to_pyo3_instrument_id(instrument_id),
                rust_filters,
            )
            loaded_ids = [_normalize_instrument_id(loaded_id)] if loaded_id is not None else []
        else:
            contract_ids = await self._load_contract_spec_via_loader(
                loader,
                instrument_id,
                bool(filters and filters.get("force_instrument_update", False)),
            )
            loaded_ids = contract_ids

        await self._sync_loaded_ids(loaded_ids)
        return loaded_ids or None

    async def get_instrument(self, contract: IBContractSpec) -> Instrument | None:
        loader = self._require_loader()
        instrument = await loader.get_instrument(
            ib_contract_spec_to_dict(contract),
        )

        if instrument is not None:
            instrument = _normalize_instrument(instrument)
            self.add(instrument)
            await self._sync_loaded_ids([instrument.id])
        return instrument

    async def instrument_id_to_ib_contract_details(
        self,
        instrument_id: InstrumentId,
    ) -> Any | None:
        instrument_id = _normalize_instrument_id(instrument_id)

        if details := self.contract_details.get(instrument_id):
            return details

        pyo3_instrument_id = _to_pyo3_instrument_id(instrument_id)
        details = self._rust_provider.instrument_id_to_ib_contract_details(pyo3_instrument_id)
        if details is None and pyo3_instrument_id is not instrument_id:
            details = self._rust_provider.instrument_id_to_ib_contract_details(instrument_id)
        if details is not None:
            self.contract_details[instrument_id] = details
            self.contract[instrument_id] = details.contract
            self.contract_id_to_instrument_id[details.contract.conId] = instrument_id
        return details

    async def instrument_id_to_ib_contract(
        self,
        instrument_id: InstrumentId,
    ) -> IBContract | None:
        if contract := self.contract.get(instrument_id):
            return contract

        details = await self.instrument_id_to_ib_contract_details(instrument_id)
        return None if details is None else details.contract

    def determine_venue_from_contract(self, contract: IBContractSpec) -> str:
        return self._rust_provider.determine_venue(ib_contract_spec_to_dict(contract))

    def get_instrument_id_by_contract_id(self, contract_id: int) -> InstrumentId | None:
        instrument_id = self._rust_provider.get_instrument_id_by_contract_id(contract_id)
        if instrument_id is not None:
            instrument_id = _normalize_instrument_id(instrument_id)
            self.contract_id_to_instrument_id[contract_id] = instrument_id
        return instrument_id
