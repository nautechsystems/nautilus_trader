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

#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  https://github.com/binance/binance-connector-python/blob/master/binance/error.py
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------


class BinanceError(Exception):
    """
    The base class for all `Binance` specific errors.
    """


class BinanceServerError(BinanceError):
    """
    Represents a `Binance` specific 500 series HTTP error.
    """

    def __init__(self, status_code, message):
        self.status_code = status_code
        self.message = message


class BinanceClientError(BinanceError):
    """
    Represents a `Binance` specific 400 series HTTP error.
    """

    def __init__(self, status_code, error_code, error_message, header):
        self.status_code = status_code
        self.error_code = error_code
        self.error_message = error_message
        self.header = header
