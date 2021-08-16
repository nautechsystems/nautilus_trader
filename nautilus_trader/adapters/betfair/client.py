import asyncio
import pathlib
import ssl
from typing import Dict, List, Optional, Union

import orjson

from nautilus_trader.common.logging import Logger
from nautilus_trader.network.http_client import HTTPClient
from nautilus_trader.network.http_client import ResponseException


class BetfairClient(HTTPClient):
    IDENTITY_URL = "https://identitysso-cert.betfair.com/api/"
    BASE_URL = "https://api.betfair.com/exchange/"
    NAVIGATION_URL = BASE_URL + "betting/rest/v1/en/navigation/menu.json"
    ACCOUNT_URL = BASE_URL + "account/json-rpc/v1"
    BETTING_URL = BASE_URL + "betting/json-rpc/v1"
    JSON_RPC_DEFAULTS = {"jsonrpc": "2.0", "id": 1}

    def __init__(
        self,
        username: str,
        password: str,
        app_key: str,
        cert_dir: str,
        loop: asyncio.AbstractEventLoop,
        logger: Logger,
        ssl=None,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
            ssl=ssl or self.ssl_context(cert_dir=cert_dir),
            connector_kwargs={"enable_cleanup_closed": True, "force_close": True},
        )
        self.username = username
        self.password = password
        self.app_key = app_key
        self.session_token: Optional[str] = None

    @property
    def headers(self):
        auth = {"X-Authentication": self.session_token} if self.session_token else {}
        return {
            "Accept-Encoding": "gzip, deflate",
            "Connection": "keep-alive",
            "content-type": "application/json",
            "X-Application": self.app_key,
            **auth,
        }

    @staticmethod
    def ssl_context(cert_dir):
        certs = [p for p in pathlib.Path(cert_dir).glob("*") if p.suffix in (".crt", ".key")]
        context = ssl.create_default_context()
        context.load_cert_chain(*certs)
        return context

    # For testing purposes, can't mock HTTPClient.request due to cython
    async def request(self, method, url, **kwargs) -> Union[bytes, List, Dict]:
        return await super().request(method=method, url=url, **kwargs)

    async def rpc_post(
        self, url, method, params: Optional[Dict] = None, data: Optional[Dict] = None
    ) -> Dict:
        data = {**self.JSON_RPC_DEFAULTS, "method": method, **(data or {}), "params": params or {}}
        try:
            resp = await self.request(method="POST", url=url, headers=self.headers, json=data)
            data = orjson.loads(resp.data)  # type: ignore
            if "error" in data:
                raise BetfairAPIError(code=data["error"]["code"], message=data["error"]["message"])
            if isinstance(data, dict):
                return data
            else:
                raise TypeError("Unexpected type:" + str(resp))
        except BetfairError as e:
            self._log.error(str(e))
            raise e

        except ResponseException as e:
            self._log.error(
                f"Err on {method} status={e.resp.status}, message={e.client_response_error.message}"
            )
            raise e

    async def connect(self):
        await super().connect()
        await self.login()

    async def disconnect(self):
        self.session_token = None
        await super().disconnect()

    async def login(self):
        self._log.debug("BetfairClient login")
        if self.session_token is not None:
            self._log.warning("Already logged in, returning")
            return
        url = self.IDENTITY_URL + "certlogin"
        data = {"username": self.username, "password": self.password}
        headers = {
            **{k: v for k, v in self.headers.items() if k not in ("content-type",)},
            **{"Content-Type": "application/x-www-form-urlencoded"},
        }
        resp = await self.post(url=url, data=data, headers=headers)
        data = orjson.loads(resp.data)
        if data["loginStatus"] == "SUCCESS":
            self.session_token = data["sessionToken"]

    async def list_navigation(self):
        """
        List the tree (navigation) of all betfair markets
        """
        resp = await self.get(url=self.NAVIGATION_URL, headers=self.headers)
        return orjson.loads(resp.data)

    async def list_market_catalogue(
        self,
        market_filter: dict,
        market_projection: List[str] = None,
        sort: str = None,
        max_results: int = 1000,
        locale: str = None,
    ):
        """
        Return specific data about markets
        """
        assert 0 < max_results <= 1000

        params: Dict = {
            "filter": parse_market_filter(market_filter),
            "maxResults": max_results,
        }
        if market_projection is not None:
            assert all([m in MARKET_PROJECTIONS for m in market_projection])
            params["marketProjection"] = market_projection
        if sort is not None:
            assert sort in MARKET_SORT
            params["sort"] = sort
        if locale is not None:
            params["locale"] = locale
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/listMarketCatalogue", params=params
        )
        if isinstance(resp, dict):
            return resp["result"]

    async def get_account_details(self):
        resp = await self.rpc_post(
            url=self.ACCOUNT_URL, method="AccountAPING/v1.0/getAccountDetails"
        )
        return resp["result"]

    async def get_account_funds(self, wallet: Optional[str] = None):
        params = None
        if wallet:
            params = {"wallet": wallet}
        resp = await self.rpc_post(
            url=self.ACCOUNT_URL, method="AccountAPING/v1.0/getAccountFunds", params=params
        )
        return resp["result"]

    async def place_orders(
        self,
        market_id: str,
        instructions: list,
        customer_ref: str = None,
        market_version: Optional[dict] = None,
        customer_strategy_ref: str = None,
    ):
        """
        Place a new order
        """
        params = {
            "marketId": market_id,
            "instructions": instructions,
        }
        if customer_ref is not None:
            params["customerRef"] = customer_ref
        if market_version is not None:
            params["marketVersion"] = market_version  # type: ignore
        if customer_strategy_ref is not None:
            params["customerStrategyRef"] = customer_strategy_ref
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/placeOrders", params=params
        )
        return resp

    async def replace_orders(
        self,
        market_id: str = None,
        instructions: list = None,
        customer_ref: str = None,
        market_version: Optional[dict] = None,
    ):
        params = {
            "marketId": market_id,
            "instructions": instructions,
        }
        if customer_ref is not None:
            params["customerRef"] = customer_ref
        if market_version is not None:
            params["marketVersion"] = market_version  # type: ignore
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/replaceOrders", params=params
        )
        return resp

    async def cancel_orders(
        self,
        market_id: str = None,
        instructions: list = None,
        customer_ref: str = None,
    ):
        params = {
            "marketId": market_id,
            "instructions": instructions,
        }
        if customer_ref is not None:
            params["customerRef"] = customer_ref
        resp = await self.rpc_post(
            url=self.BETTING_URL, method="SportsAPING/v1.0/cancelOrders", params=params
        )
        return resp


