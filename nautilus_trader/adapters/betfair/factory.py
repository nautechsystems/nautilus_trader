# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import os
from functools import lru_cache

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecutionClientFactory
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus


CLIENTS = {}


@lru_cache(1)
def get_betfair_client(
    username: str,
    password: str,
    app_key: str,
    cert_dir: str,
    loop: asyncio.AbstractEventLoop,
    logger: Logger,
) -> BetfairClient:
    global CLIENTS
    key = (username, password, app_key, cert_dir)
    if key not in CLIENTS:
        LoggerAdapter("BetfairFactory", logger).warning("Creating new instance of BetfairClient")
        client = BetfairClient(
            username=username,
            password=password,
            app_key=app_key,
            cert_dir=cert_dir,
            loop=loop,
            logger=logger,
        )
        CLIENTS[key] = client
    return CLIENTS[key]


@lru_cache(1)
def get_instrument_provider(client: BetfairClient, logger: Logger, market_filter: tuple):
    LoggerAdapter("BetfairFactory", logger).warning(
        "Creating new instance of BetfairInstrumentProvider"
    )
    # LoggerAdapter("BetfairFactory", logger).warning(f"kwargs: {locals()}")
    return BetfairInstrumentProvider(
        client=client, logger=logger, market_filter=dict(market_filter)
    )


class BetfairLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Betfair` live data client factory.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls=None,
    ):
        """
        Create a new Betfair client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict
            The configuration dictionary.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.
        client_cls : class, optional
            The class to call to return a new internal client.

        Returns
        -------
        BetfairDataClient

        """
        market_filter = config.get("market_filter", {})

        # Create client
        client = get_betfair_client(
            username=os.getenv(config.get("username", ""), ""),
            password=os.getenv(config.get("password", ""), ""),
            app_key=os.getenv(config.get("app_key", ""), ""),
            cert_dir=os.getenv(config.get("cert_dir", ""), ""),
            loop=loop,
            logger=logger,
        )
        provider = get_instrument_provider(
            client=client, logger=logger, market_filter=tuple(market_filter.items())
        )

        data_client = BetfairDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            market_filter=market_filter,
            instrument_provider=provider,
        )
        return data_client


class BetfairLiveExecutionClientFactory(LiveExecutionClientFactory):
    """
    Provides data and execution clients for Betfair.
    """

    @staticmethod
    def create(
        loop: asyncio.AbstractEventLoop,
        name: str,
        config,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: LiveLogger,
        client_cls=None,
    ):
        """
        Create new Betfair clients.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The client name.
        config : dict[str, object]
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : LiveLogger
            The logger for the client.
        client_cls : class, optional
            The internal client constructor. This allows external library and
            testing dependency injection.

        Returns
        -------
        BetfairExecClient

        """
        market_filter = config.get("market_filter", {})

        client = get_betfair_client(
            username=os.getenv(config.get("username", ""), ""),
            password=os.getenv(config.get("password", ""), ""),
            app_key=os.getenv(config.get("app_key", ""), ""),
            cert_dir=os.getenv(config.get("cert_dir", ""), ""),
            loop=loop,
            logger=logger,
        )
        provider = get_instrument_provider(
            client=client, logger=logger, market_filter=tuple(market_filter.items())
        )

        # Get account ID env variable or set default
        account_id_env_var = os.getenv(config.get("account_id", ""), "001")

        # Set account ID
        account_id = AccountId(BETFAIR_VENUE.value, account_id_env_var)

        # Create client
        exec_client = BetfairExecutionClient(
            loop=loop,
            client=client,
            account_id=account_id,
            base_currency=Currency.from_str(config.get("base_currency")),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            market_filter=market_filter,
            instrument_provider=provider,
        )
        return exec_client
