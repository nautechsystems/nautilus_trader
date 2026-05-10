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

from __future__ import annotations

import asyncio
import inspect
from collections.abc import Coroutine
from typing import Any
from typing import cast

from nautilus_trader.adapters.interactive_brokers_pyo3.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersDataClientConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersExecClientConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers_pyo3.execution import (
    InteractiveBrokersExecutionClient,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import DockerizedIBGateway
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


GATEWAYS: dict[tuple, DockerizedIBGateway] = {}
IB_INSTRUMENT_PROVIDERS: dict[tuple, InteractiveBrokersInstrumentProvider] = {}


def _safe_start_gateway(
    loop: asyncio.AbstractEventLoop,
    gateway: DockerizedIBGateway,
    wait: int | None,
) -> None:
    safe_start_blocking = getattr(gateway, "safe_start_blocking", None)
    if callable(safe_start_blocking):
        safe_start_blocking(wait=wait)
        return

    startup = gateway.safe_start(wait=wait)
    if not inspect.iscoroutine(startup):
        return
    startup_coro = cast(Coroutine[Any, Any, Any], startup)

    if loop.is_running():
        try:
            running_loop = asyncio.get_running_loop()
        except RuntimeError:
            running_loop = None

        if running_loop is loop:
            raise RuntimeError(
                "Cannot synchronously start DockerizedIBGateway on an already running event loop",
            )

        asyncio.run_coroutine_threadsafe(startup_coro, loop).result()
        return

    loop.run_until_complete(startup_coro)


def _coerce_dockerized_gateway_config(
    config: object,
) -> DockerizedIBGatewayConfig:
    if isinstance(config, DockerizedIBGatewayConfig):
        return config

    return DockerizedIBGatewayConfig(
        username=getattr(config, "username", None),
        password=getattr(config, "password", None),
        trading_mode=getattr(config, "trading_mode", None),
        read_only_api=getattr(config, "read_only_api", True),
        timeout=getattr(config, "timeout", 300),
        container_image=getattr(
            config,
            "container_image",
            "ghcr.io/gnzsnz/ib-gateway:stable",
        ),
        vnc_port=getattr(config, "vnc_port", None),
    )


def _resolve_connection(
    loop: asyncio.AbstractEventLoop,
    config: InteractiveBrokersDataClientConfig | InteractiveBrokersExecClientConfig,
) -> tuple[str, int]:
    dockerized_gateway = getattr(config, "dockerized_gateway", None)
    host = config.ibg_host
    port = config.ibg_port

    if dockerized_gateway is None:
        PyCondition.not_none(
            host,
            "Please provide the `host` IP address for the IB TWS or Gateway.",
        )
        PyCondition.not_none(port, "Please provide the `port` for the IB TWS or Gateway.")
        return host, port

    PyCondition.equal(host, "127.0.0.1", "host", "127.0.0.1")
    dockerized_gateway = _coerce_dockerized_gateway_config(dockerized_gateway)

    gateway_key = (dockerized_gateway.trading_mode,)
    gateway = GATEWAYS.get(gateway_key)
    if gateway is None:
        gateway = DockerizedIBGateway(dockerized_gateway)
        _safe_start_gateway(loop, gateway, wait=dockerized_gateway.timeout)
        GATEWAYS[gateway_key] = gateway

    return host, gateway.port


def _clone_data_config(
    config: InteractiveBrokersDataClientConfig,
    *,
    host: str,
    port: int,
) -> InteractiveBrokersDataClientConfig:
    return InteractiveBrokersDataClientConfig(
        ibg_host=host,
        ibg_port=port,
        ibg_client_id=config.ibg_client_id,
        use_regular_trading_hours=config.use_regular_trading_hours,
        market_data_type=getattr(config, "legacy_market_data_type", config.market_data_type),
        ignore_quote_tick_size_updates=config.ignore_quote_tick_size_updates,
        dockerized_gateway=config.dockerized_gateway,
        connection_timeout=config.connection_timeout,
        request_timeout=config.request_timeout,
        handle_revised_bars=config.handle_revised_bars,
        batch_quotes=config.batch_quotes,
        instrument_provider=config.instrument_provider,
        routing=config.routing,
    )


def _clone_exec_config(
    config: InteractiveBrokersExecClientConfig,
    *,
    host: str,
    port: int,
) -> InteractiveBrokersExecClientConfig:
    return InteractiveBrokersExecClientConfig(
        ibg_host=host,
        ibg_port=port,
        ibg_client_id=config.ibg_client_id,
        account_id=config.account_id,
        dockerized_gateway=config.dockerized_gateway,
        connection_timeout=config.connection_timeout,
        request_timeout=config.request_timeout,
        fetch_all_open_orders=config.fetch_all_open_orders,
        track_option_exercise_from_position_update=config.track_option_exercise_from_position_update,
        instrument_provider=config.instrument_provider,
        routing=config.routing,
    )


def _freeze_cache_value(value: object) -> object:
    if isinstance(value, dict):
        return tuple(sorted((key, _freeze_cache_value(item)) for key, item in value.items()))
    if isinstance(value, (list, tuple, set, frozenset)):
        return tuple(_freeze_cache_value(item) for item in value)

    raw_value = getattr(value, "value", None)
    if raw_value is not None and not isinstance(value, (str, bytes)):
        return raw_value

    return value


def _provider_cache_key(
    config: InteractiveBrokersDataClientConfig | InteractiveBrokersExecClientConfig,
) -> tuple[object, ...]:
    instrument_provider = getattr(config, "instrument_provider", None)
    if instrument_provider is None:
        raise ValueError("Interactive Brokers config requires an instrument_provider")

    return (
        config.ibg_host,
        config.ibg_port,
        _freeze_cache_value(getattr(instrument_provider, "legacy_load_ids", None)),
        _freeze_cache_value(getattr(instrument_provider, "legacy_load_contracts", None)),
        getattr(instrument_provider, "legacy_symbology_method", None),
        getattr(instrument_provider, "min_expiry_days", None),
        getattr(instrument_provider, "max_expiry_days", None),
        getattr(instrument_provider, "build_options_chain", None),
        getattr(instrument_provider, "build_futures_chain", None),
        getattr(instrument_provider, "cache_validity_days", None),
        getattr(instrument_provider, "pickle_path", None),
        bool(getattr(instrument_provider, "convert_exchange_to_mic_venue", False)),
        _freeze_cache_value(getattr(instrument_provider, "symbol_to_mic_venue", None)),
        _freeze_cache_value(getattr(instrument_provider, "filter_sec_types", None)),
    )


def _build_provider(
    config: InteractiveBrokersDataClientConfig | InteractiveBrokersExecClientConfig,
) -> InteractiveBrokersInstrumentProvider:
    instrument_provider = getattr(config, "instrument_provider", None)
    if instrument_provider is None:
        raise ValueError("Interactive Brokers config requires an instrument_provider")

    cache_key = _provider_cache_key(config)
    provider = IB_INSTRUMENT_PROVIDERS.get(cache_key)
    if provider is None:
        provider = InteractiveBrokersInstrumentProvider(config=instrument_provider)
        IB_INSTRUMENT_PROVIDERS[cache_key] = provider

    return provider


class InteractiveBrokersV1LiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a v1-compatible factory for the Interactive Brokers PyO3 adapter.

    The v1 node still expects Python `Live*Client` implementations, but those wrappers
    should delegate into the PyO3-backed IB adapter directly rather than routing back
    into the legacy Python IB adapter.

    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ):
        host, port = _resolve_connection(loop, config)
        resolved_config = _clone_data_config(config, host=host, port=port)
        provider = _build_provider(resolved_config)
        return InteractiveBrokersDataClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=resolved_config,
            name=name,
        )


class InteractiveBrokersV1LiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a v1-compatible factory for the Interactive Brokers PyO3 adapter.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: InteractiveBrokersExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ):
        host, port = _resolve_connection(loop, config)
        resolved_config = _clone_exec_config(config, host=host, port=port)
        provider = _build_provider(resolved_config)
        return InteractiveBrokersExecutionClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=resolved_config,
            name=name,
        )


# Backward-compatible aliases for the v1/Cython node path.
InteractiveBrokersLiveDataClientFactory = InteractiveBrokersV1LiveDataClientFactory
InteractiveBrokersLiveExecClientFactory = InteractiveBrokersV1LiveExecClientFactory
