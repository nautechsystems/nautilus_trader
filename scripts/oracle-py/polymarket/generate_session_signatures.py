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
Generate wallet signatures and order hashes with the pinned Polymarket SDK.

Clone https://github.com/Polymarket/py-sdk, check out UPSTREAM_REVISION below,
then run `uv sync --locked --no-dev` in that checkout. Run this script with
that checkout's `.venv/bin/python`. No network or wallet access is required.

"""

import json
from dataclasses import replace
from importlib.metadata import version
from pathlib import Path

from eth_account import Account
from eth_account.messages import encode_typed_data
from eth_utils import keccak
from polymarket._internal.actions.orders.typed_data import build_order_signature
from polymarket._internal.actions.orders.typed_data import build_order_typed_data
from polymarket._internal.actions.orders.types import UnsignedOrder
from polymarket._internal.wallet import wrap_deposit_wallet_signature


UPSTREAM_VERSION = "0.15.0"
UPSTREAM_REVISION = "d26958d68a054edb01787825ff63c8c0d175d127"
UPSTREAM_SOURCE = "https://github.com/Polymarket/py-sdk"

# Public Hardhat test account; never use this key for funds.
PRIVATE_KEY = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
WALLET = "0x1111111111111111111111111111111111111111"
ZERO_BYTES32 = "0x" + "00" * 32
SIGNATURE_TYPE_POLY_1271 = 3


def main() -> None:
    """
    Generate the pinned SDK signature fixture from fixed public test inputs.
    """
    if version("polymarket-client") != UPSTREAM_VERSION:
        raise RuntimeError(f"Expected polymarket-client {UPSTREAM_VERSION}")

    account = Account.from_key(PRIVATE_KEY)
    order = {
        "salt": 123456789,
        "maker": WALLET,
        "signer": WALLET,
        "tokenId": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "makerAmount": "100000000",
        "takerAmount": "50000000",
        "side": "BUY",
        "signatureType": SIGNATURE_TYPE_POLY_1271,
        "expiration": "0",
        "timestamp": "1713398400000",
        "metadata": ZERO_BYTES32,
        "builder": ZERO_BYTES32,
        "signature": "",
    }
    vectors = []
    for neg_risk, exchange in [
        (False, "0xE111180000d2663C0091e4f400237545B87B996B"),
        (True, "0xe2222d279d744050d28e00520010520000310F59"),
    ]:
        unsigned = UnsignedOrder(
            chain_id=137,
            exchange_address=exchange,
            order_type="GTC",
            salt=order["salt"],
            maker=order["maker"],
            signer=order["signer"],
            token_id=order["tokenId"],
            maker_amount=int(order["makerAmount"]),
            taker_amount=int(order["takerAmount"]),
            side=order["side"],
            signature_type=order["signatureType"],
            expiration=int(order["expiration"]),
            timestamp=int(order["timestamp"]),
            metadata=order["metadata"],
            builder=order["builder"],
        )
        signature = (
            "0x"
            + account.sign_message(
                encode_typed_data(full_message=build_order_typed_data(unsigned)),
            ).signature.hex()
        )
        wrapped = build_order_signature(unsigned, signature)
        session = wrap_deposit_wallet_signature(
            signer=account.address,
            signer_type="SESSION_KEY",
            signature=wrapped,
        )
        vectors.append(
            {
                "neg_risk": neg_risk,
                "exchange": exchange,
                "signature_chunks": [session[i : i + 96] for i in range(0, len(session), 96)],
            },
        )

    protocol_vectors = []
    for market_version, domain_version, neg_risk, exchange in [
        ("v1", "2", False, "0xE111180000d2663C0091e4f400237545B87B996B"),
        ("v1", "2", True, "0xe2222d279d744050d28e00520010520000310F59"),
        ("v2", "3", False, "0xe3333700cA9d93003F00f0F71f8515005F6c00Aa"),
        ("v2", "3", True, "0xe3333700cA9d93003F00f0F71f8515005F6c00Aa"),
    ]:
        for signature_type, signer_type in [
            (0, "OWNER"),
            (1, "OWNER"),
            (2, "OWNER"),
            (SIGNATURE_TYPE_POLY_1271, "OWNER"),
            (SIGNATURE_TYPE_POLY_1271, "SESSION_KEY"),
        ]:
            sample = dict(order)
            sample["signatureType"] = signature_type
            sample["signer"] = (
                WALLET if signature_type == SIGNATURE_TYPE_POLY_1271 else account.address
            )
            sample["maker"] = account.address if signature_type == 0 else WALLET
            unsigned = replace(
                unsigned,
                exchange_address=exchange,
                protocol_version=domain_version,
                signature_type=signature_type,
                maker=sample["maker"],
                signer=sample["signer"],
            )
            typed = build_order_typed_data(unsigned)
            signature = (
                "0x" + account.sign_message(encode_typed_data(full_message=typed)).signature.hex()
            )
            wrapped = build_order_signature(unsigned, signature)
            if signature_type == SIGNATURE_TYPE_POLY_1271:
                wrapped = wrap_deposit_wallet_signature(
                    signer=account.address,
                    signer_type=signer_type,
                    signature=wrapped,
                )
            standard = build_order_typed_data(replace(unsigned, signature_type=0))
            standard["message"]["signatureType"] = signature_type
            encoded = encode_typed_data(full_message=standard)
            order_hash = (
                "0x" + keccak(b"\x19" + encoded.version + encoded.header + encoded.body).hex()
            )
            protocol_vectors.append(
                {
                    "market_version": market_version,
                    "neg_risk": neg_risk,
                    "signer_type": signer_type,
                    "exchange": exchange,
                    "domain_version": domain_version,
                    "order": sample,
                    "order_hash": order_hash,
                    "signature_chunks": [wrapped[i : i + 96] for i in range(0, len(wrapped), 96)],
                },
            )
    protocol_fixture = {
        "source": UPSTREAM_SOURCE,
        "upstream_revision": UPSTREAM_REVISION,
        "upstream_version": UPSTREAM_VERSION,
        "generated_by": "scripts/oracle-py/polymarket/generate_session_signatures.py",
        "private_key": PRIVATE_KEY,
        "vectors": protocol_vectors,
    }
    fixture_dir = Path(__file__).resolve().parents[3] / "crates/adapters/polymarket/test_data"
    (fixture_dir / "protocol_signatures.json").write_text(
        json.dumps(protocol_fixture, indent=2) + "\n",
        encoding="utf-8",
    )

    fixture = {
        "source": UPSTREAM_SOURCE,
        "upstream_version": UPSTREAM_VERSION,
        "upstream_revision": UPSTREAM_REVISION,
        "generated_by": "scripts/oracle-py/polymarket/generate_session_signatures.py",
        "procedure": "EIP-712 order -> Deposit Wallet signature -> SESSION_KEY envelope",
        "chain_id": 137,
        "private_key": PRIVATE_KEY,
        "order": order,
        "vectors": vectors,
    }
    output = (
        Path(__file__).resolve().parents[3]
        / "crates"
        / "adapters"
        / "polymarket"
        / "test_data"
        / "session_signatures.json"
    )
    output.write_text(json.dumps(fixture, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
