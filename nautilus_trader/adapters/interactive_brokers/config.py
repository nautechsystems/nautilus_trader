# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Literal, Optional

from ibapi.common import MarketDataTypeEnum as IBMarketDataTypeEnum

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config.common import NautilusConfig


class InteractiveBrokersGatewayConfig(NautilusConfig, frozen=True):
    """
    Configuration for `InteractiveBrokersGateway` setup.

    Parameters
    ----------
    username : str, optional
        The Interactive Brokers account username.
        If ``None`` then will source the `TWS_USERNAME`.
    password : str, optional
        The Interactive Brokers account password.
        If ``None`` then will source the `TWS_PASSWORD`.
    trading_mode: str
        paper or live.
    start: bool, optional
        Start or not internal tws docker container.
    read_only_api: bool, optional, default True
        Read only; no execution. Set read_only_api=False to allow executing live orders.
    timeout: int, optional
        The timeout for trying to start gateway

    """

    username: Optional[str] = None
    password: Optional[str] = None
    trading_mode: Literal["paper", "live"] = "paper"
    start: bool = False
    read_only_api: bool = True
    timeout: int = 300


class InteractiveBrokersInstrumentProviderConfig(InstrumentProviderConfig, frozen=True):
    """
    Configuration for ``InteractiveBrokersInstrumentProvider`` instances.

    Parameters
    ----------
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
    cache_validity_days: int (default: None)
        Default None, will request fresh pull upon starting of TradingNode [only once].
        Setting value will pull the instruments at specified interval, useful when TradingNode runs for many days.
        Example: value set to 1, InstrumentProvider will make fresh pull every day even if TradingNode is not restarted.
    pickle_path: str (default: None)
        If provided valid path, will store the ContractDetails as pickle, and use during cache_validity period.

    """

    def __eq__(self, other):
        return (
            self.load_ids == other.load_ids
            and self.load_contracts == other.load_contracts
            and self.min_expiry_days == other.min_expiry_days
            and self.max_expiry_days == other.max_expiry_days
            and self.build_options_chain == other.build_options_chain
            and self.build_futures_chain == other.build_futures_chain
        )

    def __hash__(self):
        return hash(
            (
                self.load_ids,
                self.load_contracts,
                self.build_options_chain,
                self.build_futures_chain,
                self.min_expiry_days,
                self.max_expiry_days,
            ),
        )

    load_contracts: Optional[frozenset[IBContract]] = None
    build_options_chain: Optional[bool] = None
    build_futures_chain: Optional[bool] = None
    min_expiry_days: Optional[int] = None
    max_expiry_days: Optional[int] = None

    cache_validity_days: Optional[int] = None
    pickle_path: Optional[str] = None


class InteractiveBrokersDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``InteractiveBrokersDataClient`` instances.

    Parameters
    ----------
    ibg_host : str, default "127.0.0.1"
        The hostname or ip address for the IB Gateway or TWS.
    ibg_port : int, default for "paper" 4002, or "live" 4001
        The port for the gateway server.
    ibg_client_id: int, default 1
        The client_id to be passed into connect call.
    gateway : InteractiveBrokersGatewayConfig
        The clients gateway container configuration.
    use_regular_trading_hours : bool
        If True will request data for Regular Trading Hours only.
        Mostly applies to 'STK' security type. Check with InteractiveBrokers for RTH Info.
    market_data_type : bool, default REALTIME
        Set which IBMarketDataTypeEnum to be used by InteractiveBrokersClient.
        Configure `IBMarketDataTypeEnum.DELAYED_FROZEN` to use with account without data subscription.

    """

    instrument_provider: InteractiveBrokersInstrumentProviderConfig = (
        InteractiveBrokersInstrumentProviderConfig()
    )

    ibg_host: str = "127.0.0.1"
    ibg_port: Optional[int] = None
    ibg_client_id: int = 1
    gateway: InteractiveBrokersGatewayConfig = InteractiveBrokersGatewayConfig()
    use_regular_trading_hours: bool = True
    market_data_type: IBMarketDataTypeEnum = IBMarketDataTypeEnum.REALTIME


class InteractiveBrokersExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``InteractiveBrokersExecClient`` instances.

    Parameters
    ----------
    ibg_host : str, default "127.0.0.1"
        The hostname or ip address for the IB Gateway or TWS.
    ibg_port : int, default for "paper" 4002, or "live" 4001
        The port for the gateway server.
    ibg_client_id: int, default 1
        The client_id to be passed into connect call.
    ibg_account_id : str
        The Interactive Brokers account id to which TWS/Gateway is logged on.
        If ``None`` then will source the `TWS_ACCOUNT`.

    """

    instrument_provider: InteractiveBrokersInstrumentProviderConfig = (
        InteractiveBrokersInstrumentProviderConfig()
    )
    ibg_host: str = "127.0.0.1"
    ibg_port: Optional[int] = None
    ibg_client_id: int = 1
    gateway: InteractiveBrokersGatewayConfig = InteractiveBrokersGatewayConfig()
    account_id: Optional[str] = None

    # trade_outside_regular_hours (possible to set flag in order)
