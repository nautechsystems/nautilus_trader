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

from py_clob_client.exceptions import PolyApiException


class PolymarketError(Exception):
    """
    Represents a Polymarket specific error.
    """

    def __init__(
        self,
        code: int | None,
        message: str | None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message

    def __repr__(self) -> str:
        return f"{type(self).__name__}(code={self.code}, message='{self.message}')"


def should_retry(error: BaseException) -> bool:
    """
    Determine if a retry should be attempted based on the error code.

    Parameters
    ----------
    error : BaseException
        The error to check.

    Returns
    -------
    bool
        True if should retry, otherwise False.

    """
    if isinstance(error, PolyApiException):
        # https://github.com/Polymarket/py-clob-client/blob/main/py_clob_client/exceptions.py
        status_code = getattr(error, "status_code", None)

        # Retry on rate limits and server errors
        if status_code == 429 or (status_code is not None and status_code >= 500):
            return True

    return False
