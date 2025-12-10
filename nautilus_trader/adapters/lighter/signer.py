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

from __future__ import annotations

import ctypes
import platform
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


_DEFAULT_SIGNER_ROOT = Path("/tmp/lighter-python/lighter/signers")


class SignerError(Exception):
    """Raised when the signer returns an error string."""


def _resolve_signer_path(root: Path | None = None) -> Path:
    root = root or _DEFAULT_SIGNER_ROOT
    system = platform.system()
    machine = platform.machine().lower()

    if system == "Darwin" and machine == "arm64":
        filename = "lighter-signer-darwin-arm64.dylib"
    elif system == "Linux" and machine in {"x86_64", "amd64"}:
        filename = "lighter-signer-linux-amd64.so"
    elif system == "Linux" and machine in {"arm64", "aarch64"}:
        filename = "lighter-signer-linux-arm64.so"
    elif system == "Windows" and machine in {"amd64", "x86_64"}:
        filename = "lighter-signer-windows-amd64.dll"
    else:
        raise SignerError(f"Unsupported platform {system}/{machine}")

    path = root / filename
    if not path.exists():
        raise SignerError(f"Signer binary not found at {path}")
    return path


class _StrOrErr(ctypes.Structure):
    _fields_ = [("str", ctypes.c_char_p), ("err", ctypes.c_char_p)]


class _SignedTx(ctypes.Structure):
    _fields_ = [
        ("txType", ctypes.c_uint8),
        ("txInfo", ctypes.c_char_p),
        ("txHash", ctypes.c_char_p),
        ("messageToSign", ctypes.c_char_p),
        ("err", ctypes.c_char_p),
    ]


@dataclass(frozen=True)
class SignedTx:
    tx_type: int
    tx_info: str
    tx_hash: Optional[str]


class LighterSigner:
    """
    Thin ctypes wrapper around the native Lighter signer binary.
    """

    def __init__(
        self,
        base_url: str,
        account_index: int,
        api_key_index: int,
        api_key_private: str,
        chain_id: int,
        root: Path | None = None,
    ) -> None:
        path = _resolve_signer_path(root)
        self._lib = ctypes.CDLL(str(path))

        self._lib.CreateClient.argtypes = [
            ctypes.c_char_p,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_longlong,
        ]
        self._lib.CreateClient.restype = ctypes.c_char_p

        self._lib.CreateAuthToken.argtypes = [
            ctypes.c_longlong,
            ctypes.c_int,
            ctypes.c_longlong,
        ]
        self._lib.CreateAuthToken.restype = _StrOrErr

        self._lib.SignCreateOrder.argtypes = [
            ctypes.c_int,
            ctypes.c_longlong,
            ctypes.c_longlong,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_longlong,
            ctypes.c_longlong,
            ctypes.c_int,
            ctypes.c_longlong,
        ]
        self._lib.SignCreateOrder.restype = _SignedTx

        self._lib.SignCancelOrder.argtypes = [
            ctypes.c_int,
            ctypes.c_longlong,
            ctypes.c_longlong,
            ctypes.c_int,
            ctypes.c_longlong,
        ]
        self._lib.SignCancelOrder.restype = _SignedTx

        err = self._lib.CreateClient(
            base_url.encode("utf-8"),
            api_key_private.encode("utf-8"),
            int(chain_id),
            int(api_key_index),
            int(account_index),
        )
        if err:
            raise SignerError(err.decode("utf-8"))

        self._account_index = int(account_index)
        self._api_key_index = int(api_key_index)

    def auth_token(self, *, expiry_seconds: int = 600, timestamp: int | None = None) -> str:
        """
        Create an auth token valid for the given duration.
        """
        ts = int(time.time()) if timestamp is None else timestamp
        res = self._lib.CreateAuthToken(
            ctypes.c_longlong(ts + expiry_seconds),
            ctypes.c_int(self._api_key_index),
            ctypes.c_longlong(self._account_index),
        )
        if res.err:
            raise SignerError(res.err.decode("utf-8"))
        return res.str.decode("utf-8")

    def sign_create_order(
        self,
        market_index: int,
        client_order_index: int,
        base_amount_int: int,
        price_int: int,
        is_ask: bool,
        order_type: int,
        time_in_force: int,
        *,
        reduce_only: bool = False,
        trigger_price: int = 0,
        order_expiry: int = -1,
        nonce: int,
    ) -> SignedTx:
        res = self._lib.SignCreateOrder(
            ctypes.c_int(market_index),
            ctypes.c_longlong(client_order_index),
            ctypes.c_longlong(base_amount_int),
            ctypes.c_int(price_int),
            ctypes.c_int(int(is_ask)),
            ctypes.c_int(order_type),
            ctypes.c_int(time_in_force),
            ctypes.c_int(int(reduce_only)),
            ctypes.c_int(trigger_price),
            ctypes.c_longlong(order_expiry),
            ctypes.c_longlong(nonce),
            ctypes.c_int(self._api_key_index),
            ctypes.c_longlong(self._account_index),
        )
        if res.err:
            raise SignerError(res.err.decode("utf-8"))
        return SignedTx(int(res.txType), res.txInfo.decode("utf-8"), _safe_decode(res.txHash))

    def sign_cancel_order(
        self,
        market_index: int,
        order_index: int,
        *,
        nonce: int,
    ) -> SignedTx:
        res = self._lib.SignCancelOrder(
            ctypes.c_int(market_index),
            ctypes.c_longlong(order_index),
            ctypes.c_longlong(nonce),
            ctypes.c_int(self._api_key_index),
            ctypes.c_longlong(self._account_index),
        )
        if res.err:
            raise SignerError(res.err.decode("utf-8"))
        return SignedTx(int(res.txType), res.txInfo.decode("utf-8"), _safe_decode(res.txHash))


def _safe_decode(raw: ctypes.c_char_p | None) -> Optional[str]:
    return raw.decode("utf-8") if raw else None
