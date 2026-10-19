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

Runs derive-py 0.1.4 at the pinned revision against fixed v3 inputs. Order,
trigger-order, and replace requests share the TradeModuleData signing pipeline.

Regenerate independently of the Rust implementation:

    git clone https://github.com/derivexyz/derive-py
    cd derive-py
    git checkout fad785e6c328746b5f8a8219e14009670bc97a35
    uv venv /tmp/derive-oracle-env
    uv pip install --python /tmp/derive-oracle-env/bin/python . \
        web3==7.16.0 eth-abi==6.0.0 eth-account==0.14.0
    /tmp/derive-oracle-env/bin/python <nautilus_trader>/scripts/oracle-py/derive/generate_oracle.py

Replace <nautilus_trader> with the absolute path to the NautilusTrader checkout.

The generator checks the installed SDK source hashes and version before writing
fixtures. It also derives the domain and Action typehash from the published v3
formulas, independently of the SDK constants. RFC 6979 makes signatures exact
byte-equality targets. Negative vectors prove signed-int256 encoding only;
the venue rejects negative order prices and amounts. No credentials or live
requests are needed.

"""

from __future__ import annotations

import argparse
import hashlib
import inspect
import json
import sys
from decimal import Decimal
from importlib.metadata import version
from pathlib import Path
from typing import Any

from derive_py._web3.action_signing import SignedAction
from derive_py._web3.action_signing import TradeModuleData
from derive_py._web3.action_signing import utils
from derive_py.config import contracts
from derive_py.config.contracts import CONFIGS
from derive_py.data_types import Chain
from eth_abi import encode
from web3 import Web3


UPSTREAM_VERSION = "0.1.4"
UPSTREAM_REVISION = "fad785e6c328746b5f8a8219e14009670bc97a35"
UPSTREAM_SOURCE = "https://github.com/derivexyz/derive-py"

DEFAULT_OUT = (
    Path(__file__).resolve().parents[3]
    / "crates"
    / "adapters"
    / "derive"
    / "test_data"
    / "common"
    / "signing_trade_action_vectors.json"
)

# Published test inputs retained from derivexyz/v2-action-signing-python at
# d1914d61985e33559244da242892c7255b6fd0ca, the session key controls no funds
SESSION_KEY = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd"
# Legacy smart-contract wallet used as a distinct test owner
OWNER = "0x8772185a1516f0d61fC1c2524926BfC69F95d698"

SUBACCOUNT_ID = 30769
# Fixed expiry keeps minimum-TTL validation satisfiable until 2038 without a
# clock dependency
SIGNATURE_EXPIRY_SEC = 2147483647
BASE_NONCE = 1695836058725001000
DECIMAL_PRECISION = 12

# SDK configuration is independent of the Rust constants.
CHAINS = {"mainnet": Chain.ETHEREUM, "testnet": Chain.SEPOLIA}
CHAIN_IDS = {"mainnet": 1, "testnet": 11155111}
DOMAINS = {name: CONFIGS[chain].DOMAIN_SEPARATOR for name, chain in CHAINS.items()}
TRADE_MODULES = {name: CONFIGS[chain].contracts.TRADE_MODULE for name, chain in CHAINS.items()}
ACTION_TYPEHASH = CONFIGS[Chain.ETHEREUM].ACTION_TYPEHASH
SDK_SOURCE_HASHES = {
    "contracts": "c3de85d9797daa21540a960cfd3ac48205b2d1f31ed089deaa9ac8f7d922a7e5",
    "SignedAction": "11705789160cc46ac7cf942b883e5cfcfe785400d7d20d54e4c0990574985880",
    "TradeModuleData": "d62e7dbcdc869e0c5e6b2a98df1efec48a10c19cb134f05588c6be05f61b0885",
    "utils": "cb7d752de48b1bd2541c010afd28abd1ff08f01a351341fecc1d29e13545448a",
}

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
    {
        "case": "precision_boundary_mainnet",
        "environment": "mainnet",
        "asset_address": "0x000000000000000000000000000000000000beef",
        "sub_id": 19,
        "limit_price": "123.123456789012",
        "amount": "0.000000000001",
        "max_fee": "0.987654321098",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": True,
    },
    {
        "case": "precision_boundary_negative_testnet",
        "environment": "testnet",
        "asset_address": "0x000000000000000000000000000000000000c0de",
        "sub_id": 23,
        "limit_price": "-123.123456789012",
        "amount": "-0.000000000001",
        "max_fee": "0.987654321098",
        "recipient_id": SUBACCOUNT_ID,
        "is_bid": False,
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
    for field in ("limit_price", "amount", "max_fee"):
        value = Decimal(case[field])
        if value.as_tuple().exponent < -DECIMAL_PRECISION:
            raise ValueError(
                f"case {case['case']}: {field} exceeds {DECIMAL_PRECISION} fractional digits",
            )
    wire = action.to_json()
    for field in ("limit_price", "amount", "max_fee"):
        if Decimal(wire[field]) != Decimal(case[field]):
            raise RuntimeError(f"case {case['case']}: {field} wire value differs")
    if wire["nonce"] != str(BASE_NONCE + index):
        raise RuntimeError(f"case {case['case']}: nonce wire value differs")
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
        "nonce": str(BASE_NONCE + index),
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

    if version("derive-py") != UPSTREAM_VERSION:
        raise RuntimeError("installed derive-py version differs from the pinned oracle")
    for name, source in [
        ("SignedAction", SignedAction),
        ("TradeModuleData", TradeModuleData),
        ("utils", utils),
        ("contracts", contracts),
    ]:
        actual = hashlib.sha256(Path(inspect.getfile(source)).read_bytes()).hexdigest()
        if actual != SDK_SOURCE_HASHES[name]:
            raise RuntimeError(f"installed SDK source differs for {name}")
    action_type = (
        "Action(uint256 subaccountId,uint256 nonce,address module,bytes data,"
        "uint256 expiry,address owner,address signer)"
    )
    if prefixed(Web3.keccak(text=action_type).hex()) != ACTION_TYPEHASH:
        raise RuntimeError("SDK Action typehash differs from the v3 formula")
    for environment, chain_id in CHAIN_IDS.items():
        domain = Web3.keccak(
            encode(
                ["bytes32", "bytes32", "bytes32", "uint256", "address"],
                [
                    Web3.keccak(
                        text="EIP712Domain(string name,string version,"
                        "uint256 chainId,address verifyingContract)",
                    ),
                    Web3.keccak(text="Matching"),
                    Web3.keccak(text="1.0"),
                    chain_id,
                    "0xeB8d770ec18DB98Db922E9D83260A585b9F0DeAD",
                ],
            ),
        )
        if prefixed(domain.hex()) != DOMAINS[environment]:
            raise RuntimeError(f"SDK {environment} domain differs from the v3 formula")

    signer_address = Web3().eth.account.from_key(SESSION_KEY).address
    vectors = [build_vector(index, case, signer_address) for index, case in enumerate(CASES)]

    payload = {
        "metadata": {
            "license": "MIT, Copyright (c) 2026 derive-py contributors",
            "primitive": "derive_trade_action",
            "source": UPSTREAM_SOURCE,
            "upstream_version": UPSTREAM_VERSION,
            "upstream_revision": UPSTREAM_REVISION,
            "source_sha256": SDK_SOURCE_HASHES,
            "dependencies": {name: version(name) for name in ["web3", "eth-abi", "eth-account"]},
            "specification": "https://docs.derive.xyz/authentication/action-signing",
            "generated_by": "scripts/oracle-py/derive/generate_oracle.py",
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
