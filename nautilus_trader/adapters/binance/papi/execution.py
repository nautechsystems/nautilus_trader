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

import asyncio
from typing import TYPE_CHECKING

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.papi.http.account import BinancePortfolioMarginAccountHttpAPI
from nautilus_trader.adapters.binance.papi.http.execution import BinancePortfolioMarginExecutionHttpAPI
from nautilus_trader.adapters.binance.papi.http.user import BinancePortfolioMarginUserDataHttpAPI
from nautilus_trader.adapters.binance.papi.providers import BinancePortfolioMarginInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.correctness import PyCondition


class BinancePortfolioMarginExecutionClient(BinanceCommonExecutionClient):
    """
    Provides an execution client for the Binance Portfolio Margin account.

    Portfolio Margin is a unified account that allows trading across spot, margin,
    and futures (both USDT-M and COIN-M) markets with cross-collateralization.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BinanceHttpClient
        The Binance HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BinanceFuturesInstrumentProvider | BinancePortfolioMarginInstrumentProvider
        The instrument provider.
    base_url_ws : str
        The base URL for the WebSocket client.
    config : BinanceExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BinanceFuturesInstrumentProvider | BinancePortfolioMarginInstrumentProvider,
        base_url_ws: str,
        config: BinanceExecClientConfig,
        name: str | None = None,
    ) -> None:
        account_type = BinanceAccountType.PORTFOLIO_MARGIN

        PyCondition.is_true(
            account_type.is_portfolio_margin,
            "account_type was not PORTFOLIO_MARGIN",
        )

        # Portfolio Margin HTTP APIs
        self._papi_http_account = BinancePortfolioMarginAccountHttpAPI(client, clock, account_type)
        self._papi_http_execution = BinancePortfolioMarginExecutionHttpAPI(client, clock, account_type)
        self._papi_http_user = BinancePortfolioMarginUserDataHttpAPI(client, account_type)

        # Use futures enum parser as portfolio margin supports futures contracts
        self._futures_enum_parser = BinanceFuturesEnumParser()

        # Instantiate common base class
        super().__init__(
            loop=loop,
            client=client,
            account=self._papi_http_account,
            market=None,  # Portfolio margin uses multiple markets
            user=self._papi_http_user,
            enum_parser=self._futures_enum_parser,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            base_url_ws=base_url_ws,
            config=config,
            account_type=account_type,
            name=name,
        )

    # Additional Portfolio Margin specific methods can be added here
    async def _update_portfolio_margin_account(self) -> None:
        """
        Update Portfolio Margin account information.
        """
        self._log.debug("Updating Portfolio Margin account information")
        
        try:
            # Query account balances
            balances = await self._papi_http_account.query_balance()
            
            # Query UM position risk
            um_positions = await self._papi_http_account.query_um_position_risk()
            
            # Query CM position risk
            cm_positions = await self._papi_http_account.query_cm_position_risk()

            # Process account updates
            # TODO: Convert to Nautilus account events and send to cache

            self._log.debug(f"Updated Portfolio Margin account: {len(balances)} balances, "
                          f"{len(um_positions)} UM positions, {len(cm_positions)} CM positions")
                          
        except Exception as e:
            self._log.error(f"Failed to update Portfolio Margin account: {e}")

    async def _submit_um_order(self, **kwargs):
        """
        Submit a USD-M futures order via Portfolio Margin API.
        """
        return await self._papi_http_execution.new_um_order(**kwargs)

    async def _submit_cm_order(self, **kwargs):
        """
        Submit a COIN-M futures order via Portfolio Margin API.
        """
        return await self._papi_http_execution.new_cm_order(**kwargs)

    async def _submit_margin_order(self, **kwargs):
        """
        Submit a margin order via Portfolio Margin API.
        """
        return await self._papi_http_execution.new_margin_order(**kwargs)

    async def _modify_um_order(self, **kwargs):
        """
        Modify a USD-M futures order via Portfolio Margin API.
        """
        return await self._papi_http_execution.modify_um_order(**kwargs)

    async def _modify_cm_order(self, **kwargs):
        """
        Modify a COIN-M futures order via Portfolio Margin API.
        """
        return await self._papi_http_execution.modify_cm_order(**kwargs)

    async def _cancel_um_order(self, **kwargs):
        """
        Cancel a USD-M futures order via Portfolio Margin API.
        """
        return await self._papi_http_execution.cancel_um_order(**kwargs)

    async def _cancel_cm_order(self, **kwargs):
        """
        Cancel a COIN-M futures order via Portfolio Margin API.
        """
        return await self._papi_http_execution.cancel_cm_order(**kwargs)

    async def _cancel_margin_order(self, **kwargs):
        """
        Cancel a margin order via Portfolio Margin API.
        """
        return await self._papi_http_execution.cancel_margin_order(**kwargs)
