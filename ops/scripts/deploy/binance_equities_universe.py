#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path
import tomllib
from typing import Any
from urllib.request import urlopen


DEFAULT_EXCHANGE_INFO_URL = "https://fapi.binance.com/fapi/v1/exchangeInfo"


@dataclass(frozen=True, slots=True)
class BinanceEquityPerpContract:
    symbol: str
    status: str
    contract_type: str
    underlying_type: str
    base_asset: str
    quote_asset: str


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _text(value: Any) -> str:
    return str(value or "").strip().upper()


def load_binance_equity_perps(payload: dict[str, Any]) -> tuple[BinanceEquityPerpContract, ...]:
    contracts: list[BinanceEquityPerpContract] = []
    for row in payload.get("symbols", []):
        if not isinstance(row, dict):
            continue
        symbol = _text(row.get("symbol"))
        contract_type = _text(row.get("contractType"))
        underlying_type = _text(row.get("underlyingType"))
        if not symbol or contract_type != "TRADIFI_PERPETUAL" or underlying_type != "EQUITY":
            continue
        contracts.append(
            BinanceEquityPerpContract(
                symbol=symbol,
                status=_text(row.get("status")),
                contract_type=contract_type,
                underlying_type=underlying_type,
                base_asset=_text(row.get("baseAsset")),
                quote_asset=_text(row.get("quoteAsset")),
            )
        )
    return tuple(sorted(contracts, key=lambda contract: contract.symbol))


def active_binance_equity_perp_symbols(
    contracts: tuple[BinanceEquityPerpContract, ...] | list[BinanceEquityPerpContract],
) -> tuple[str, ...]:
    return tuple(
        contract.symbol
        for contract in contracts
        if _text(contract.status) == "TRADING"
    )


def discover_active_equity_perps(payload: dict[str, Any]) -> tuple[BinanceEquityPerpContract, ...]:
    return tuple(
        contract for contract in load_binance_equity_perps(payload) if _text(contract.status) == "TRADING"
    )


def enrolled_binance_equity_symbols(config: dict[str, Any]) -> tuple[str, ...]:
    api_cfg = config.get("api")
    raw_strategy_ids = api_cfg.get("equities_strategy_ids", []) if isinstance(api_cfg, dict) else []
    if not raw_strategy_ids:
        return ()
    enrolled_strategy_ids = {
        _text(strategy_id).lower()
        for strategy_id in raw_strategy_ids
    }
    enrolled = {
        _text(row.get("maker_symbol"))
        for row in config.get("strategy_contracts", [])
        if (
            isinstance(row, dict)
            and _text(row.get("maker_venue")) == "BINANCE_PERP"
            and _text(row.get("strategy_id")).lower() in enrolled_strategy_ids
        )
    }
    enrolled.discard("")
    return tuple(sorted(enrolled))


def fetch_exchange_info(*, exchange_info_url: str, timeout_seconds: float) -> dict[str, Any]:
    with urlopen(exchange_info_url, timeout=timeout_seconds) as response:  # noqa: S310
        return json.load(response)


def _load_manifest(path: Path) -> dict[str, Any]:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Show the live Binance USD-M equity-perp universe and enrolled equities routes.",
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=_repo_root() / "deploy/equities/equities.live.toml",
        help="Equities manifest to diff against enrolled BINANCE_PERP routes.",
    )
    parser.add_argument(
        "--exchange-info-url",
        default=DEFAULT_EXCHANGE_INFO_URL,
        help="Binance USD-M exchangeInfo endpoint.",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=float,
        default=10.0,
        help="HTTP timeout for the exchangeInfo request.",
    )
    parser.add_argument(
        "--show-diff",
        action="store_true",
        help="Deprecated no-op; the discovery diff is always printed.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    payload = fetch_exchange_info(
        exchange_info_url=str(args.exchange_info_url),
        timeout_seconds=float(args.timeout_seconds),
    )
    active_contracts = discover_active_equity_perps(payload)
    active_symbols = tuple(contract.symbol for contract in active_contracts)

    print(f"Discovered active Binance equity perps ({len(active_contracts)}):")
    for contract in active_contracts:
        print(f"- {contract.symbol} -> {contract.base_asset}")

    if not args.config:
        return 0

    config = _load_manifest(args.config)
    enrolled_symbols = enrolled_binance_equity_symbols(config)
    missing_symbols = tuple(symbol for symbol in active_symbols if symbol not in enrolled_symbols)
    stale_enrollment = tuple(symbol for symbol in enrolled_symbols if symbol not in active_symbols)

    print(f"Discovered but not enrolled ({len(missing_symbols)}):")
    for symbol in missing_symbols:
        print(f"- {symbol}")

    print(f"Enrolled but not currently live on Binance ({len(stale_enrollment)}):")
    for symbol in stale_enrollment:
        print(f"- {symbol}")

    print("This report is read-only and does not modify strategy rows or allowlists.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
