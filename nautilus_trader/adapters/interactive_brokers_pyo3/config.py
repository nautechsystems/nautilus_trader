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
Configuration classes for the Interactive Brokers PyO3 adapter.

These classes wrap the Rust PyO3 bindings and provide the same interface as the Python
adapter configuration classes.

"""

from __future__ import annotations

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import ib_contract_specs_to_dicts
from nautilus_trader.config import RoutingConfig
from nautilus_trader.core.nautilus_pyo3 import InstrumentId as PyO3InstrumentId
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
    DockerizedIBGatewayConfig as RustDockerizedIBGatewayConfig,
)
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
    InteractiveBrokersDataClientConfig as RustInteractiveBrokersDataClientConfig,
)
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
    InteractiveBrokersExecClientConfig as RustInteractiveBrokersExecClientConfig,
)
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
    InteractiveBrokersInstrumentProviderConfig as RustInteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import MarketDataType
from nautilus_trader.core.nautilus_pyo3.interactive_brokers import TradingMode
from nautilus_trader.model.identifiers import InstrumentId


def _normalize_load_ids(load_ids):
    if load_ids is None:
        return None

    return {
        instrument_id
        if isinstance(instrument_id, PyO3InstrumentId)
        else PyO3InstrumentId.from_str(
            instrument_id.value if isinstance(instrument_id, InstrumentId) else str(instrument_id),
        )
        for instrument_id in load_ids
    }


def _normalize_legacy_load_ids(load_ids):
    if load_ids is None:
        return frozenset()

    return frozenset(
        instrument_id
        if isinstance(instrument_id, InstrumentId)
        else InstrumentId.from_str(
            instrument_id.value
            if isinstance(instrument_id, PyO3InstrumentId)
            else str(instrument_id),
        )
        for instrument_id in load_ids
    )


def _normalize_market_data_type(market_data_type):
    if market_data_type is None or isinstance(market_data_type, MarketDataType):
        return market_data_type

    mapping = {
        1: MarketDataType.Realtime,
        2: MarketDataType.Frozen,
        3: MarketDataType.Delayed,
        4: MarketDataType.DelayedFrozen,
    }

    return mapping.get(int(market_data_type), market_data_type)


def _normalize_trading_mode(trading_mode):
    if trading_mode is None or isinstance(trading_mode, TradingMode):
        return trading_mode if trading_mode is not None else TradingMode.Paper

    mapping = {
        "paper": TradingMode.Paper,
        "live": TradingMode.Live,
    }

    return mapping[str(trading_mode).lower()]


def _normalize_symbology_method(symbology_method):
    rust_symbology_enum = type(RustInteractiveBrokersInstrumentProviderConfig().symbology_method)

    if symbology_method is None or isinstance(symbology_method, rust_symbology_enum):
        return symbology_method

    value = getattr(symbology_method, "value", symbology_method)
    value = str(value).lower()
    mapping = {
        "simplified": rust_symbology_enum.Simplified,
        "raw": rust_symbology_enum.Raw,
    }

    return mapping.get(value, symbology_method)


class DockerizedIBGatewayConfig(RustDockerizedIBGatewayConfig):
    """
    Configuration for the dockerized IB Gateway using PyO3 bindings.
    """

    def __new__(
        cls,
        username: str | None = None,
        password: str | None = None,
        trading_mode: TradingMode | str | None = TradingMode.Paper,
        read_only_api: bool = True,
        timeout: int = 300,
        container_image: str = "ghcr.io/gnzsnz/ib-gateway:stable",
        vnc_port: int | None = None,
        **kwargs,
    ):
        return super().__new__(
            cls,
            username=username,
            password=password,
            trading_mode=_normalize_trading_mode(trading_mode),
            read_only_api=read_only_api,
            timeout=timeout,
            container_image=container_image,
            vnc_port=vnc_port,
        )


class InteractiveBrokersDataClientConfig(RustInteractiveBrokersDataClientConfig):
    """
    Configuration for `InteractiveBrokersDataClient` using PyO3 bindings.

    This class wraps the Rust implementation configuration and provides the same
    interface as the Python adapter.

    """

    def __new__(
        cls,
        ibg_host: str = "127.0.0.1",
        ibg_port: int | None = None,
        ibg_client_id: int = 1,
        use_regular_trading_hours: bool = True,
        market_data_type=None,  # MarketDataType
        ignore_quote_tick_size_updates: bool = False,
        dockerized_gateway=None,  # DockerizedIBGatewayConfig
        connection_timeout: int = 300,
        request_timeout: int | None = None,
        request_timeout_secs: int | None = None,
        handle_revised_bars: bool = False,
        batch_quotes: bool = True,
        instrument_provider: InteractiveBrokersInstrumentProviderConfig | None = None,
        routing: RoutingConfig | None = None,
        **kwargs,
    ):
        # Handle aliases
        host = ibg_host
        port = ibg_port if ibg_port is not None else 4002
        client_id = ibg_client_id
        request_timeout_value = (
            request_timeout if request_timeout is not None else request_timeout_secs
        )

        obj = super().__new__(
            cls,
            host=host,
            port=port,
            client_id=client_id,
            use_regular_trading_hours=use_regular_trading_hours,
            market_data_type=_normalize_market_data_type(market_data_type),
            ignore_quote_tick_size_updates=ignore_quote_tick_size_updates,
            connection_timeout=connection_timeout,
            request_timeout=request_timeout_value if request_timeout_value is not None else 60,
            handle_revised_bars=handle_revised_bars,
            batch_quotes=batch_quotes,
        )

        # Store v1-compatible attributes on the Python wrapper object.
        obj.dockerized_gateway = dockerized_gateway
        obj.instrument_provider = (
            instrument_provider
            if instrument_provider is not None
            else InteractiveBrokersInstrumentProviderConfig()
        )
        obj.routing = routing if routing is not None else RoutingConfig()
        obj.legacy_market_data_type = int(
            market_data_type if market_data_type is not None else MarketDataType.Realtime,
        )
        return obj

    @property
    def ibg_host(self) -> str:
        return self.host

    @property
    def ibg_port(self) -> int:
        return self.port

    @property
    def ibg_client_id(self) -> int:
        return self.client_id

    @property
    def request_timeout_secs(self) -> int:
        return self.request_timeout


class InteractiveBrokersExecClientConfig(RustInteractiveBrokersExecClientConfig):
    """
    Configuration for `InteractiveBrokersExecutionClient` using PyO3 bindings.

    This class wraps the Rust implementation configuration and provides the same
    interface as the Python adapter.

    """

    def __new__(
        cls,
        ibg_host: str = "127.0.0.1",
        ibg_port: int | None = None,
        ibg_client_id: int = 1,
        account_id: str | None = None,
        dockerized_gateway=None,  # DockerizedIBGatewayConfig
        connection_timeout: int = 300,
        request_timeout: int | None = None,
        request_timeout_secs: int | None = None,
        fetch_all_open_orders: bool = False,
        track_option_exercise_from_position_update: bool = False,
        instrument_provider: InteractiveBrokersInstrumentProviderConfig | None = None,
        routing: RoutingConfig | None = None,
        **kwargs,
    ):
        # Handle aliases
        host = ibg_host
        port = ibg_port if ibg_port is not None else 4002
        client_id = ibg_client_id
        request_timeout_value = (
            request_timeout if request_timeout is not None else request_timeout_secs
        )

        obj = super().__new__(
            cls,
            host=host,
            port=port,
            client_id=client_id,
            account_id=account_id,
            connection_timeout=connection_timeout,
            request_timeout=request_timeout_value if request_timeout_value is not None else 60,
            fetch_all_open_orders=fetch_all_open_orders,
            track_option_exercise_from_position_update=track_option_exercise_from_position_update,
        )

        # Store v1-compatible attributes on the Python wrapper object.
        obj.dockerized_gateway = dockerized_gateway
        obj.instrument_provider = (
            instrument_provider
            if instrument_provider is not None
            else InteractiveBrokersInstrumentProviderConfig()
        )
        obj.routing = routing if routing is not None else RoutingConfig()
        return obj

    @property
    def ibg_host(self) -> str:
        return self.host

    @property
    def ibg_port(self) -> int:
        return self.port

    @property
    def ibg_client_id(self) -> int:
        return self.client_id

    @property
    def request_timeout_secs(self) -> int:
        return self.request_timeout


class InteractiveBrokersInstrumentProviderConfig(RustInteractiveBrokersInstrumentProviderConfig):
    """
    Configuration for `InteractiveBrokersInstrumentProvider` using PyO3 bindings.

    This class wraps the Rust implementation configuration and provides the same
    interface as the Python adapter.

    """

    @property
    def load_all(self) -> bool:
        return False

    @property
    def filters(self) -> None:
        return None

    @property
    def pickle_path(self) -> str | None:
        return self.cache_path

    @pickle_path.setter
    def pickle_path(self, value: str | None) -> None:
        self.cache_path = value

    def __new__(
        cls,
        load_ids=None,
        load_contracts=None,
        symbology_method=None,
        min_expiry_days=None,
        max_expiry_days=None,
        build_options_chain=None,
        build_futures_chain=None,
        cache_validity_days=None,
        convert_exchange_to_mic_venue=None,
        symbol_to_mic_venue=None,
        filter_sec_types=None,
        pickle_path=None,  # Mapped to cache_path
        **kwargs,
    ):
        load_ids_normalized = _normalize_load_ids(load_ids)
        load_contract_dicts = ib_contract_specs_to_dicts(load_contracts)
        obj = super().__new__(
            cls,
            symbology_method=_normalize_symbology_method(symbology_method),
            load_ids=load_ids_normalized,
            load_contracts=load_contract_dicts,
            min_expiry_days=min_expiry_days,
            max_expiry_days=max_expiry_days,
            build_options_chain=build_options_chain,
            build_futures_chain=build_futures_chain,
            cache_validity_days=cache_validity_days,
            convert_exchange_to_mic_venue=convert_exchange_to_mic_venue,
            symbol_to_mic_venue=symbol_to_mic_venue,
            filter_sec_types=filter_sec_types,
            cache_path=pickle_path,
        )
        obj.legacy_load_ids = _normalize_legacy_load_ids(load_ids)
        obj.legacy_load_contracts = (
            frozenset(IBContract(**contract) for contract in load_contract_dicts)
            if load_contract_dicts
            else None
        )
        obj.legacy_symbology_method = symbology_method
        return obj
