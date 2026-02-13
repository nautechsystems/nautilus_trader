# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import base64
import os
import warnings

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.env import get_env_key


_ED25519_OID = b"\x06\x03\x2b\x65\x70"


def is_ed25519_private_key(api_secret: str) -> bool:
    """
    Check whether `api_secret` looks like an Ed25519 private key.

    Strips any PEM headers/footers, base64-decodes, and checks for
    the Ed25519 PKCS#8 OID (1.3.101.112). This correctly distinguishes
    Ed25519 from RSA PEM keys and plain HMAC secrets.

    Raw 32-byte Ed25519 seeds (without PKCS#8 wrapping) will not be
    detected; use explicit ``key_type=ED25519`` for those.

    """
    try:
        key_data = "".join(line for line in api_secret.splitlines() if not line.startswith("-----"))
        key_bytes = base64.b64decode(key_data, validate=True)
        return _ED25519_OID in key_bytes
    except Exception:
        return False


def extract_ed25519_private_key(api_secret: str) -> bytes:
    """
    Extract 32-byte Ed25519 private key from API secret.

    Handles both raw base64 and PEM-formatted keys by stripping
    PEM headers/footers before decoding.

    Note: Only unencrypted PEM keys are supported. Encrypted PEM files
    with Proc-Type/DEK-Info headers will fail to decode.

    """
    key_data = "".join(line for line in api_secret.splitlines() if not line.startswith("-----"))
    key_bytes = base64.b64decode(key_data)

    # Extract 32-byte seed (works for both raw and PKCS#8 DER format)
    return key_bytes[-32:]


def get_api_key(account_type: BinanceAccountType, environment: BinanceEnvironment) -> str:
    """
    Get Binance API key from environment variables.

    Checks for deprecated `*_ED25519_*` variables first (with deprecation warning), then
    falls back to standard variables. Demo uses a single shared key across products.

    """
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return _get_credential_with_ed25519_fallback(
                ed25519_key="BINANCE_TESTNET_ED25519_API_KEY",
                standard_key="BINANCE_TESTNET_API_KEY",
            )
        else:
            return _get_credential_with_ed25519_fallback(
                ed25519_key="BINANCE_FUTURES_TESTNET_ED25519_API_KEY",
                standard_key="BINANCE_FUTURES_TESTNET_API_KEY",
            )

    if environment == BinanceEnvironment.DEMO:
        return get_env_key("BINANCE_DEMO_API_KEY")

    return _get_credential_with_ed25519_fallback(
        ed25519_key="BINANCE_ED25519_API_KEY",  # gitleaks:allow
        standard_key="BINANCE_API_KEY",
    )


def get_api_secret(account_type: BinanceAccountType, environment: BinanceEnvironment) -> str:
    """
    Get Binance API secret from environment variables.

    Checks for deprecated `*_ED25519_*` variables first (with deprecation warning), then
    falls back to standard variables. Demo uses a single shared key across products.

    """
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return _get_credential_with_ed25519_fallback(
                ed25519_key="BINANCE_TESTNET_ED25519_API_SECRET",
                standard_key="BINANCE_TESTNET_API_SECRET",
            )
        else:
            return _get_credential_with_ed25519_fallback(
                ed25519_key="BINANCE_FUTURES_TESTNET_ED25519_API_SECRET",
                standard_key="BINANCE_FUTURES_TESTNET_API_SECRET",
            )

    if environment == BinanceEnvironment.DEMO:
        return get_env_key("BINANCE_DEMO_API_SECRET")

    return _get_credential_with_ed25519_fallback(
        ed25519_key="BINANCE_ED25519_API_SECRET",  # gitleaks:allow
        standard_key="BINANCE_API_SECRET",
    )


def _get_credential_with_ed25519_fallback(ed25519_key: str, standard_key: str) -> str:
    standard_value = os.environ.get(standard_key)
    if standard_value is not None:
        return standard_value

    # Fall back to deprecated env var
    ed25519_value = os.environ.get(ed25519_key)
    if ed25519_value is not None:
        warnings.warn(
            f"'{ed25519_key}' is deprecated and will be removed in a future version. "
            f"Set your Ed25519 credentials in '{standard_key}' instead.",
            DeprecationWarning,
            stacklevel=4,
        )
        return ed25519_value

    raise ValueError(f"'{standard_key}' not found in environment")
