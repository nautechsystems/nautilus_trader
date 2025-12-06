# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.botfolio.config import BotfolioDataClientConfig
from nautilus_trader.adapters.botfolio.config import BotfolioExecClientConfig
from nautilus_trader.adapters.botfolio.data import BotfolioDataClient
from nautilus_trader.adapters.botfolio.execution import BotfolioExecutionClient
from nautilus_trader.adapters.botfolio.providers import BotfolioInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_botfolio_instrument_provider(
    clock: LiveClock,
    config: InstrumentProviderConfig,
) -> BotfolioInstrumentProvider:
    """
    Cache and return a Botfolio instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    clock : LiveClock
        The clock for the provider.
    config : InstrumentProviderConfig
        The configuration for the provider.

    Returns
    -------
    BotfolioInstrumentProvider

    """
    return BotfolioInstrumentProvider(
        clock=clock,
        config=config,
    )


class BotfolioLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Botfolio live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BotfolioDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BotfolioDataClient:
        """
        Create a new Botfolio data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BotfolioDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BotfolioDataClient

        """
        # Get instrument provider singleton
        provider = get_cached_botfolio_instrument_provider(
            clock=clock,
            config=config.instrument_provider,
        )

        return BotfolioDataClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class BotfolioLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Botfolio live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BotfolioExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BotfolioExecutionClient:
        """
        Create a new Botfolio execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BotfolioExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BotfolioExecutionClient

        """
        # Get instrument provider singleton
        provider = get_cached_botfolio_instrument_provider(
            clock=clock,
            config=config.instrument_provider,
        )

        return BotfolioExecutionClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )

