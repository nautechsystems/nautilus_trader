# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from types import SimpleNamespace

import nautilus_trader.adapters.bitget as bitget
import nautilus_trader.adapters.bitget.factories as bitget_factories


def test_bitget_package_exports_expected_symbols() -> None:
    assert "BITGET" in bitget.__all__
    assert "BitgetDataClientConfig" in bitget.__all__
    assert "BitgetExecClientConfig" in bitget.__all__
    assert "BitgetInstrumentProvider" in bitget.__all__
    assert "BitgetLiveDataClientFactory" in bitget.__all__
    assert "BitgetLiveExecClientFactory" in bitget.__all__


def test_get_cached_bitget_http_client_reuses_cached_instance(monkeypatch) -> None:
    calls: list[tuple[object, str, str, str]] = []
    sentinel = object()

    bitget_factories.get_cached_bitget_http_client.cache_clear()
    monkeypatch.setattr(
        bitget_factories.nautilus_pyo3,
        "BitgetEnvironment",
        SimpleNamespace(MAINNET="mainnet", DEMO="demo"),
        raising=False,
    )
    monkeypatch.setattr(
        bitget_factories.nautilus_pyo3,
        "BitgetHttpClient",
        SimpleNamespace(
            with_credentials=lambda environment, api_key, api_secret, api_passphrase: (
                calls.append((environment, api_key, api_secret, api_passphrase)) or sentinel
            ),
        ),
        raising=False,
    )

    client1 = bitget_factories.get_cached_bitget_http_client(
        api_key="key",
        api_secret="secret",
        api_passphrase="pass",
        demo=True,
    )
    client2 = bitget_factories.get_cached_bitget_http_client(
        api_key="key",
        api_secret="secret",
        api_passphrase="pass",
        demo=True,
    )

    assert client1 is sentinel
    assert client2 is sentinel
    assert calls == [("demo", "key", "secret", "pass")]


def test_get_cached_bitget_instrument_provider_reuses_cached_instance() -> None:
    bitget_factories.get_cached_bitget_instrument_provider.cache_clear()

    client = object()

    provider1 = bitget_factories.get_cached_bitget_instrument_provider(client=client, config=None)
    provider2 = bitget_factories.get_cached_bitget_instrument_provider(client=client, config=None)

    assert provider1 is provider2


def test_create_bitget_live_data_client_wires_cached_client_and_provider(monkeypatch) -> None:
    captured: list[dict] = []
    cached_client = object()
    cached_provider = object()

    monkeypatch.setattr(
        bitget_factories,
        "get_cached_bitget_http_client",
        lambda **kwargs: cached_client,
    )
    monkeypatch.setattr(
        bitget_factories,
        "get_cached_bitget_instrument_provider",
        lambda **kwargs: cached_provider,
    )
    monkeypatch.setattr(
        bitget_factories,
        "BitgetDataClient",
        lambda **kwargs: captured.append(kwargs) or SimpleNamespace(**kwargs),
    )

    config = SimpleNamespace(
        api_key="key",
        api_secret="secret",
        api_passphrase="pass",
        demo=False,
        instrument_provider="provider-config",
        product_types=None,
    )
    loop = object()
    msgbus = object()
    cache = object()
    clock = object()

    client = bitget_factories.BitgetLiveDataClientFactory.create(
        loop=loop,
        name="BITGET",
        config=config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    assert client.client is cached_client
    assert client.instrument_provider is cached_provider
    assert client.name == "BITGET"
    assert client.config is config
    assert captured[0]["loop"] is loop
    assert captured[0]["msgbus"] is msgbus
    assert captured[0]["cache"] is cache
    assert captured[0]["clock"] is clock


def test_create_bitget_live_exec_client_wires_cached_client_and_provider(monkeypatch) -> None:
    captured: list[dict] = []
    cached_client = object()
    cached_provider = object()

    monkeypatch.setattr(
        bitget_factories,
        "get_cached_bitget_http_client",
        lambda **kwargs: cached_client,
    )
    monkeypatch.setattr(
        bitget_factories,
        "get_cached_bitget_instrument_provider",
        lambda **kwargs: cached_provider,
    )
    monkeypatch.setattr(
        bitget_factories,
        "BitgetExecutionClient",
        lambda **kwargs: captured.append(kwargs) or SimpleNamespace(**kwargs),
    )

    config = SimpleNamespace(
        api_key="key",
        api_secret="secret",
        api_passphrase="pass",
        demo=True,
        instrument_provider="provider-config",
    )
    loop = object()
    msgbus = object()
    cache = object()
    clock = object()

    client = bitget_factories.BitgetLiveExecClientFactory.create(
        loop=loop,
        name="BITGET",
        config=config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    assert client.client is cached_client
    assert client.instrument_provider is cached_provider
    assert client.name == "BITGET"
    assert client.config is config
    assert captured[0]["loop"] is loop
    assert captured[0]["msgbus"] is msgbus
    assert captured[0]["cache"] is cache
    assert captured[0]["clock"] is clock
