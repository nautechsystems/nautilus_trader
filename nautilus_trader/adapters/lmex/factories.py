# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX client factories.

These are the entry points consumed by the NautilusTrader live engine when
constructing data and execution clients from configuration objects.
"""

from __future__ import annotations

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.lmex.config import LmexDataClientConfig
from nautilus_trader.adapters.lmex.config import LmexExecClientConfig
from nautilus_trader.adapters.lmex.config import resolve_api_key
from nautilus_trader.adapters.lmex.config import resolve_api_secret
from nautilus_trader.adapters.lmex.constants import LMEX_BASE_URL_LIVE
from nautilus_trader.adapters.lmex.constants import LMEX_BASE_URL_SANDBOX
from nautilus_trader.adapters.lmex.constants import LMEX_WS_URL_LIVE
from nautilus_trader.adapters.lmex.constants import LMEX_WS_URL_SANDBOX
from nautilus_trader.adapters.lmex.data import LmexLiveMarketDataClient
from nautilus_trader.adapters.lmex.execution import LmexLiveExecutionClient
from nautilus_trader.adapters.lmex.http.client import LmexHttpClient
from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
from nautilus_trader.adapters.lmex.websocket.client import LmexWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(maxsize=4)
def get_cached_lmex_http_client(
    api_key: str | None,
    api_secret: str | None,
    base_url: str,
    proxy_url: str | None = None,
) -> LmexHttpClient:
    """
    Cache and return an ``LmexHttpClient`` for the given credentials.

    Returns the same cached instance for identical parameter combinations so
    that the data and execution clients can share a single connection pool.

    Parameters
    ----------
    api_key : str or None
        The LMEX API public key.
    api_secret : str or None
        The LMEX API secret.
    base_url : str
        The REST base URL (live or sandbox).
    proxy_url : str or None, optional
        Optional HTTP proxy URL.

    Returns
    -------
    LmexHttpClient

    """
    from nautilus_trader.common.component import LiveClock as _LiveClock  # noqa: PLC0415
    clock = _LiveClock()  # standalone clock for the HTTP client
    return LmexHttpClient(
        clock=clock,
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        proxy_url=proxy_url,
    )


@lru_cache(maxsize=4)
def get_cached_lmex_instrument_provider(
    market_api_key: str,
    config: InstrumentProviderConfig | None = None,
) -> LmexInstrumentProvider:
    """
    Cache and return an ``LmexInstrumentProvider``.

    Parameters
    ----------
    market_api_key : str
        Cache key — the REST base URL used by the HTTP client.  This ensures
        live and sandbox environments get separate providers.
    config : InstrumentProviderConfig, optional
        Provider configuration.

    Returns
    -------
    LmexInstrumentProvider

    Notes
    -----
    The ``market_api_key`` parameter is used only for cache keying; the
    actual HTTP client is retrieved by the caller and injected separately.

    """
    # Caller must inject the correct http client; this factory is a placeholder
    # for the lru_cache key.  The real provider is built in the factory method.
    raise NotImplementedError(
        "Use LmexLiveDataClientFactory.create() to obtain a provider."
    )


class LmexLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a factory for creating ``LmexLiveMarketDataClient`` instances.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LmexDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> LmexLiveMarketDataClient:
        """
        Create and return a new ``LmexLiveMarketDataClient``.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            Custom client ID (passed as ``name`` to the client).
        config : LmexDataClientConfig
            The data client configuration.
        msgbus : MessageBus
            The message bus.
        cache : Cache
            The data cache.
        clock : LiveClock
            The clock.

        Returns
        -------
        LmexLiveMarketDataClient

        """
        api_key = resolve_api_key(config.api_key, config.is_sandbox)
        api_secret = resolve_api_secret(config.api_secret, config.is_sandbox)

        base_url_http = (
            config.base_url_http
            or (LMEX_BASE_URL_SANDBOX if config.is_sandbox else LMEX_BASE_URL_LIVE)
        )
        base_url_ws = (
            config.base_url_ws
            or (LMEX_WS_URL_SANDBOX if config.is_sandbox else LMEX_WS_URL_LIVE)
        )

        http_client = get_cached_lmex_http_client(
            api_key=api_key,
            api_secret=api_secret,
            base_url=base_url_http,
            proxy_url=config.proxy_url,
        )

        market_api = LmexMarketHttpAPI(http_client)

        provider = LmexInstrumentProvider(
            market_api=market_api,
            config=config.instrument_provider,
        )

        ws_client = LmexWebSocketClient(
            clock=clock,
            base_url=base_url_ws,
            handler=lambda raw: None,  # overridden by data client below
            handler_reconnect=None,
            loop=loop,
            proxy_url=config.proxy_url,
        )

        data_client = LmexLiveMarketDataClient(
            loop=loop,
            http_client=http_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )

        # Wire the WS handler to the data client's message router
        ws_client._handler = data_client._handle_msg

        return data_client


class LmexLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a factory for creating ``LmexLiveExecutionClient`` instances.
    """

    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LmexExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> LmexLiveExecutionClient:
        """
        Create and return a new ``LmexLiveExecutionClient``.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            Custom client ID.
        config : LmexExecClientConfig
            The execution client configuration.
        msgbus : MessageBus
            The message bus.
        cache : Cache
            The data cache.
        clock : LiveClock
            The clock.

        Returns
        -------
        LmexLiveExecutionClient

        """
        api_key = resolve_api_key(config.api_key, config.is_sandbox)
        api_secret = resolve_api_secret(config.api_secret, config.is_sandbox)

        base_url_http = (
            config.base_url_http
            or (LMEX_BASE_URL_SANDBOX if config.is_sandbox else LMEX_BASE_URL_LIVE)
        )
        base_url_ws = (
            config.base_url_ws
            or (LMEX_WS_URL_SANDBOX if config.is_sandbox else LMEX_WS_URL_LIVE)
        )

        http_client = get_cached_lmex_http_client(
            api_key=api_key,
            api_secret=api_secret,
            base_url=base_url_http,
            proxy_url=config.proxy_url,
        )

        market_api = LmexMarketHttpAPI(http_client)

        provider = LmexInstrumentProvider(
            market_api=market_api,
            config=config.instrument_provider,
        )

        ws_client = LmexWebSocketClient(
            clock=clock,
            base_url=base_url_ws,
            handler=lambda raw: None,  # exec client uses handle_ws_message directly
            handler_reconnect=None,
            loop=loop,
            proxy_url=config.proxy_url,
        )

        exec_client = LmexLiveExecutionClient(
            loop=loop,
            http_client=http_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )

        # Wire the WS handler to the execution client's order event router
        ws_client._handler = exec_client.handle_ws_message

        return exec_client
