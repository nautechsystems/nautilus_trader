import asyncio
from functools import lru_cache
from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.data import BybitDataClient
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3.network import Quota
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.utils.env import get_env_key


HTTP_CLIENTS: dict[str, BybitHttpClient] = {}


@lru_cache(1)
def get_cached_bybit_http_client(
    clock: LiveClock,
    logger: Logger,
    key: Optional[str] = None,
    secret: Optional[str] = None,
    base_url: Optional[str] = None,
    is_testnet: bool = False,
):
    """
    Cache and return a Binance HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that cached
    client will be returned.

    """
    global HTTP_CLIENTS
    key = key or _get_api_key(is_testnet)
    secret = secret or _get_api_secret(is_testnet)
    http_base_url = base_url or _get_http_base_url(is_testnet)
    client_key: str = "|".join((key, secret))

    # setup rate limit quotas
    ratelimiter_default_quota = Quota.rate_per_second(120)
    ratelimiter_quotas: list[tuple[str, Quota]] = []

    if client_key not in HTTP_CLIENTS:
        client = BybitHttpClient(
            clock=clock,
            logger=logger,
            api_key=key,
            api_secret=secret,
            base_url=http_base_url,
            ratelimiter_quotas=ratelimiter_quotas,
            ratelimiter_default_quota=ratelimiter_default_quota,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]


@lru_cache(1)
def get_cached_bybit_instrument_provider(
    client: BybitHttpClient,
    logger: Logger,
    clock: LiveClock,
    instrument_type: BybitInstrumentType,
    is_testnet: bool,
    config: InstrumentProviderConfig,
) -> BybitInstrumentProvider:
    return BybitInstrumentProvider(
        client=client,
        logger=logger,
        config=config,
        clock=clock,
        instrument_type=instrument_type,
        is_testnet=is_testnet,
    )


class BybitLiveDataClientFactory(LiveDataClientFactory):
    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BybitDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
    ) -> BybitDataClient:
        client: BybitHttpClient = get_cached_bybit_http_client(
            clock=clock,
            logger=logger,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.testnet,
        )
        provider = get_cached_bybit_instrument_provider(
            client=client,
            logger=logger,
            clock=clock,
            instrument_type=config.instrument_type,
            is_testnet=config.testnet,
            config=config.instrument_provider,
        )
        default_base_url_ws: str = _get_ws_base_url(
            instrument_type=config.instrument_type,
            is_testnet=config.testnet,
        )

        return BybitDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            instrument_type=config.instrument_type,
            base_url_ws=config.base_url_ws or default_base_url_ws,
            config=config,
        )


class BybitLiveExecClientFactory(LiveExecClientFactory):
    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BybitExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
    ) -> BybitExecutionClient:
        client: BybitHttpClient = get_cached_bybit_http_client(
            clock=clock,
            logger=logger,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.testnet,
        )
        provider = get_cached_bybit_instrument_provider(
            client=client,
            logger=logger,
            clock=clock,
            instrument_type=config.instrument_type,
            is_testnet=config.testnet,
            config=config.instrument_provider,
        )
        default_base_url_ws: str = _get_ws_base_url(
            instrument_type=config.instrument_type,
            is_testnet=config.testnet,
        )
        return BybitExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=provider,
            instrument_type=config.instrument_type,
            base_url_ws=config.base_url_ws or default_base_url_ws,
            config=config,
        )


def _get_api_key(is_testnet: bool) -> str:
    if is_testnet:
        return get_env_key("BYBIT_TESTNET_API_KEY")
    else:
        return get_env_key("BYBIT_API_KEY")


def _get_api_secret(is_testnet: bool) -> str:
    if is_testnet:
        return get_env_key("BYBIT_TESTNET_API_SECRET")
    else:
        return get_env_key("BYBIT_API_SECRET")


def _get_http_base_url(is_testnet: bool):
    if is_testnet:
        return "https://api-testnet.bybit.com"
    else:
        return "https://api.bytick.com"


def _get_ws_base_url(instrument_type: BybitInstrumentType, is_testnet: bool):
    if not is_testnet:
        if instrument_type == BybitInstrumentType.SPOT:
            return "wss://stream.bybit.com/v5/public/spot"
        elif instrument_type == BybitInstrumentType.LINEAR:
            return "wss://stream.bybit.com/v5/public/linear"
        elif instrument_type == BybitInstrumentType.INVERSE:
            return "wss://stream.bybit.com/v5/public/inverse"
        else:
            raise RuntimeError(
                f"invalid `BybitAccountType`, was {instrument_type}",  # pragma: no cover
            )
    else:
        if instrument_type == BybitInstrumentType.SPOT:
            return "wss://stream-testnet.bybit.com/v5/public/spot"
        elif instrument_type == BybitInstrumentType.LINEAR:
            return "wss://stream-testnet.bybit.com/v5/public/linear"
        elif instrument_type == BybitInstrumentType.INVERSE:
            return "wss://stream-testnet.bybit.com/v5/public/inverse"
        else:
            raise RuntimeError(
                f"invalid `BybitAccountType`, was {instrument_type}",  # pragma: no cover
            )
