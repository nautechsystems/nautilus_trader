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
"""
Define a dYdX exception.
"""

from typing import Any

from grpc.aio._call import AioRpcError
from msgspec import DecodeError

from nautilus_trader.adapters.dydx.common.constants import DYDX_RETRY_ERRORS_GRPC
from nautilus_trader.adapters.dydx.grpc.errors import DYDXGRPCError
from nautilus_trader.core.nautilus_pyo3 import HttpError
from nautilus_trader.core.nautilus_pyo3 import HttpTimeoutError
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError


class DYDXError(Exception):
    """
    Define the class for all dYdX specific errors.
    """

    def __init__(self, status: int, message: str, headers: dict[str, Any]) -> None:
        """
        Define the base class for all dYdX specific errors.
        """
        super().__init__(message)
        self.status = status
        self.message = message
        self.headers = headers


def should_retry(error: BaseException) -> bool:
    """
    Determine if a retry should be attempted.

    Parameters
    ----------
    error : BaseException
        The error to check.

    Returns
    -------
    bool
        True if should retry, otherwise False.

    """
    if isinstance(error, DYDXGRPCError):
        return error.code in DYDX_RETRY_ERRORS_GRPC

    if isinstance(
        error,
        AioRpcError | DYDXError | HttpError | HttpTimeoutError | WebSocketClientError | DecodeError,
    ):
        return True

    return False
