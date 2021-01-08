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
from cryptofeed import FeedHandler
from cryptofeed.callback import TickerCallback
from cryptofeed.callback import TradeCallback
from cryptofeed.defines import TICKER
from cryptofeed.defines import TRADES
from cryptofeed.exchanges import Binance


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
        "BTC/USDT",
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
        "BTC/USDT",
        limit=100,
    )
    with open('res_trades.json', 'w') as json_file:
        json.dump(trades, json_file)


async def ticker(feed, pair, bid, ask, timestamp, receipt_timestamp):
    print(f'Timestamp: {timestamp} Feed: {feed} Pair: {pair} Bid: {bid} Ask: {ask}')


async def trade(feed, pair, order_id, timestamp, side, amount, price, receipt_timestamp):
    print(f"Timestamp: {timestamp} Feed: {feed} Pair: {pair} ID: {order_id} Side: {side} Amount: {amount} Price: {price}")


def streaming_ticker():
    f = FeedHandler()
    f.add_feed(Binance(pairs=['BTC-USDT'], channels=[TRADES, TICKER], callbacks={TICKER: TickerCallback(ticker), TRADES: TradeCallback(trade)}))

    f.run()


if __name__ == "__main__":
    # Enter function to run
    pass