MARKET_PROJECTIONS = [
    "COMPETITION",
    "EVENT",
    "EVENT_TYPE",
    "MARKET_START_TIME",
    "MARKET_DESCRIPTION",
    "RUNNER_DESCRIPTION",
    "RUNNER_METADATA",
]

MARKET_SORT = [
    "MINIMUM_TRADED",
    "MAXIMUM_TRADED",
    "MINIMUM_AVAILABLE",
    "MAXIMUM_AVAILABLE",
    "FIRST_TO_START",
    "LAST_TO_START",
]

MARKET_BETTING_TYPE = [
    "ODDS",
    "LINE",
    "RANGE",
    "ASIAN_HANDICAP_DOUBLE_LINE",
    "ASIAN_HANDICAP_SINGLE_LINE",
    "FIXED_ODDS",
]

ORDER_STATUS = [
    "PENDING",
    "EXECUTION_COMPLETE",
    "EXECUTABLE",
    "EXPIRED",
]


def parse_market_filter(market_filter):
    string_keys = ("textQuery",)
    bool_keys = ("bspOnly", "turnInPlayEnabled", "inPlayOnly")
    list_string_keys = (
        "exchangeIds",
        "eventTypeIds",
        "eventIds",
        "competitionIds",
        "marketIds",
        "venues",
        "marketBettingTypes",
        "marketCountries",
        "marketTypeCodes",
        "withOrders",
        "raceTypes",
    )
    for key in string_keys:
        if key not in market_filter:
            continue
        # Condition.type(market_filter[key], str, key)
        assert isinstance(market_filter[key], str), f"{key} should be type `str` not {type(key)}"
    for key in bool_keys:
        if key not in market_filter:
            continue
        # Condition.type(market_filter[key], bool, key)
        assert isinstance(market_filter[key], bool), f"{key} should be type `bool` not {type(key)}"
    for key in list_string_keys:
        if key not in market_filter:
            continue
        # Condition.list_type(market_filter[key], str, key)
        assert isinstance(market_filter[key], list), f"{key} should be type `list` not {type(key)}"
        for v in market_filter[key]:
            assert isinstance(v, str), f"{v} should be type `str` not {type(v)}"
    return market_filter


class BetfairError(Exception):
    pass


class BetfairAPIError(BetfairError):
    def __init__(self, code: str, message: str):
        super().__init__()
        self.code = code
        self.message = message

    def __str__(self):
        return f"BetfairAPIError(code={self.code}, message={self.message})"
