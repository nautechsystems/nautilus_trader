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
from typing import Optional, Tuple

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class BetfairDataClientConfig(LiveDataClientConfig):
    """
    Configuration for ``BetfairDataClient`` instances.

    Parameters
    ----------
    username : str, optional
        The Betfair account username.
        If ``None`` then will source the `TWS_USERNAME`
    password : str, optional
        The Betfair account password.
        If ``None`` then will source the `TWS_PASSWORD`
    account_id : str, optional
        The account_id to use for nautilus
    gateway_host : str, optional
        The hostname for the gateway server
    gateway_port : int, optional
        The port for the gateway server
    """

    username: Optional[str] = None
    password: Optional[str] = None
    app_key: Optional[str] = None
    cert_dir: Optional[str] = None
    market_filter: Optional[Tuple] = None

    def __init__(self, **kwargs):
        kwargs["username"] = kwargs.get("username", os.environ.get("BETFAIR_USERNAME"))
        kwargs["password"] = kwargs.get("password", os.environ.get("BETFAIR_PASSWORD"))
        kwargs["app_key"] = kwargs.get("app_key", os.environ.get("BETFAIR_APP_KEY"))
        kwargs["cert_dir"] = kwargs.get("cert_dir", os.environ.get("BETFAIR_CERT_DIR"))
        super().__init__(**kwargs)


class BetfairExecClientConfig(LiveExecClientConfig):
    """
    Configuration for ``BetfairExecClient`` instances.

    Parameters
    ----------
    username : str, optional
        The Betfair account username.
        If ``None`` then will source the `BETFAIR_USERNAME`
    password : str, optional
        The Betfair account password.
        If ``None`` then will source the `BETFAIR_PASSWORD`
    app_key : str, optional
        The Betfair app_key
        If ``None`` then will source the `BETFAIR_APP_KEY`
    cert_dir : str, optional
        The directory containing certificates for Betfair
        If ``None`` then will source the `BETFAIR_CERT_DIR`
    """

    base_currency: str
    username: Optional[str] = None
    password: Optional[str] = None
    app_key: Optional[str] = None
    cert_dir: Optional[str] = None
    market_filter: Optional[Tuple] = None

    def __init__(self, **kwargs):
        kwargs["username"] = kwargs.get("username", os.environ.get("BETFAIR_USERNAME"))
        kwargs["password"] = kwargs.get("password", os.environ.get("BETFAIR_PASSWORD"))
        kwargs["app_key"] = kwargs.get("app_key", os.environ.get("BETFAIR_APP_KEY"))
        kwargs["cert_dir"] = kwargs.get("cert_dir", os.environ.get("BETFAIR_CERT_DIR"))
        super().__init__(**kwargs)
