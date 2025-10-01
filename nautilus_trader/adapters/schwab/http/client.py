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

from collections.abc import Mapping
from typing import Any

from schwab import auth as schwab_auth
from schwab.utils import Utils

from nautilus_trader.adapters.schwab.config import SchwabClientConfig
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money


class SchwabHttpClientError(RuntimeError):
    """
    Raised when the Schwab client cannot fulfil a request.
    """


class SchwabHttpClient:
    """
    Lightweight async wrapper around ``schwab-py`` REST operations.
    """

    def __init__(
        self,
        config: SchwabClientConfig,
        account_id: str | None = None,
    ) -> None:
        self._config = config
        self._client = self.create_schwab_client()

    async def get_account_numbers(self) -> Mapping[str, Any]:
        response = await self._client.get_account_numbers()
        response.raise_for_status()
        account_hashmap = {v["accountNumber"]: v["hashValue"] for v in response.json()}
        return account_hashmap

    async def get_account(
        self,
        account_hash: str,
        currency: Currency,
    ) -> tuple[list[AccountBalance], list[MarginBalance]]:
        response = await self._client.get_account(account_hash)
        response.raise_for_status()
        account_balance = response.json()
        total = account_balance["securitiesAccount"]["currentBalances"]["liquidationValue"]
        locked = account_balance["securitiesAccount"]["currentBalances"]["maintenanceRequirement"]
        balances = [
            AccountBalance(
                total=Money(total, currency),
                free=Money(total - locked, currency),
                locked=Money(locked, currency),
            ),
        ]
        initial_locked = account_balance["securitiesAccount"]["initialBalances"][
            "maintenanceRequirement"
        ]
        margins = [
            MarginBalance(
                initial=Money(initial_locked, currency),
                maintenance=Money(locked, currency),
            ),
        ]
        return balances, margins

    async def get_orders_for_account(
        self,
        account_hash: str | None,
        from_entered_datetime=None,
        to_entered_datetime=None,
    ) -> list[Mapping[str, Any]]:
        response = await self._client.get_orders_for_account(
            account_hash,
            from_entered_datetime=from_entered_datetime,
            to_entered_datetime=to_entered_datetime,
        )
        response.raise_for_status()
        return response.json()

    # async def get_option_chain(self, symbol: str, **params: Any) -> Mapping[str, Any]:
    #     response = await self._call_async(
    #         ["get_option_chain", "options.get_option_chain"],
    #         symbol,
    #         **params,
    #     )
    #     return self._as_mapping(response)

    async def place_order(
        self,
        account_hash: str | None,
        order_spec: Mapping[str, Any],
    ) -> str:
        response = await self._client.place_order(account_hash, order_spec)
        assert response.status_code == 201, response.raise_for_status()
        order_id = Utils(self._client, account_hash).extract_order_id(response)
        assert order_id is not None
        return str(order_id)

    async def get_order(self, order_id: str, account_hash: str | None) -> Mapping[str, Any]:
        response = await self._client.get_order(order_id, account_hash)
        response.raise_for_status()
        return response.json()

    async def cancel_order(self, order_id: str, account_hash: str) -> Mapping[str, Any]:
        response = await self._client.cancel_order(order_id, account_hash)
        response.raise_for_status()
        return response

    # async def _call_async(self, candidates: Sequence[str], *args: Any, **kwargs: Any) -> Any:
    #     func = self._resolve_callable(candidates)
    #     return await asyncio.to_thread(func, *args, **kwargs)

    # def _resolve_callable(self, candidates: Sequence[str]):
    #     for name in candidates:
    #         target: Any = self._client
    #         for attr in name.split("."):
    #             target = getattr(target, attr, None)
    #             if target is None:
    #                 break
    #         if callable(target):
    #             return target
    #     raise SchwabHttpClientError(
    #         f"Client does not expose any of the expected callables: {
    #             ', '.join(candidates)}",
    #     )

    # def _as_mapping(self, response: Any) -> Mapping[str, Any]:
    #     if isinstance(response, Mapping):
    #         return response
    #     if hasattr(response, "json"):
    #         data = response.json()
    #         if isinstance(data, Mapping):
    #             return data
    #     if hasattr(response, "data") and isinstance(response.data, Mapping):
    #         return response.data
    #     raise SchwabHttpClientError("Response payload is not a mapping")

    # def _ensure_mapping(self, value: Any) -> Mapping[str, Any]:
    #     if isinstance(value, Mapping):
    #         return value
    #     raise SchwabHttpClientError("Expected mapping entry in response list")

    # def _require_account_id(self, value: str | None) -> str:
    #     account = value or self._default_account_id
    #     if not account:
    #         raise SchwabHttpClientError("Account ID is required")
    #     return account

    def create_schwab_client(self) -> Any:
        """
        Instantiate a ``schwab-py`` client using the provided configuration.

        The token expires every week, for now we can only refresh the token manually.
        Every time the token is refreshed manually, the client should be re-
        instantiated.

        """
        http_client = schwab_auth.easy_client(
            api_key=self._config.api_key,
            app_secret=self._config.app_secret,
            callback_url=self._config.callback_url,
            token_path=self._config.token_path,
            asyncio=True,
        )
        return http_client


__all__ = ["SchwabRESTClient", "SchwabRESTClientError", "create_schwab_client"]
