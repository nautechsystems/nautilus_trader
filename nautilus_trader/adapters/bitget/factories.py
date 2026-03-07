# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.bitget.config import BitgetDataClientConfig
from nautilus_trader.adapters.bitget.config import BitgetExecClientConfig
from nautilus_trader.adapters.bitget.data import BitgetDataClient
from nautilus_trader.adapters.bitget.execution import BitgetExecutionClient
from nautilus_trader.adapters.bitget.providers import BitgetInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_bitget_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    api_passphrase: str | None = None,
    demo: bool = False,
) -> object:
    """Cache and return a Bitget HTTP client instance."""
    bitget_environment = nautilus_pyo3.BitgetEnvironment  # type: ignore[attr-defined]
    bitget_http_client = nautilus_pyo3.BitgetHttpClient  # type: ignore[attr-defined]
    environment = bitget_environment.DEMO if demo else bitget_environment.MAINNET
    return bitget_http_client.with_credentials(
        environment,
        api_key or "",
        api_secret or "",
        api_passphrase or "",
    )


@lru_cache(1)
def get_cached_bitget_instrument_provider(
    client: object,
    config: InstrumentProviderConfig | None = None,
) -> BitgetInstrumentProvider:
    """Cache and return a Bitget instrument provider."""
    return BitgetInstrumentProvider(client=client, config=config)


class BitgetLiveDataClientFactory(LiveDataClientFactory):
    """Provides a Bitget live data client factory."""

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BitgetDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BitgetDataClient:
        client = get_cached_bitget_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            demo=config.demo,
        )

        provider = get_cached_bitget_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )

        return BitgetDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class BitgetLiveExecClientFactory(LiveExecClientFactory):
    """Provides a Bitget live execution client factory."""

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BitgetExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BitgetExecutionClient:
        client = get_cached_bitget_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            demo=config.demo,
        )

        provider = get_cached_bitget_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )

        return BitgetExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
