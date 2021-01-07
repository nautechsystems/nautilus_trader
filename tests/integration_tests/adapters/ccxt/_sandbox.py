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


# Requirements:
# - An internet connection


def request_instruments():
    client = ccxt.binance({
        "apiKey": "",
        "secret": "",
        "timeout": 10000,         # Hard coded for now
        "enableRateLimit": True,  # Hard coded for now
    })

    client.load_markets()
    res = client.markets

    with open('res_instruments.json', 'w') as json_file:
        json.dump(res, json_file)


def request_bars():
    client = ccxt.binance({
        "apiKey": "",
        "secret": "",
        "timeout": 10000,         # Hard coded for now
        "enableRateLimit": True,  # Hard coded for now
    })

    client.load_markets()

    bars = client.fetch_ohlcv(
        "ETH/USDT",
        "1m",
        limit=101,  # Simulate a user request of 100 accounting for partial bar
    )
    with open('res_bars.json', 'w') as json_file:
        json.dump(bars, json_file)


def request_trades():
    client = ccxt.binance({
        "apiKey": "",
        "secret": "",
        "timeout": 10000,         # Hard coded for now
        "enableRateLimit": True,  # Hard coded for now
    })

    client.load_markets()

    trades = client.fetch_trades(
        "ETH/USDT",
        limit=100,
    )
    with open('res_trades.json', 'w') as json_file:
        json.dump(trades, json_file)


if __name__ == "__main__":
    # Enter function to run
    pass
