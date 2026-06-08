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
Layer 2 oracle for the Lighter L2 tx signer.

Loads the closed-source signer that ships with `lighter-python`, runs it against
deterministic inputs covering the trading-critical L2 tx types, and writes a
JSON fixture consumed by `signing/tx/encode.rs` tests.

The signer's `SignedHash` is a deterministic Poseidon2 hash over the tx body
elements. The signature itself uses a randomly sampled nonce `k`, so signature
bytes vary between runs; the fixture is regenerated when the signer pin moves
and the tests treat `sig` as a single valid witness rather than an equality
target.

"""

from __future__ import annotations

import argparse
import base64
import ctypes
import json
import sys
from pathlib import Path


CHAIN_ID_TESTNET = 300

# Tx type discriminants, mirrored from the lighter-go constants.
TX_TYPE_L2_CREATE_ORDER = 14
TX_TYPE_L2_CANCEL_ORDER = 15
TX_TYPE_L2_MODIFY_ORDER = 17
TX_TYPE_L2_APPROVE_INTEGRATOR = 45


class SignedTxResponse(ctypes.Structure):
    _fields_ = [
        ("txType", ctypes.c_uint8),
        ("txInfo", ctypes.c_void_p),
        ("txHash", ctypes.c_void_p),
        ("messageToSign", ctypes.c_void_p),
        ("err", ctypes.c_void_p),
    ]


class StrOrErr(ctypes.Structure):
    _fields_ = [("str", ctypes.c_void_p), ("err", ctypes.c_void_p)]


def take_str(lib: ctypes.CDLL, ptr: int | None) -> str | None:
    """
    Drain a Go-allocated C string and free the underlying buffer.
    """
    if not ptr:
        return None
    s = ctypes.string_at(ptr).decode("utf-8")
    lib.Free(ptr)
    return s


def setup_lib(path: Path) -> ctypes.CDLL:
    lib = ctypes.CDLL(str(path))

    lib.CreateClient.argtypes = [
        ctypes.c_char_p,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    lib.CreateClient.restype = ctypes.c_void_p

    lib.SignCreateOrder.argtypes = [
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
        ctypes.c_int,
        ctypes.c_uint8,
        ctypes.c_longlong,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    lib.SignCreateOrder.restype = SignedTxResponse

    lib.SignCancelOrder.argtypes = [
        ctypes.c_int,
        ctypes.c_longlong,
        ctypes.c_uint8,
        ctypes.c_longlong,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    lib.SignCancelOrder.restype = SignedTxResponse

    lib.SignModifyOrder.argtypes = [
        ctypes.c_int,
        ctypes.c_longlong,
        ctypes.c_longlong,
        ctypes.c_longlong,
        ctypes.c_longlong,
        ctypes.c_longlong,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_uint8,
        ctypes.c_longlong,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    lib.SignModifyOrder.restype = SignedTxResponse

    lib.SignApproveIntegrator.argtypes = [
        ctypes.c_longlong,
        ctypes.c_uint32,
        ctypes.c_uint32,
        ctypes.c_uint32,
        ctypes.c_uint32,
        ctypes.c_longlong,
        ctypes.c_uint8,
        ctypes.c_longlong,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    lib.SignApproveIntegrator.restype = SignedTxResponse

    lib.CreateAuthToken.argtypes = [
        ctypes.c_longlong,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    lib.CreateAuthToken.restype = StrOrErr

    lib.Free.argtypes = [ctypes.c_void_p]
    lib.Free.restype = None
    return lib


def decode(lib: ctypes.CDLL, resp: SignedTxResponse) -> dict:
    err = take_str(lib, resp.err)
    info = take_str(lib, resp.txInfo)
    tx_hash = take_str(lib, resp.txHash)
    take_str(lib, resp.messageToSign)
    if err:
        raise RuntimeError(f"signer returned error: {err}")
    if info is None or tx_hash is None:
        raise RuntimeError("signer returned empty tx_info/tx_hash")
    parsed = json.loads(info)
    sig_b64 = parsed["Sig"]
    sig_hex = base64.b64decode(sig_b64).hex()
    return {
        "tx_type": resp.txType,
        "tx_info": info,
        "tx_info_decoded": parsed,
        "tx_hash": tx_hash,
        "sig": sig_hex,
    }


def fixed_private_key() -> str:
    # 40-byte (80-hex) deterministic key. Bytes are arbitrary but non-trivial
    # so every limb of the underlying scalar takes a non-zero value.
    return "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001"


def derived_public_key_hex(lib: ctypes.CDLL, sk_hex: str) -> str:
    """
    Re-run the signer once to harvest the matching public-key bytes.

    The closed signer does not export PubKey directly to Python, but every
    signed tx response embeds the pubkey-derived `tx_hash`, and the signer
    accepts the same private key in `CreateClient`. We capture the pubkey by
    signing a throwaway tx and decoding it, but a simpler path is to just
    bake the public key recovered from the curve-side fixtures into the
    fixture file. To keep this script self-contained we instead compute the
    pubkey out-of-band and the caller passes it in.

    """
    raise NotImplementedError("PublicKey derivation is performed in Rust at fixture-load time")


def gen_create_order(lib: ctypes.CDLL, ctx: dict, fields: dict) -> dict:
    resp = lib.SignCreateOrder(
        fields["market_index"],
        fields["client_order_index"],
        fields["base_amount"],
        fields["price"],
        int(fields["is_ask"]),
        fields["order_type"],
        fields["time_in_force"],
        int(fields["reduce_only"]),
        fields["trigger_price"],
        fields["order_expiry"],
        fields["integrator_account_index"],
        fields["integrator_taker_fee"],
        fields["integrator_maker_fee"],
        fields["skip_nonce"],
        ctx["nonce"],
        ctx["api_key_index"],
        ctx["account_index"],
    )
    return decode(lib, resp)


def gen_cancel_order(lib: ctypes.CDLL, ctx: dict, fields: dict) -> dict:
    resp = lib.SignCancelOrder(
        fields["market_index"],
        fields["index"],
        fields["skip_nonce"],
        ctx["nonce"],
        ctx["api_key_index"],
        ctx["account_index"],
    )
    return decode(lib, resp)


def gen_modify_order(lib: ctypes.CDLL, ctx: dict, fields: dict) -> dict:
    resp = lib.SignModifyOrder(
        fields["market_index"],
        fields["index"],
        fields["base_amount"],
        fields["price"],
        fields["trigger_price"],
        fields["integrator_account_index"],
        fields["integrator_taker_fee"],
        fields["integrator_maker_fee"],
        fields["skip_nonce"],
        ctx["nonce"],
        ctx["api_key_index"],
        ctx["account_index"],
    )
    return decode(lib, resp)


def gen_approve_integrator(lib: ctypes.CDLL, ctx: dict, fields: dict) -> dict:
    resp = lib.SignApproveIntegrator(
        fields["integrator_account_index"],
        fields["max_perps_taker_fee"],
        fields["max_perps_maker_fee"],
        fields["max_spot_taker_fee"],
        fields["max_spot_maker_fee"],
        fields["approval_expiry"],
        fields["skip_nonce"],
        ctx["nonce"],
        ctx["api_key_index"],
        ctx["account_index"],
    )
    return decode(lib, resp)


def gen_auth_token(lib: ctypes.CDLL, deadline: int, api_key_index: int, account_index: int) -> str:
    """
    Drive the closed signer's `CreateAuthToken` for a single deadline.

    `CreateAuthToken(deadline, api_key_index, account_index)` returns the
    serialized `"{message}:{hex(sig)}"` string. The signer reuses the client
    handle established by `CreateClient`, so the caller must run that first.

    """
    resp = lib.CreateAuthToken(deadline, api_key_index, account_index)
    err = take_str(lib, resp.err)
    token = take_str(lib, resp.str)
    if err:
        raise RuntimeError(f"CreateAuthToken returned error: {err}")
    if token is None:
        raise RuntimeError("CreateAuthToken returned empty token")
    return token


def build_auth_vectors(
    lib: ctypes.CDLL,
    sk_hex: str,
    chain_id: int,
    account_index: int,
    seeded_api_key: int,
) -> list[dict]:
    """
    Generate auth-token vectors at fixed deadlines under the seeded signer.

    Each entry pins the inputs that drove the closed signer's `CreateAuthToken`
    plus the resulting token string. The Rust side recomputes the digest from
    `message` and verifies the embedded signature under the public key derived
    from `sk` to gate behavioural equivalence.

    The closed signer requires a `CreateClient` call for every `(api_key_index,
    account_index)` pair before signing; the seeded key is reused for the
    other vectors so the script does not need to re-initialise per case.

    """
    fixed_deadlines = [
        # Wide spread of deadlines so distinct ASCII chunks appear in the
        # canonical-byte preimage; api_key_index stays on the seeded slot.
        1_700_000_000,
        1_777_809_907,
        1_999_999_999,
        1_111_111_111,
        1_234_567_890,
    ]
    vectors: list[dict] = []
    for deadline in fixed_deadlines:
        token = gen_auth_token(lib, deadline, seeded_api_key, account_index)
        vectors.append(
            {
                "sk": sk_hex,
                "account_index": account_index,
                "api_key_index": seeded_api_key,
                "deadline": deadline,
                "token": token,
            },
        )

    # Add one vector under a distinct api_key_index to cover the second-limb
    # path. `CreateClient` is required per (api_key, account) pair.
    extra_api_key = 0
    err = lib.CreateClient(
        b"https://placeholder",
        sk_hex.encode("ascii"),
        chain_id,
        extra_api_key,
        account_index,
    )
    err_s = take_str(lib, err)
    if err_s:
        raise RuntimeError(f"CreateClient(api_key={extra_api_key}) failed: {err_s}")
    extra_deadline = 1_888_888_888
    token = gen_auth_token(lib, extra_deadline, extra_api_key, account_index)
    vectors.append(
        {
            "sk": sk_hex,
            "account_index": account_index,
            "api_key_index": extra_api_key,
            "deadline": extra_deadline,
            "token": token,
        },
    )

    return vectors


def write_auth_oracle(
    lib: ctypes.CDLL,
    sk_hex: str,
    chain_id: int,
    account_index: int,
    seeded_api_key: int,
    out_path: Path,
) -> int:
    vectors = build_auth_vectors(lib, sk_hex, chain_id, account_index, seeded_api_key)
    payload = {
        "metadata": {
            "license": "Apache-2.0 (SDK repository; compiled signer binary)",
            "primitive": "lighter_auth_token",
            "source": "github.com/elliottech/lighter-python",
            "note": (
                "Sig is non-deterministic (random k); the Rust side verifies "
                "each oracle token under the derived pubkey rather than "
                "asserting byte equality on the sig."
            ),
        },
        "vectors": vectors,
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2)
        f.write("\n")
    print(f"wrote {len(vectors)} auth-token vectors to {out_path}")
    return len(vectors)


def build_vector(kind: str, ctx: dict, fields: dict, sig_resp: dict) -> dict:
    body = {
        "kind": kind,
        "chain_id": ctx["chain_id"],
        "sk": ctx["private_key"],
        "account_index": ctx["account_index"],
        "api_key_index": ctx["api_key_index"],
        "nonce": ctx["nonce"],
        "fields": fields,
        "tx_type": sig_resp["tx_type"],
        "tx_info": sig_resp["tx_info"],
        "tx_hash": sig_resp["tx_hash"],
        "sig": sig_resp["sig"],
        # Re-export the parsed ExpiredAt because the closed signer auto-fills
        # it from wall-clock time inside the FFI; the Rust side reads it back
        # to reconstruct the same hash preimage.
        "expired_at": sig_resp["tx_info_decoded"]["ExpiredAt"],
    }
    return body


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--signer",
        type=Path,
        required=True,
        help="path to the lighter-signer-*.{so,dylib,dll}",
    )
    parser.add_argument(
        "--out",
        type=Path,
        required=True,
        help="output JSON fixture path for the L2 tx oracle",
    )
    parser.add_argument(
        "--auth-out",
        type=Path,
        default=None,
        help="optional output path for the auth-token oracle fixture",
    )
    args = parser.parse_args()

    lib = setup_lib(args.signer)
    sk = fixed_private_key()
    chain_id = CHAIN_ID_TESTNET
    account_index = 12345
    api_key_index = 5

    err = lib.CreateClient(
        b"https://placeholder",
        sk.encode("ascii"),
        chain_id,
        api_key_index,
        account_index,
    )
    err_s = take_str(lib, err)
    if err_s:
        print(f"CreateClient failed: {err_s}", file=sys.stderr)
        return 1

    base_ctx = {
        "chain_id": chain_id,
        "private_key": sk,
        "account_index": account_index,
        "api_key_index": api_key_index,
    }

    vectors: list[dict] = []

    # CreateOrder: limit GTT, sell 0.1 ETH at 4050 USDC.
    create_fields = {
        "market_index": 0,
        "client_order_index": 123,
        "base_amount": 1_000,
        "price": 405_000,
        "is_ask": True,
        "order_type": 0,
        "time_in_force": 1,
        "reduce_only": False,
        "trigger_price": 0,
        "order_expiry": 1_735_689_600_000,
        "integrator_account_index": 0,
        "integrator_taker_fee": 0,
        "integrator_maker_fee": 0,
        "skip_nonce": 0,
    }
    ctx = {**base_ctx, "nonce": 0}
    vectors.append(
        build_vector("create_order", ctx, create_fields, gen_create_order(lib, ctx, create_fields)),
    )

    # CreateOrder with integrator attribution (non-empty L2TxAttributes).
    create_with_integrator = {
        **create_fields,
        "client_order_index": 124,
        "integrator_account_index": 723_813,
        "integrator_taker_fee": 250,
        "integrator_maker_fee": 100,
    }
    ctx = {**base_ctx, "nonce": 1}
    vectors.append(
        build_vector(
            "create_order",
            ctx,
            create_with_integrator,
            gen_create_order(lib, ctx, create_with_integrator),
        ),
    )

    # CancelOrder by client order index.
    cancel_fields = {
        "market_index": 0,
        "index": 123,
        "skip_nonce": 0,
    }
    ctx = {**base_ctx, "nonce": 2}
    vectors.append(
        build_vector("cancel_order", ctx, cancel_fields, gen_cancel_order(lib, ctx, cancel_fields)),
    )

    # ModifyOrder: bump size and price.
    modify_fields = {
        "market_index": 0,
        "index": 123,
        "base_amount": 1_100,
        "price": 410_000,
        "trigger_price": 0,
        "integrator_account_index": 0,
        "integrator_taker_fee": 0,
        "integrator_maker_fee": 0,
        "skip_nonce": 0,
    }
    ctx = {**base_ctx, "nonce": 3}
    vectors.append(
        build_vector("modify_order", ctx, modify_fields, gen_modify_order(lib, ctx, modify_fields)),
    )

    # ApproveIntegrator off the trading critical path.
    approve_fields = {
        "integrator_account_index": 723_813,
        "max_perps_taker_fee": 500,
        "max_perps_maker_fee": 200,
        "max_spot_taker_fee": 600,
        "max_spot_maker_fee": 300,
        "approval_expiry": 1_780_000_000_000,
        "skip_nonce": 0,
    }
    ctx = {**base_ctx, "nonce": 4}
    vectors.append(
        build_vector(
            "approve_integrator",
            ctx,
            approve_fields,
            gen_approve_integrator(lib, ctx, approve_fields),
        ),
    )

    # CreateOrder buy + reduce-only: covers is_ask=False and reduce_only=True
    # paths absent from the baseline vector.
    buy_reduce_only = {
        **create_fields,
        "client_order_index": 200,
        "is_ask": False,
        "reduce_only": True,
    }
    ctx = {**base_ctx, "nonce": 5}
    vectors.append(
        build_vector(
            "create_order",
            ctx,
            buy_reduce_only,
            gen_create_order(lib, ctx, buy_reduce_only),
        ),
    )

    # CancelOrder skip_nonce=1: exercises the attribute-aggregation branch on
    # the cancel hash and the {"4":1} JSON shape.
    cancel_skip_fields = {
        "market_index": 0,
        "index": 124,
        "skip_nonce": 1,
    }
    ctx = {**base_ctx, "nonce": 6}
    vectors.append(
        build_vector(
            "cancel_order",
            ctx,
            cancel_skip_fields,
            gen_cancel_order(lib, ctx, cancel_skip_fields),
        ),
    )

    # ApproveIntegrator skip_nonce=1: same gate, on the approve path.
    approve_skip = {
        **approve_fields,
        "skip_nonce": 1,
    }
    ctx = {**base_ctx, "nonce": 7}
    vectors.append(
        build_vector(
            "approve_integrator",
            ctx,
            approve_skip,
            gen_approve_integrator(lib, ctx, approve_skip),
        ),
    )

    # ModifyOrder with integrator attribution: exercises the aggregate hash
    # branch on modify (the baseline modify vector has all-zero integrator
    # slots, which short-circuits to the body digest).
    modify_with_integrator = {
        **modify_fields,
        "integrator_account_index": 723_813,
        "integrator_taker_fee": 250,
        "integrator_maker_fee": 100,
    }
    ctx = {**base_ctx, "nonce": 8}
    vectors.append(
        build_vector(
            "modify_order",
            ctx,
            modify_with_integrator,
            gen_modify_order(lib, ctx, modify_with_integrator),
        ),
    )

    payload = {
        "metadata": {
            "license": "Apache-2.0 (SDK repository; compiled signer binary)",
            "primitive": "lighter_l2_tx",
            "source": "github.com/elliottech/lighter-python",
            "note": (
                "Sig is non-deterministic (random k); tx_hash and tx_info "
                "carry deterministic byte equality targets."
            ),
        },
        "vectors": vectors,
    }

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2)
        f.write("\n")
    print(f"wrote {len(vectors)} vectors to {args.out}")

    if args.auth_out is not None:
        write_auth_oracle(lib, sk, chain_id, account_index, api_key_index, args.auth_out)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
