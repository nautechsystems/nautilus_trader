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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  https://github.com/binance/binance-connector-python/blob/master/binance/api.py
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

import asyncio
import hashlib
import hmac
import json
import logging
from json import JSONDecodeError
from typing import Dict
from urllib.parse import urlencode

from nautilus_trader.adapters.binance.client.error import BinanceClientError
from nautilus_trader.adapters.binance.client.error import BinanceServerError
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.network.http import HTTPClient


class BinanceHTTPClient(HTTPClient):
    """
    Provides a `Binance` asynchronous HTTP client
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        clock: LiveClock,
        logger: Logger,
        key=None,
        secret=None,
        base_url=None,
        timeout=None,
        proxies=None,
        show_limit_usage=False,
        show_header=False,
    ):
        super().__init__(
            loop=loop,
            logger=logger,
        )
        self.clock = clock
        self.key = key
        self.secret = secret
        self.timeout = timeout
        self.show_limit_usage = False
        self.show_header = False
        self.proxies = None
        self.headers: Dict[str, str] = {}
        # self.session.headers.update(
        #     {
        #         "Content-Type": "application/json;charset=utf-8",
        #         "User-Agent": "binance-connector/" + __version__,
        #         "X-MBX-APIKEY": key,
        #     }
        # )

        if base_url:
            self.base_url = base_url

        if show_limit_usage is True:
            self.show_limit_usage = True

        if show_header is True:
            self.show_header = True

        if type(proxies) is dict:
            self.proxies = proxies

        return

    def query(self, url_path, payload=None):
        return self.send_request("GET", url_path, payload=payload)

    def limit_request(self, http_method, url_path, payload=None):
        """
        Limit request is for those endpoints require API key in the header.
        """
        # check_required_parameter(self.key, "apiKey")
        return self.send_request(http_method, url_path, payload=payload)

    def sign_request(self, http_method, url_path, payload=None):
        if payload is None:
            payload = {}
        payload["timestamp"] = self.clock.timestamp() * 1000
        query_string = self._prepare_params(payload)
        signature = self._get_sign(query_string)
        payload["signature"] = signature
        return self.send_request(http_method, url_path, payload)

    def limited_encoded_sign_request(self, http_method, url_path, payload=None):
        """
        Limit encoded sign request.

        This is used for some endpoints has special symbol in the url.
        In some endpoints these symbols should not encoded
        - @
        - [
        - ]
        so we have to append those parameters in the url.
        """
        if payload is None:
            payload = {}
        payload["timestamp"] = self.clock.timestamp() * 1000
        query_string = self._prepare_params(payload)
        signature = self._get_sign(query_string)
        url_path = url_path + "?" + query_string + "&signature=" + signature
        return self.send_request(http_method, url_path)

    def send_request(self, http_method, url_path, payload=None):
        if payload is None:
            payload = {}
        url = self.base_url + url_path
        logging.debug("url: " + url)
        params = {
            "url": url,
            "params": self._prepare_params(payload),
            "timeout": self.timeout,
            "proxies": self.proxies,
        }
        response = self._dispatch_request(http_method)(**params)
        logging.debug("raw response from server:" + response.text)
        self._handle_exception(response)

        try:
            data = response.json()
        except ValueError:
            data = response.text
        result = {}

        if self.show_limit_usage:
            limit_usage = {}
            for key in response.headers.keys():
                key = key.lower()
                if (
                    key.startswith("x-mbx-used-weight")
                    or key.startswith("x-mbx-order-count")
                    or key.startswith("x-sapi-used")
                ):
                    limit_usage[key] = response.headers[key]
            result["limit_usage"] = limit_usage

        if self.show_header:
            result["header"] = response.headers

        if len(result) != 0:
            result["data"] = data
            return result

        return data

    def _prepare_params(self, params):
        return urlencode(clean_none_value(params), True).replace("%40", "@")

    def _get_sign(self, data):
        m = hmac.new(self.secret.encode("utf-8"), data.encode("utf-8"), hashlib.sha256)
        return m.hexdigest()

    def _handle_exception(self, response):
        status_code = response.status_code
        if status_code < 400:
            return
        if 400 <= status_code < 500:
            try:
                err = json.loads(response.text)
            except JSONDecodeError:
                raise BinanceClientError(status_code, None, response.text, response.headers)
            raise BinanceClientError(status_code, err["code"], err["msg"], response.headers)
        raise BinanceServerError(status_code, response.text)


def clean_none_value(d) -> dict:
    out = {}
    for k in d.keys():
        if d[k] is not None:
            out[k] = d[k]
    return out
