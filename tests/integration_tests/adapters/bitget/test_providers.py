# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import pytest
from types import SimpleNamespace

from nautilus_trader.adapters.bitget.providers import BitgetInstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId


@pytest.mark.asyncio
async def test_load_all_async_without_request_method_is_safe() -> None:
    provider = BitgetInstrumentProvider(client=object())
    await provider.load_all_async()
    assert provider.instruments_pyo3() == []


@pytest.mark.asyncio
async def test_load_ids_async_enforces_bitget_venue() -> None:
    provider = BitgetInstrumentProvider(client=object())

    with pytest.raises(ValueError):
        await provider.load_ids_async([InstrumentId.from_str("BTCUSDT.BINANCE")])


@pytest.mark.asyncio
async def test_load_ids_async_filters_cached_instruments_and_raw_pyo3(monkeypatch) -> None:
    btcusdt = InstrumentId.from_str("BTCUSDT.BITGET")
    ethusdt = InstrumentId.from_str("ETHUSDT.BITGET")
    btc_raw = object()
    eth_raw = object()
    btc_instrument = SimpleNamespace(id=btcusdt)
    eth_instrument = SimpleNamespace(id=ethusdt)

    async def request_instruments():
        return [btc_raw, eth_raw]

    monkeypatch.setattr(
        "nautilus_trader.adapters.bitget.providers.instruments_from_pyo3",
        lambda pyo3_instruments: [btc_instrument, eth_instrument],
    )

    provider = BitgetInstrumentProvider(client=SimpleNamespace(request_instruments=request_instruments))

    await provider.load_ids_async([btcusdt])

    assert provider.count == 1
    assert provider.instruments_pyo3() == [btc_raw]
