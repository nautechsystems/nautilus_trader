#!/usr/bin/env python3
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
Mainnet sanity script: place + cancel a tiny Lighter perp order using the native signer.

This follows the validation runbook in ``docs/lighter_adapter/PR_NOTES_AUTH_VALIDATION.md`` and is
intended for quick manual checks (not automated tests).

Requirements:
- .env populated with LIGHTER_HTTP_BASE, LIGHTER_ACCOUNT_INDEX, LIGHTER_API_KEY_INDEX,
  LIGHTER_API_KEY_PRIVATE_KEY (or LIGHTER_API_SECRET).
- Signer binaries present under /tmp/lighter-python/lighter/signers (as per README).
- requests library available (installed with dev deps).

Usage:
    python examples/live/lighter/lighter_place_cancel_sanity.py --market btc --notional 10
    python examples/live/lighter/lighter_place_cancel_sanity.py --market eth --discount 0.05

The script:
1) Fetches price/size scales from /orderBookDetails.
2) Signs a limit order below the last trade price (discount).
3) Submits via /sendTx.
4) Cancels the same client_order_index via /sendTx.
5) Prints minimal status; all secrets stay in env vars.
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from decimal import Decimal

import requests


REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
if REPO_ROOT not in sys.path:
    sys.path.insert(0, REPO_ROOT)

from nautilus_trader.adapters.lighter.constants import LIGHTER_MAINNET_HTTP_BASE  # noqa: E402
from nautilus_trader.adapters.lighter.constants import LIGHTER_TESTNET_HTTP_BASE
from nautilus_trader.adapters.lighter.signer import LighterSigner  # noqa: E402


MARKET_MAP = {
    "eth": 0,
    "btc": 1,
}


def env(name: str, default: str | None = None) -> str:
    value = os.getenv(name, default)
    if value is None:
        raise SystemExit(f"Missing required env var {name}")
    return value


def fetch_book_details(base_url: str, market_id: int) -> dict:
    resp = requests.get(
        f"{base_url}/api/v1/orderBookDetails",
        params={"market_id": market_id},
        timeout=10,
    )
    resp.raise_for_status()
    body = resp.json()
    return body["order_book_details"][0]


def fetch_nonce(base_url: str, account_index: int, api_key_index: int) -> int:
    resp = requests.get(
        f"{base_url}/api/v1/nextNonce",
        params={"account_index": account_index, "api_key_index": api_key_index},
        timeout=10,
    )
    resp.raise_for_status()
    return resp.json()["nonce"]


def post_send_tx(base_url: str, tx_type: int, tx_info: str) -> dict:
    resp = requests.post(
        f"{base_url}/api/v1/sendTx",
        data={"tx_type": tx_type, "tx_info": tx_info, "price_protection": True},
        timeout=10,
    )
    resp.raise_for_status()
    return resp.json()


def price_to_int(price: Decimal, decimals: int) -> int:
    return int(price * (Decimal(10) ** decimals))


def size_to_int(size: Decimal, decimals: int) -> int:
    return int(size * (Decimal(10) ** decimals))


def main() -> None:
    parser = argparse.ArgumentParser(description="Lighter mainnet place+cancel sanity check.")
    parser.add_argument("--market", choices=["btc", "eth"], default="btc", help="Perp market to test")
    parser.add_argument("--notional", type=float, default=10.0, help="Approx USD notional for the test order")
    parser.add_argument("--discount", type=float, default=0.10, help="Place limit this fraction below last price")
    parser.add_argument("--testnet", action="store_true", help="Use testnet endpoints/chain id")
    args = parser.parse_args()

    base_http = os.getenv(
        "LIGHTER_HTTP_BASE",
        LIGHTER_TESTNET_HTTP_BASE if args.testnet else LIGHTER_MAINNET_HTTP_BASE,
    ).rstrip("/")

    account_index = int(env("LIGHTER_ACCOUNT_INDEX"))
    api_key_index = int(env("LIGHTER_API_KEY_INDEX", "2"))
    api_key = env("LIGHTER_API_KEY_PRIVATE_KEY", os.getenv("LIGHTER_API_SECRET", "")).removeprefix("0x")
    if not api_key:
        raise SystemExit("LIGHTER_API_KEY_PRIVATE_KEY (or LIGHTER_API_SECRET) must be set")

    market_id = MARKET_MAP[args.market]
    print(f"Using market_id={market_id} ({args.market.upper()}), base={base_http}, account={account_index}")

    book = fetch_book_details(base_http, market_id)
    price_decimals = int(book["price_decimals"])
    size_decimals = int(book["size_decimals"])
    last_price = Decimal(str(book["last_trade_price"]))
    min_base = Decimal(str(book["min_base_amount"]))

    limit_price = last_price * (Decimal(1) - Decimal(str(args.discount)))
    size = (Decimal(str(args.notional)) / limit_price).max(min_base)

    price_int = price_to_int(limit_price, price_decimals)
    size_int = size_to_int(size, size_decimals)

    print(f"Last price={last_price} -> limit={limit_price} (int {price_int}), size={size} (int {size_int})")

    signer_base = base_http.removesuffix("/api/v1")
    signer = LighterSigner(
        base_url=signer_base,
        account_index=account_index,
        api_key_index=api_key_index,
        api_key_private=api_key,
        chain_id=304 if not args.testnet else 300,
    )

    nonce = fetch_nonce(base_http, account_index, api_key_index)
    client_order_index = int(time.time_ns() // 1_000_000)
    expiry_ms = int((time.time() + 600) * 1000)  # 10 minutes from now
    signed_create = signer.sign_create_order(
        market_index=market_id,
        client_order_index=client_order_index,
        base_amount_int=size_int,
        price_int=price_int,
        is_ask=False,
        order_type=2,  # 0=limit, 1=market, 2=stop_loss (using 2 as workaround)
        time_in_force=0,  # GTT (Good Till Time)
        nonce=nonce,
        reduce_only=False,
        trigger_price=price_int,  # Must match price for order_type=2; shows as "S/L Market"
        order_expiry=expiry_ms,
    )
    print(f"Signed create: tx_hash={signed_create.tx_hash}")

    resp_create = post_send_tx(base_http, signed_create.tx_type, signed_create.tx_info)
    print(f"sendTx(create) response: {resp_create}")

    # Reuse or fetch fresh nonce for cancel; safer to refresh.
    cancel_nonce = fetch_nonce(base_http, account_index, api_key_index)
    signed_cancel = signer.sign_cancel_order(
        market_index=market_id,
        order_index=client_order_index,
        nonce=cancel_nonce,
    )
    print(f"Signed cancel: tx_hash={signed_cancel.tx_hash}")

    resp_cancel = post_send_tx(base_http, signed_cancel.tx_type, signed_cancel.tx_info)
    print(f"sendTx(cancel) response: {resp_cancel}")

    token = signer.auth_token()
    orders = requests.get(
        f"{base_http}/api/v1/accountActiveOrders",
        params={"account_index": account_index, "market_id": market_id},
        headers={"Authorization": f"Bearer {token}"},
        timeout=10,
    ).json()
    print(f"Active orders after cancel: {orders.get('orders')}")


if __name__ == "__main__":
    main()
