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
    # Arrange
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


# def request_bars():
#     # Arrange
#     oanda_api_token = os.getenv("OANDA_API_TOKEN")
#
#     client = oandapyV20.API(access_token=oanda_api_token)
#
#     # BarType = AUD/USD.OANDA-1-MINUTE-MID
#     params = {
#         "dailyAlignment": 0,  # UTC
#         "count": 100,
#         "price": "M",
#         "granularity": "M1",
#     }
#     req = InstrumentsCandles(instrument="AUD_USD", params=params)
#     res = client.request(req)
#
#     with open('bars.json', 'w') as json_file:
#         json.dump(res, json_file)


if __name__ == "__main__":
    request_instruments()
