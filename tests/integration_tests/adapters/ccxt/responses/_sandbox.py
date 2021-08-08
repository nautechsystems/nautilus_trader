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

import json

import ccxt


def download_instruments():
    exchange = "bitmex"
    client = getattr(ccxt, exchange.lower())()
    client.load_markets()
    print(client.name)

    # precisions = [{k: v['precision']} for k, v in ccxt.markets.items()]
    # print(json.dumps(precisions, sort_keys=True, indent=4))

    instruments = {k: v for k, v in client.markets.items()}
    print(json.dumps(instruments["BTC/USD"], sort_keys=True, indent=4))

    # currencies = {k: v for k, v in ccxt.currencies.items()}
    # print(json.dumps(currencies, sort_keys=True, indent=4))


def request_instruments():
    client = ccxt.binance(
        {
            "apiKey": "",
            "secret": "",
            "timeout": 10000,  # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
        }
    )

    client.load_markets()
    res = client.markets

    with open("markets.json", "w") as json_file:
        json.dump(res, json_file)


def request_currencies():
    client = ccxt.binance(
        {
            "apiKey": "",
            "secret": "",
            "timeout": 10000,  # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
        }
    )

    client.load_markets()
    currencies = client.currencies

    with open("currencies.json", "w") as json_file:
        json.dump(currencies, json_file)


def request_order_book():
    client = ccxt.binance(
        {
            "apiKey": "",
            "secret": "",
            "timeout": 10000,  # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
        }
    )

    client.load_markets()

    order_book = client.fetch_order_book(
        "ETH/USDT",
    )
    with open("fetch_order_book.json", "w") as json_file:
        json.dump(order_book, json_file)


def request_bars():
    client = ccxt.binance(
        {
            "apiKey": "",
            "secret": "",
            "timeout": 10000,  # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
        }
    )

    client.load_markets()

    bars = client.fetch_ohlcv(
        "ETH/USDT",
        "1m",
        limit=101,  # Simulate a user request of 100 accounting for partial bar
    )
    with open("fetch_ohlcv.json", "w") as json_file:
        json.dump(bars, json_file)


def request_trades():
    client = ccxt.binance(
        {
            "apiKey": "",
            "secret": "",
            "timeout": 10000,  # Hard coded for now
            "enableRateLimit": True,  # Hard coded for now
        }
    )

    client.load_markets()

    trades = client.fetch_trades(
        "ETH/USDT",
        limit=100,
    )
    with open("fetch_trades.json", "w") as json_file:
        json.dump(trades, json_file)


if __name__ == "__main__":
    download_instruments()
    pass
