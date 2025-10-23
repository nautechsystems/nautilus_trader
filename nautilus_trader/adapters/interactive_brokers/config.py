# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from enum import Enum
from typing import Literal

from ibapi.common import MarketDataTypeEnum as IBMarketDataTypeEnum

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import NautilusConfig


class SymbologyMethod(Enum):
    IB_SIMPLIFIED = "simplified"
    IB_RAW = "raw"


class DockerizedIBGatewayConfig(NautilusConfig, frozen=True):
    """
    Configuration for `DockerizedIBGateway` setup when working with containerized
    installations.

    Parameters
    ----------
    username : str, optional
        The Interactive Brokers account username.
        If ``None`` then will source the `TWS_USERNAME` environment variable.
    password : str, optional
        The Interactive Brokers account password.
        If ``None`` then will source the `TWS_PASSWORD` environment variable.
    trading_mode: str
        ``paper`` or ``live``.
    read_only_api: bool, optional, default True
        If True, no order execution is allowed. Set read_only_api=False to allow executing live orders.
    timeout: int, optional
        The timeout (seconds) for trying to launch IBG docker container when start=True.
    container_image: str, optional
        The reference to the container image used by the IB Gateway.
    vnc_port: int | None, optional, default None
        The VNC port for the container. Set to None to disable VNC access.
        The VNC server provides remote desktop access to the IB Gateway interface.
        Examples: 5900, 5901, 5902, etc.

    """

    username: str | None = None
    password: str | None = None
    trading_mode: Literal["paper", "live"] = "paper"
    read_only_api: bool = True
    timeout: int = 300
    container_image: str = "ghcr.io/gnzsnz/ib-gateway:stable"
    vnc_port: int | None = None

    def __repr__(self):
        masked_username = self._mask_sensitive_info(self.username)

        return (
            f"DockerizedIBGatewayConfig(username={masked_username}, "
            f"password=********, trading_mode='{self.trading_mode}', "
            f"read_only_api={self.read_only_api}, timeout={self.timeout})"
        )

    @staticmethod
    def _mask_sensitive_info(value: str | None) -> str:
        if value is None:
            return "None"

        return value[0] + "*" * (len(value) - 2) + value[-1] if len(value) > 2 else "*" * len(value)


class InteractiveBrokersInstrumentProviderConfig(InstrumentProviderConfig, frozen=True):
    """
    Configuration for instances of `InteractiveBrokersInstrumentProvider`.

    Specify either `load_ids`, `load_contracts`, or both to dictate which instruments the system loads upon start.
    It should be noted that the `InteractiveBrokersInstrumentProviderConfig` isn't limited to the instruments
    initially loaded. Instruments can be dynamically requested and loaded at runtime as needed.

    Parameters
    ----------
    load_all : bool, default False
        Note: Loading all instruments isn't supported by the InteractiveBrokersInstrumentProvider.
        As such, this parameter is not applicable.
    load_ids : FrozenSet[InstrumentId], optional
        A frozenset of `InstrumentId` instances that should be loaded during startup. These represent the specific
        instruments that the provider should initially load.
    load_contracts: FrozenSet[IBContract], optional
        A frozenset of `IBContract` objects that are loaded during the initial startup.These specific contracts
        correspond to the instruments that the  provider preloads. It's important to note that while the `load_ids`
        option can be used for loading individual instruments, using `load_contracts` allows for a more versatile
        loading of several related instruments like Futures and Options that share the same underlying asset.
    symbology_method : SymbologyMethod, optional
        Specifies the symbology format used for identifying financial instruments. The available options are:
        - IB_RAW: Uses the raw symbology format provided by Interactive Brokers. Instrument symbols follow a detailed
        format such as `localSymbol=secType.exchange` (e.g., `EUR.USD=CASH.IDEALPRO`).
        While this format may lack visual clarity, it is robust and supports instruments from any region,
        especially those with non-standard symbology where simplified parsing may fail.
        - IB_SIMPLIFIED: Adopts a simplified symbology format specific to Interactive Brokers which uses Venue acronym.
        Instrument symbols use a cleaner notation, such as `ESZ28.CME` or `EUR/USD.IDEALPRO`.
        This format prioritizes ease of readability and usability and is default.
    build_options_chain: bool (default: None)
        Search for full option chain. Global setting for all applicable instruments.
    build_futures_chain: bool (default: None)
        Search for full futures chain. Global setting for all applicable instruments.
    min_expiry_days: int (default: None)
        Filters the options_chain and futures_chain which are expiring after specified number of days.
        Global setting for all applicable instruments.
    max_expiry_days: int (default: None)
        Filters the options_chain and futures_chain which are expiring before specified number of days.
        Global setting for all applicable instruments.
    convert_exchange_to_mic_venue: bool (default: False)
        Whether to convert IB exchanges to MIC venues when converting an IB contract to an instrument id.
    symbol_to_mic_venue: dict, optional
        A dictionary to override the default MIC venue conversion.
        A key is a symbol prefix (for example ES for all futures and options on it), the value is the MIC venue to use.
    cache_validity_days: int (default: None)
        Default None, will request fresh pull upon starting of TradingNode [only once].
        Setting value will pull the instruments at specified interval, useful when TradingNode runs for many days.
        Example: value set to 1, InstrumentProvider will make fresh pull every day even if TradingNode is not restarted.
    pickle_path: str (default: None)
        If provided valid path, will store the ContractDetails as pickle, and use during cache_validity period.
    filter_sec_types: FrozenSet[str], optional
        A set of IB `secType` values which should be ignored by the provider. Any contract whose
        `secType` matches one of these entries will be skipped with a warning before reconciliation is
        attempted. Use this to opt out from assets that are not yet supported (for example `WAR` or `IOPT`).

    """

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, InteractiveBrokersInstrumentProviderConfig):
            return False

        return (
            self.load_ids == other.load_ids
            and self.load_contracts == other.load_contracts
            and self.min_expiry_days == other.min_expiry_days
            and self.max_expiry_days == other.max_expiry_days
            and self.build_options_chain == other.build_options_chain
            and self.build_futures_chain == other.build_futures_chain
            and self.filter_sec_types == other.filter_sec_types
        )

    def __hash__(self) -> int:
        return hash(
            (
                self.load_ids,
                self.load_contracts,
                self.build_options_chain,
                self.build_futures_chain,
                self.min_expiry_days,
                self.max_expiry_days,
                self.symbology_method,
                self.convert_exchange_to_mic_venue,
                (
                    tuple(sorted(self.symbol_to_mic_venue.items()))
                    if self.symbol_to_mic_venue
                    else None
                ),
                self.cache_validity_days,
                self.pickle_path,
                self.filter_sec_types,
            ),
        )

    symbology_method: SymbologyMethod = SymbologyMethod.IB_SIMPLIFIED
    load_contracts: frozenset[IBContract] | None = None
    build_options_chain: bool | None = None
    build_futures_chain: bool | None = None
    min_expiry_days: int | None = None
    max_expiry_days: int | None = None
    convert_exchange_to_mic_venue: bool = False
    symbol_to_mic_venue: dict = {}

    cache_validity_days: int | None = None
    pickle_path: str | None = None
    filter_sec_types: frozenset[str] = frozenset()


class InteractiveBrokersDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``InteractiveBrokersDataClient`` instances.

    Parameters
    ----------
    ibg_host : str, default "127.0.0.1"
        The hostname or ip address for the IB Gateway (IBG) or Trader Workstation (TWS).
    ibg_port : int, default None
        The port for the gateway server. ("paper"/"live" defaults: IBG 4002/4001; TWS 7497/7496)
    ibg_client_id: int, default 1
        The client_id to be passed into connect call.
    use_regular_trading_hours : bool
        If True, will request data for Regular Trading Hours only.
        Only applies to bar data - will have no effect on trade or tick data feeds.
        Usually used for 'STK' security type. Check with InteractiveBrokers for RTH Info.
    market_data_type : IBMarketDataTypeEnum, default REALTIME
        Set which IBMarketDataTypeEnum to be used by InteractiveBrokersClient.
        Configure `IBMarketDataTypeEnum.DELAYED_FROZEN` to use with account without data subscription.
    ignore_quote_tick_size_updates : bool
        If set to True, the QuoteTick subscription will exclude ticks where only the size has changed but not the price.
        This can help reduce the volume of tick data. When set to False (the default), QuoteTick updates will include
        all updates, including those where only the size has changed.
    dockerized_gateway : DockerizedIBGatewayConfig, Optional
        The client's gateway container configuration.
    connection_timeout : int, default 300
        The timeout (seconds) to wait for the client connection to be established.
    request_timeout : int, default 60
        The timeout (seconds) to wait for a historical data response.

    """

    instrument_provider: InteractiveBrokersInstrumentProviderConfig = (
        InteractiveBrokersInstrumentProviderConfig()
    )

    ibg_host: str = "127.0.0.1"
    ibg_port: int | None = None
    ibg_client_id: int = 1
    use_regular_trading_hours: bool = True
    market_data_type: IBMarketDataTypeEnum = IBMarketDataTypeEnum.REALTIME
    ignore_quote_tick_size_updates: bool = False
    dockerized_gateway: DockerizedIBGatewayConfig | None = None
    connection_timeout: int = 300
    request_timeout: int = 60


class InteractiveBrokersExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``InteractiveBrokersExecClient`` instances.

    Parameters
    ----------
    ibg_host : str, default "127.0.0.1"
        The hostname or ip address for the IB Gateway (IBG) or Trader Workstation (TWS).
    ibg_port : int
        The port for the gateway server. ("paper"/"live" defaults: IBG 4002/4001; TWS 7497/7496)
    ibg_client_id: int, default 1
        The client_id to be passed into connect call.
    account_id : str
        Represents the account_id for the Interactive Brokers to which the TWS/Gateway is logged in.
        It's crucial that the account_id aligns with the account for which the TWS/Gateway is logged in.
        If the account_id is `None`, the system will fallback to use the `TWS_ACCOUNT` from environment variable.
    dockerized_gateway : DockerizedIBGatewayConfig, Optional
        The client's gateway container configuration.
    connection_timeout : int, default 300
        The timeout (seconds) to wait for the client connection to be established.
    fetch_all_open_orders : bool, default False
        If True, uses reqAllOpenOrders to fetch orders from all API clients and TWS GUI.
        If False, uses reqOpenOrders to fetch only orders from current client ID session.
        Note: When using reqAllOpenOrders with client ID 0, it can see orders from all
        sources including TWS GUI, but cannot see orders from other non-zero client IDs.
    track_option_exercise_from_position_update : bool, default False
        If True, subscribes to real-time position updates to track option exercises.

    """

    instrument_provider: InteractiveBrokersInstrumentProviderConfig = (
        InteractiveBrokersInstrumentProviderConfig()
    )
    ibg_host: str = "127.0.0.1"
    ibg_port: int | None = None
    ibg_client_id: int = 1
    account_id: str | None = None
    dockerized_gateway: DockerizedIBGatewayConfig | None = None
    connection_timeout: int = 300
    fetch_all_open_orders: bool = False
    track_option_exercise_from_position_update: bool = False
