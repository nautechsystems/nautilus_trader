# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import os
from typing import Literal, Optional

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class InteractiveBrokersDataClientConfig(LiveDataClientConfig):
    """
    Configuration for ``InteractiveBrokersDataClient`` instances.

    Parameters
    ----------
    username : str, optional
        The Interactive Brokers account username.
        If ``None`` then will source the `TWS_USERNAME`
    password : str, optional
        The Interactive Brokers account password.
        If ``None`` then will source the `TWS_PASSWORD`
    trading_mode: str
        paper or live
    account_id : str, optional
        The account_id to use for nautilus
    gateway_host : str, optional
        The hostname for the gateway server
    gateway_port : int, optional
        The port for the gateway server
    client_id: int, optional
        The client_id to be passed into connect call
    start_gateway: bool, optional
        Start or not internal tws docker container
    """

    username: Optional[str] = None
    password: Optional[str] = None
    trading_mode: Literal["paper", "live"] = "paper"
    account_id: str = "001"
    gateway_host: str = "127.0.0.1"
    gateway_port: Optional[int] = None
    client_id: int = 1
    start_gateway: bool = True

    def __init__(self, **kwargs):
        kwargs["username"] = (
            kwargs.get("username", os.environ["TWS_USERNAME"])
            if kwargs.get("start_gateway")
            else ""
        )
        kwargs["password"] = (
            kwargs.get("password", os.environ["TWS_PASSWORD"])
            if kwargs.get("start_gateway")
            else ""
        )
        super().__init__(**kwargs)


class InteractiveBrokersExecClientConfig(LiveExecClientConfig):
    """
    Configuration for ``InteractiveBrokersExecClient`` instances.

    Parameters
    ----------
    username : str, optional
        The Interactive Brokers account username.
        If ``None`` then will source the `TWS_USERNAME`
    password : str, optional
        The Interactive Brokers account password.
        If ``None`` then will source the `TWS_PASSWORD`
    trading_mode: str
        paper or live
    account_id : str, optional
        The account_id to use for nautilus
    gateway_host : str, optional
        The hostname for the gateway server
    gateway_port : int, optional
        The port for the gateway server
     client_id: int, optional
        The client_id to be passed into connect call
    start_gateway: bool, optional
        Start or not internal tws docker container
    """

    username: Optional[str] = None
    password: Optional[str] = None
    trading_mode: Literal["paper", "live"] = "paper"
    account_id: str = "001"
    gateway_host: str = "127.0.0.1"
    gateway_port: Optional[int] = None
    client_id: int = 1
    start_gateway: bool = True

    def __init__(self, **kwargs):
        kwargs["username"] = (
            kwargs.get("username", os.environ["TWS_USERNAME"])
            if kwargs.get("start_gateway")
            else ""
        )
        kwargs["password"] = (
            kwargs.get("password", os.environ["TWS_PASSWORD"])
            if kwargs.get("start_gateway")
            else ""
        )
        super().__init__(**kwargs)
