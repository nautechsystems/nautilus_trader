# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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


class BybitError(Exception):
    """
    The base class for all `Bybit` specific errors.
    """

    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


class BybitKeyExpiredError(BybitError):
    code = 33004
    message = "Your api key has expired."

    def __init__(self):
        super().__init__(self.code, self.message)


def raise_bybit_error(code):
    if code == BybitKeyExpiredError.code:
        raise BybitKeyExpiredError
    else:
        raise BybitError(code, "Unknown bybit error")
