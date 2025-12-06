# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.alpaca.config import AlpacaDataClientConfig
from nautilus_trader.adapters.alpaca.config import AlpacaExecClientConfig
from nautilus_trader.adapters.alpaca.credentials import resolve_credentials
from nautilus_trader.adapters.alpaca.data import AlpacaDataClient
from nautilus_trader.adapters.alpaca.execution import AlpacaExecutionClient
from nautilus_trader.adapters.alpaca.http.client import AlpacaHttpClient
from nautilus_trader.adapters.alpaca.providers import AlpacaInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_alpaca_http_client(
    clock: LiveClock,
    api_key: str | None = None,
    api_secret: str | None = None,
    access_token: str | None = None,
    paper: bool = True,
) -> AlpacaHttpClient:
    """
    Cache and return an Alpaca HTTP client.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    api_key : str, optional
        The Alpaca API key.
    api_secret : str, optional
        The Alpaca API secret.
    access_token : str, optional
        The Alpaca OAuth access token.
    paper : bool, default True
        If using paper trading.

    Returns
    -------
    AlpacaHttpClient

    """
    resolved_key, resolved_secret, resolved_token = resolve_credentials(
        api_key, api_secret, access_token
    )

    return AlpacaHttpClient(
        clock=clock,
        api_key=resolved_key,
        api_secret=resolved_secret,
        access_token=resolved_token,
        paper=paper,
    )


@lru_cache(1)
def get_cached_alpaca_instrument_provider(
    client: AlpacaHttpClient,
    clock: LiveClock,
    config: InstrumentProviderConfig,
) -> AlpacaInstrumentProvider:
    """
    Cache and return an Alpaca instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : AlpacaHttpClient
        The HTTP client for the provider.
    clock : LiveClock
        The clock for the provider.
    config : InstrumentProviderConfig
        The configuration for the provider.

    Returns
    -------
    AlpacaInstrumentProvider

    """
    return AlpacaInstrumentProvider(
        client=client,
        clock=clock,
        config=config,
    )


class AlpacaLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides an Alpaca live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: AlpacaDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> AlpacaDataClient:
        """
        Create a new Alpaca data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : AlpacaDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        AlpacaDataClient

        """
        # Get HTTP client singleton
        client = get_cached_alpaca_http_client(
            clock=clock,
            api_key=config.api_key,
            api_secret=config.api_secret,
            access_token=config.access_token,
            paper=config.paper,
        )

        # Get instrument provider singleton
        provider = get_cached_alpaca_instrument_provider(
            client=client,
            clock=clock,
            config=config.instrument_provider,
        )

        return AlpacaDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class AlpacaLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides an Alpaca live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: AlpacaExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> AlpacaExecutionClient:
        """
        Create a new Alpaca execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : AlpacaExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        AlpacaExecutionClient

        """
        # Get HTTP client singleton
        client = get_cached_alpaca_http_client(
            clock=clock,
            api_key=config.api_key,
            api_secret=config.api_secret,
            access_token=config.access_token,
            paper=config.paper,
        )

        # Get instrument provider singleton
        provider = get_cached_alpaca_instrument_provider(
            client=client,
            clock=clock,
            config=config.instrument_provider,
        )

        return AlpacaExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )

