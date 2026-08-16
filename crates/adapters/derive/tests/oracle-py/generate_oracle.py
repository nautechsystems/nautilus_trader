#!/usr/bin/env python3
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
"""
Independent signing oracle for Derive self-custodial trade actions.

Runs the official Derive action-signing SDK against fixed inputs and writes a
JSON fixture consumed by the `signing` module tests in this crate. Every
locally supported signed action (`private/order`, `private/trigger_order`, and
`private/replace`) signs `TradeModuleData` through the same EIP-712 pipeline,
so one vector set for the trade module covers the full signing surface.

The SDK signs with RFC 6979 deterministic nonces, so identical inputs produce
identical signatures across runs and across independent implementations, which
makes byte equality against this fixture a valid oracle.

Regenerating the fixture:

    git clone https://github.com/derivexyz/v2-action-signing-python
    cd v2-action-signing-python
    git checkout <upstream_revision>  # UPSTREAM_REVISION in this script
    python3 -m venv .venv
    .venv/bin/pip install .
    .venv/bin/python <nautilus_trader repository root>/crates/adapters/derive/tests/oracle-py/generate_oracle.py

The generator resolves its default output path from its own location, so it
writes the fixture into `test_data/common/` regardless of the working
directory.

The upstream revision below is the pin recorded in the fixture metadata. Moving
to a new revision (for example a future V3 signer) is a re-pin plus a
regeneration of this fixture, not a change to the Rust test structure.

"""

from __future__ import annotations

import argparse
import json
import sys
from decimal import Decimal
from pathlib import Path
from typing import Any

from derive_action_signing import SignedAction
from derive_action_signing import TradeModuleData
from web3 import Web3


UPSTREAM_VERSION = "0.0.13"
UPSTREAM_REVISION = "d1914d61985e33559244da242892c7255b6fd0ca"
UPSTREAM_SOURCE = "github.com/derivexyz/v2-action-signing-python"

DEFAULT_OUT = (
    Path(__file__).resolve().parents[2]
    / "test_data"
    / "common"
    / "signing_trade_action_vectors.json"
)

# Session key published in the upstream SDK's own test suite and reused by this
# crate's signing tests. It controls no funds.
SESSION_KEY = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd"
# Smart-contract wallet published alongside it in the upstream test suite.
OWNER = "0x8772185a1516f0d61fC1c2524926BfC69F95d698"

SUBACCOUNT_ID = 30769
# Matches the upstream test convention (MAX_INT_32) and keeps the Rust side's
# minimum-TTL validation satisfiable until 2038 without a clock dependency.
SIGNATURE_EXPIRY_SEC = 2147483647
BASE_NONCE = 1695836058725001

# Protocol constants from https://docs.derive.xyz/reference/protocol-constants,
# matching `src/common/consts.rs`.
DOMAINS = {
    "mainnet": "0xd96e5f90797da7ec8dc4e276260c7f3f87fedf68775fbe1ef116e996fc60441b",
    "testnet": "0x9bcf4dc06df5d8bf23af818d5716491b995020f377d3b7b64c29ed14e3dd1105",
}
TRADE_MODULES = {
    "mainnet": "0xB8D20c2B7a1Ad2EE33Bc50eF10876eD3035b5e7b",
    "testnet": "0x87F2863866D85E3192a35A73b388BD625D83f2be",
}
ACTION_TYPEHASH = "0x4d7a9f27c403ff9c0f19bce61d76d82f9aa29f8d6d4b0c5474607d9770d1af17"

# One vector per behavioral branch of the trade encoder and the action-hash
# composition: both environments, both sides, fractional and negative decimal
# scaling, a zero max fee, and an option sub id beyond the 64-bit range.
CASES = [
    {
        "case": "limit_buy_round_mainnet",
        "environment": "mainnet",
        "asset_address": "0x000000000000000000000000000000000000abcd",
        "sub_id": 42,
        "limit_price": "100",
        "amount": "1",
        "max_fee": "1000",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": True,
    },
    {
        "case": "limit_sell_fractional_testnet",
        "environment": "testnet",
        "asset_address": "0x000000000000000000000000000000000000beef",
        "sub_id": 0,
        "limit_price": "3500.01",
        "amount": "1.25",
        "max_fee": "0.5",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": False,
    },
    {
        "case": "sell_negative_amount_testnet",
        "environment": "testnet",
        "asset_address": "0x000000000000000000000000000000000000c0de",
        "sub_id": 7,
        "limit_price": "3419.55",
        "amount": "-0.75",
        "max_fee": "0.0001",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": False,
    },
    {
        "case": "option_buy_large_sub_id_mainnet",
        "environment": "mainnet",
        "asset_address": "0x000000000000000000000000000000000000abcd",
        "sub_id": 39614082202024973918552016768,
        "limit_price": "0.05",
        "amount": "10",
        "max_fee": "1",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": True,
    },
    {
        "case": "limit_buy_zero_max_fee_testnet",
        "environment": "testnet",
        "asset_address": "0x000000000000000000000000000000000000beef",
        "sub_id": 1,
        "limit_price": "2.5",
        "amount": "0.001",
        "max_fee": "0",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": True,
    },
]


