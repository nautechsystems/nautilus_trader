#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import importlib
import json
import os
from decimal import Decimal, InvalidOperation
from pathlib import Path
import sys


PRICE_PER_REQUEST_USDC = Decimal("0.0005")
REPO_ROOT = Path(__file__).resolve().parents[3]


def _import_hyperliquid_http_client():
    root_text = str(REPO_ROOT)
    if root_text not in sys.path:
        sys.path.insert(0, root_text)
    module = importlib.import_module("nautilus_trader.core.nautilus_pyo3")
    return module.HyperliquidHttpClient


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Inspect and optionally reserve Hyperliquid address request quota.",
    )
    parser.add_argument(
        "--common-env",
        type=Path,
        default=Path("/etc/flux/common.env"),
        help="Path to the shared Flux common env file.",
    )
    parser.add_argument(
        "--dex",
        default="xyz",
        help="Optional Hyperliquid perp DEX scope to attach to the client.",
    )
    parser.add_argument(
        "--amount-usdc",
        type=Decimal,
        default=None,
        help="USDC budget to spend on request quota. $10 buys 20,000 requests.",
    )
    parser.add_argument(
        "--weight",
        type=int,
        default=None,
        help="Reserve this exact number of requests instead of deriving from --amount-usdc.",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="Actually submit the reserveRequestWeight action.",
    )
    parser.add_argument(
        "--show-only",
        action="store_true",
        help="Only show current quota state; do not reserve anything.",
    )
    return parser.parse_args()


def _parse_env_file(path: Path) -> dict[str, str]:
    if not path.is_file():
        raise FileNotFoundError(f"common env not found: {path}")

    values: dict[str, str] = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export ") :].strip()
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        values[key] = value
    return values


def _load_env_values(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    file_error: Exception | None = None
    try:
        values.update(_parse_env_file(path))
    except (FileNotFoundError, PermissionError) as exc:
        file_error = exc

    for key in (
        "TRADE_XYZ_AGENT_PK",
        "TRADE_XYZ_ACCOUNT_ADDRESS",
        "TRADE_XYZ_VAULT_ADDRESS",
    ):
        env_value = str(os.environ.get(key, "")).strip()
        if env_value:
            values[key] = env_value

    if values:
        return values
    if file_error is not None:
        raise file_error
    return values


def _required_env(values: dict[str, str], key: str) -> str:
    value = values.get(key, "").strip()
    if not value:
        raise ValueError(f"missing required env var {key}")
    return value


def _weight_from_budget(amount_usdc: Decimal) -> int:
    if amount_usdc <= 0:
        raise ValueError("amount-usdc must be positive")
    requests = amount_usdc / PRICE_PER_REQUEST_USDC
    if requests != requests.to_integral_value():
        raise ValueError(
            f"amount-usdc must be a multiple of {PRICE_PER_REQUEST_USDC} to map to an integer request count",
        )
    return int(requests)


async def _query_rate_limit(client: object) -> dict[str, object]:
    raw = await client.info_user_rate_limit()
    if isinstance(raw, str):
        return json.loads(raw)
    if isinstance(raw, bytes):
        return json.loads(raw.decode("utf-8"))
    raise TypeError(f"unexpected userRateLimit payload type: {type(raw).__name__}")


def _format_snapshot(snapshot: dict[str, object]) -> str:
    return (
        f"cumVlm={snapshot.get('cumVlm')} "
        f"nRequestsUsed={snapshot.get('nRequestsUsed')} "
        f"nRequestsCap={snapshot.get('nRequestsCap')} "
        f"nRequestsSurplus={snapshot.get('nRequestsSurplus')}"
    )


async def _main() -> int:
    args = _parse_args()
    if args.weight is not None and args.amount_usdc is not None:
        raise ValueError("use either --amount-usdc or --weight, not both")

    env_values = _load_env_values(args.common_env)
    private_key = _required_env(env_values, "TRADE_XYZ_AGENT_PK")
    account_address = _required_env(env_values, "TRADE_XYZ_ACCOUNT_ADDRESS")
    vault_address = env_values.get("TRADE_XYZ_VAULT_ADDRESS") or None

    hyperliquid_http_client = _import_hyperliquid_http_client()
    client = hyperliquid_http_client(
        private_key=private_key,
        vault_address=vault_address,
        is_testnet=False,
        timeout_secs=10,
        proxy_url=None,
        normalize_prices=True,
        dex=args.dex or None,
        account_address=account_address,
    )

    before = await _query_rate_limit(client)
    print(f"[hyperliquid-quota] before {_format_snapshot(before)}")

    if args.show_only:
        return 0

    weight = args.weight
    if weight is None and args.amount_usdc is not None:
        weight = _weight_from_budget(args.amount_usdc)

    if weight is None:
        return 0

    spend = (Decimal(weight) * PRICE_PER_REQUEST_USDC).quantize(Decimal("0.0001"))
    print(f"[hyperliquid-quota] reserve_request_weight weight={weight} spend_usdc={spend}")

    if not args.yes:
        print("[hyperliquid-quota] dry-run only; pass --yes to submit")
        return 0

    await client.reserve_request_weight(weight)
    after = await _query_rate_limit(client)
    print(f"[hyperliquid-quota] after {_format_snapshot(after)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(asyncio.run(_main()))
    except (FileNotFoundError, InvalidOperation, TypeError, ValueError) as exc:
        raise SystemExit(f"[hyperliquid-quota] error: {exc}") from exc
