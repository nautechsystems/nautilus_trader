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
Tests instrument loading without changing the node cache.
"""

import asyncio
import subprocess
import sys

import pytest

from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.providers import InstrumentProvider
from nautilus_trader.model import Currency
from nautilus_trader.testkit.providers import TestInstrumentProvider


@pytest.mark.parametrize(
    "first_import",
    ["nautilus_trader.config", "nautilus_trader.live", "nautilus_trader.live.providers"],
)
def test_provider_reexport_supports_import_order(first_import: str) -> None:
    """
    Expose the same provider class without a config import cycle.
    """
    code = f"""
import {first_import}
from nautilus_trader.live import InstrumentProvider
from nautilus_trader.live.providers import InstrumentProvider as OriginalProvider
assert InstrumentProvider is OriginalProvider
assert InstrumentProvider().count == 0
"""
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=10,
        check=False,
    )

    assert result.returncode == 0, result.stderr


@pytest.mark.asyncio
async def test_initialize_serializes_loads_and_retries_failure() -> None:
    """
    Initialize serializes loads and retries failure.
    """
    calls = []
    entered = asyncio.Event()
    released = asyncio.Event()
    instrument = TestInstrumentProvider.audusd_sim()

    class Provider(InstrumentProvider):
        async def load_all_async(self, filters=None):
            calls.append(filters)
            if len(calls) == 1:
                raise RuntimeError("Transient load failure")
            entered.set()
            await released.wait()
            self.add(instrument)

    provider = Provider(InstrumentProviderConfig(load_all=True, filters={"market": "spot"}))
    with pytest.raises(RuntimeError, match="Transient load failure"):
        await provider.initialize()

    first = asyncio.create_task(provider.initialize())
    await entered.wait()
    second = asyncio.create_task(provider.initialize())
    released.set()
    await asyncio.gather(first, second)

    assert calls == [{"market": "spot"}, {"market": "spot"}]
    assert provider.get_all() == {instrument.id: instrument}
    assert provider.count == 1


@pytest.mark.asyncio
async def test_load_ids_preserves_previous_and_filters_bulk_load() -> None:
    """
    Load ids preserves previous and filters bulk load.
    """
    instruments = [
        TestInstrumentProvider.audusd_sim(),
        TestInstrumentProvider.usdjpy_sim(),
        TestInstrumentProvider.gbpusd_sim(),
    ]

    class Provider(InstrumentProvider):
        async def load_all_async(self, _filters=None):
            self.add_bulk(instruments)

    provider = Provider()
    provider.add(instruments[0])
    await provider.load_ids_async([instruments[1].id])
    snapshot = provider.get_all()
    snapshot.clear()

    assert provider.list_all() == [instruments[1], instruments[0]]
    assert provider.find(instruments[2].id) is None
    assert provider.count == 2


@pytest.mark.asyncio
async def test_bound_provider_loading_uses_client_task_supervision() -> None:
    """
    Bound provider loading uses client task supervision.
    """
    entered = asyncio.Event()
    finished = asyncio.Event()

    class Client:
        client_id = "PROVIDER"

    class Provider(InstrumentProvider):
        async def load_all_async(self, _filters=None):
            entered.set()
            try:
                await asyncio.Future()
            finally:
                finished.set()

    client = Client()
    runtime = ClientRuntime(client)
    provider = Provider()
    provider._bind(runtime)
    with pytest.raises(RuntimeError, match="not bound"):
        provider.load_all()
    runtime.bind(asyncio.get_running_loop())
    task = provider.load_all()
    await entered.wait()
    runtime.dispose()
    with pytest.raises(asyncio.CancelledError):
        await task

    assert finished.is_set() is True
    assert runtime.complete is True


def test_standalone_provider_loads_synchronously() -> None:
    """
    Standalone provider loads synchronously.
    """
    instrument = TestInstrumentProvider.audusd_sim()

    class Provider(InstrumentProvider):
        async def load_all_async(self, _filters=None):
            self.add(instrument)

    provider = Provider()
    provider.load_all()

    assert provider.list_all() == [instrument]


@pytest.mark.asyncio
async def test_initialize_load_ids_forwards_filters_and_reload() -> None:
    """
    Configured IDs load once unless explicitly reloaded.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    calls = []

    class Provider(InstrumentProvider):
        async def load_ids_async(self, instrument_ids, filters=None):
            calls.append((instrument_ids, filters))
            self.add(instrument)

    provider = Provider(
        InstrumentProviderConfig(load_ids=[str(instrument.id)], filters={"market": "spot"}),
    )
    await provider.initialize()
    await provider.initialize()
    await provider.initialize(reload=True)

    assert calls == [([instrument.id], {"market": "spot"})] * 2
    assert provider.get_all() == {instrument.id: instrument}