def prefixed(hex_string: str) -> str:
    """
    Normalize an SDK hex output to a 0x-prefixed string.
    """
    return hex_string if hex_string.startswith("0x") else "0x" + hex_string


def build_vector(index: int, case: dict[str, Any], signer_address: str) -> dict[str, Any]:
    """
    Sign one case through the upstream SDK and capture its full output.
    """
    environment = case["environment"]
    action = SignedAction(
        subaccount_id=SUBACCOUNT_ID,
        owner=OWNER,
        signer=signer_address,
        signature_expiry_sec=SIGNATURE_EXPIRY_SEC,
        nonce=BASE_NONCE + index,
        module_address=TRADE_MODULES[environment],
        module_data=TradeModuleData(
            asset_address=case["asset_address"],
            sub_id=case["sub_id"],
            limit_price=Decimal(case["limit_price"]),
            amount=Decimal(case["amount"]),
            max_fee=Decimal(case["max_fee"]),
            recipient_id=case["recipient_id"],
            is_bid=case["is_bid"],
        ),
        DOMAIN_SEPARATOR=DOMAINS[environment],
        ACTION_TYPEHASH=ACTION_TYPEHASH,
    )
    signature = prefixed(action.sign(SESSION_KEY))
    action.validate_signature()

    module_data = action.module_data.to_abi_encoded()
    module_data_hash = Web3.keccak(module_data)
    # The SDK exposes no public accessor for these digests; its own test suite
    # calls the same private methods, so mirroring them is the provenance-safe
    # way to record upstream-computed values.
    action_hash = action._get_action_hash()
    typed_data_hash = action._to_typed_data_hash()

    # The typed-data hash is keccak256(0x1901 || domain_separator || action_hash);
    # recomputing it independently of the SDK guards the fixture itself.
    recomposed = Web3.keccak(
        bytes.fromhex("1901" + DOMAINS[environment][2:] + action_hash.hex()),
    )
    if recomposed != typed_data_hash:
        raise RuntimeError(f"case {case['case']}: typed-data hash recomposition diverged")

    return {
        "case": case["case"],
        "environment": environment,
        "domain_separator": DOMAINS[environment],
        "action_typehash": ACTION_TYPEHASH,
        "module_address": TRADE_MODULES[environment],
        "subaccount_id": SUBACCOUNT_ID,
        "nonce": BASE_NONCE + index,
        "signature_expiry_sec": SIGNATURE_EXPIRY_SEC,
        "owner": OWNER,
        "session_key": SESSION_KEY,
        "signer": signer_address,
        "trade": {
            "asset_address": case["asset_address"],
            "sub_id": str(case["sub_id"]),
            "limit_price": case["limit_price"],
            "amount": case["amount"],
            "max_fee": case["max_fee"],
            "recipient_id": case["recipient_id"],
            "is_bid": case["is_bid"],
        },
        "module_data": prefixed(module_data.hex()),
        "module_data_hash": prefixed(module_data_hash.hex()),
        "action_hash": prefixed(action_hash.hex()),
        "typed_data_hash": prefixed(typed_data_hash.hex()),
        "signature": signature,
    }


def main() -> int:
    """
    Generate all vectors and write the fixture.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help="output JSON fixture path (default: %(default)s)",
    )
    args = parser.parse_args()

    signer_address = Web3().eth.account.from_key(SESSION_KEY).address
    vectors = [build_vector(index, case, signer_address) for index, case in enumerate(CASES)]

    payload = {
        "metadata": {
            "license": (
                "MIT (declared via the pyproject classifier; the upstream "
                "repository carries no LICENSE file at the pinned revision)"
            ),
            "primitive": "derive_trade_action",
            "source": UPSTREAM_SOURCE,
            "upstream_version": UPSTREAM_VERSION,
            "upstream_revision": UPSTREAM_REVISION,
            "generated_by": "crates/adapters/derive/tests/oracle-py/generate_oracle.py",
            "procedure": (
                "Clone and install the upstream SDK at "
                f"{UPSTREAM_REVISION}, then run generate_oracle.py; the full "
                "procedure is documented in its module docstring"
            ),
            "note": (
                "Signatures use RFC 6979 deterministic nonces, so every value "
                "is a byte-equality target. Protocol constants match "
                "src/common/consts.rs and docs.derive.xyz."
            ),
        },
        "vectors": vectors,
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2)
        f.write("\n")
    print(f"wrote {len(vectors)} trade-action vectors to {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
