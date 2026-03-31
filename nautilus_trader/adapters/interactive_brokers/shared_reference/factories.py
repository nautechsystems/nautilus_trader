from __future__ import annotations

import asyncio

import redis

from nautilus_trader.adapters.interactive_brokers.shared_reference.config import (
    InteractiveBrokersSharedReferenceDataClientConfig,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    InteractiveBrokersSharedReferenceDataClient,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    SharedReferenceInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory


def _build_redis_client(config: InteractiveBrokersSharedReferenceDataClientConfig) -> redis.Redis:
    return redis.Redis(
        host=config.redis_host,
        port=config.redis_port,
        db=config.redis_db,
        username=config.redis_username,
        password=config.redis_password,
        ssl=config.redis_ssl,
        socket_connect_timeout=config.redis_connect_timeout_secs,
        socket_timeout=config.redis_read_timeout_secs,
        decode_responses=False,
    )


class InteractiveBrokersSharedReferenceLiveDataClientFactory(LiveDataClientFactory):
    @staticmethod
    def create(  # type: ignore[override]
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersSharedReferenceDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> InteractiveBrokersSharedReferenceDataClient:
        redis_client = _build_redis_client(config)
        instrument_provider = SharedReferenceInstrumentProvider(
            config=config.instrument_provider,
        )
        return InteractiveBrokersSharedReferenceDataClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
            redis_client=redis_client,
            name=name,
        )


__all__ = ["InteractiveBrokersSharedReferenceLiveDataClientFactory"]