@pytest.mark.asyncio
async def test_initialize_without_loading_configuration_stays_retryable(native_log) -> None:
    """
    No configured load leaves initialization available on later calls.
    """
    provider = InstrumentProvider()
    await provider.initialize()
    await provider.initialize()

    assert [call.args[0] for call in native_log.warning.call_args_list] == [
        "No instrument loading configured",
    ] * 2
    assert provider.get_all() == {}


@pytest.mark.asyncio
async def test_load_empty_ids_and_existing_instrument_skip_remote_load() -> None:
    """
    Only a missing instrument triggers the bulk fallback.
    """
    existing = TestInstrumentProvider.audusd_sim()
    requested = TestInstrumentProvider.usdjpy_sim()
    calls = []

    class Provider(InstrumentProvider):
        async def load_all_async(self, filters=None):
            calls.append(filters)
            self.add(requested)

    provider = Provider()
    provider.add(existing)
    await provider.load_ids_async([], {"unused": True})
    await provider.load_async(existing.id, {"unused": True})
    await provider.load_async(requested.id, {"market": "fx"})

    assert calls == [{"market": "fx"}]
    assert provider.get_all() == {requested.id: requested, existing.id: existing}


@pytest.mark.parametrize("method", ["load", "load_ids"])
def test_standalone_selected_loads_forward_arguments(method) -> None:
    """
    Synchronous entry points retain selection and filters.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    calls = []

    class Provider(InstrumentProvider):
        async def load_all_async(self, filters=None):
            calls.append(filters)
            self.add(instrument)

    provider = Provider()
    selection = instrument.id if method == "load" else [instrument.id]
    result = getattr(provider, method)(selection, {"market": "spot"})

    assert result is None
    assert calls == [{"market": "spot"}]
    assert provider.list_all() == [instrument]


@pytest.mark.asyncio
@pytest.mark.parametrize("method", ["load_all", "load_ids", "load"])
async def test_unbound_sync_entry_points_reject_running_loop(method) -> None:
    """
    An unowned provider cannot start unsupervised work on a running loop.
    """
    provider = InstrumentProvider()
    instrument = TestInstrumentProvider.audusd_sim()
    args = {"load_all": (), "load_ids": ([instrument.id],), "load": (instrument.id,)}

    with pytest.raises(RuntimeError, match="Await the async provider method"):
        getattr(provider, method)(*args[method])

    assert provider.get_all() == {}


def test_provider_binding_rejects_second_owner() -> None:
    """
    Repeated binding to one owner succeeds; ownership cannot transfer.
    """

    class Client:
        client_id = "PROVIDER"

    first_client = Client()
    second_client = Client()
    first = ClientRuntime(first_client)
    second = ClientRuntime(second_client)
    provider = InstrumentProvider()
    provider._bind(first)
    provider._bind(first)

    with pytest.raises(RuntimeError, match="cannot belong to multiple clients"):
        provider._bind(second)

    assert provider._runtime is first


def test_provider_rejects_foreign_instrument_identifier() -> None:
    """
    Invalid instrument identities never enter local storage.
    """

    class ForeignInstrument:
        id = "AUD/USD.SIM"

    provider = InstrumentProvider()
    with pytest.raises(TypeError, match="Nautilus InstrumentId"):
        provider.add(ForeignInstrument())

    assert provider.get_all() == {}


def test_provider_currency_storage_and_registry_fallback() -> None:
    """
    Currency snapshots cannot mutate provider storage.
    """
    provider = InstrumentProvider()
    usd = Currency.from_str("USD")
    eur = Currency.from_str("EUR")
    provider.add_currency(usd)
    snapshot = provider.currencies()
    snapshot.clear()

    assert provider.currencies() == {"USD": usd}
    assert provider.currency("USD") == usd
    assert provider.currency("EUR") == eur


@pytest.mark.parametrize("code", ["", None, 42])
def test_provider_rejects_invalid_currency_code(code) -> None:
    """
    Currency lookup requires a nonempty string.
    """
    with pytest.raises(ValueError, match="non-empty string"):
        InstrumentProvider().currency(code)


@pytest.mark.asyncio
async def test_base_provider_requires_a_loader() -> None:
    """
    A missing venue loader fails rather than marking an empty load successful.
    """
    provider = InstrumentProvider(InstrumentProviderConfig(load_all=True))
    with pytest.raises(NotImplementedError):
        await provider.initialize()
    assert provider.get_all() == {}
    assert provider._loaded is False
