from __future__ import annotations

import argparse
import configparser
import json
import logging
import os
import signal
import time
from collections.abc import Sequence
from decimal import Decimal
from pathlib import Path
from typing import Any

from lp.config import LpHedgerConfig
from lp.config import load_lp_hedger_config
from lp.execution.bybit import BybitPerpClient
from lp.execution.bybit import MarketOrderRequest
from lp.hedgers.core import LpHedger
from lp.hedgers.registry import list_hedgers


logger = logging.getLogger(__name__)
LP_SYSTEM_CONFIG_ENV = "LP_SYSTEM_CONFIG"
DEFAULT_SYSTEM_CONFIG_PATH = Path("configs/config.ini")


def _read_config(path: Path) -> configparser.ConfigParser:
    parser = configparser.ConfigParser()
    if not parser.read(path):
        raise FileNotFoundError(path)
    return parser


def _read_config_optional(path: Path) -> configparser.ConfigParser:
    parser = configparser.ConfigParser()
    parser.read(path)
    return parser


def _infer_hedger_id(hedger_config_path: Path) -> str:
    parser = _read_config(hedger_config_path)
    if parser.has_section("identity"):
        hedger_id = parser.get("identity", "id", fallback="").strip()
        if hedger_id:
            return hedger_id
    return "eth_plume_lp"


def resolve_bybit_credentials(
    *,
    system_config_path: Path,
    hedger_config_path: Path,
    hedger_id: str | None = None,
) -> tuple[str, str]:
    hedger_parser = _read_config_optional(hedger_config_path)
    if hedger_parser.has_section("bybit"):
        api_key = hedger_parser.get("bybit", "api_key", fallback="").strip()
        secret = hedger_parser.get("bybit", "api_secret", fallback="").strip()
        if api_key and secret:
            return api_key, secret

    resolved_hedger_id = hedger_id or _infer_hedger_id(hedger_config_path)
    system_parser = _read_config(system_config_path)
    candidate_sections = ["bybit_hedger", "bybit"]
    if resolved_hedger_id == "eth_plume_lp_band2":
        candidate_sections.insert(0, "bybit_hedger_band2")

    for section in candidate_sections:
        if not system_parser.has_section(section):
            continue
        api_key = system_parser.get(section, "api_key", fallback="").strip()
        secret = system_parser.get(section, "secret", fallback="").strip()
        if api_key and secret:
            return api_key, secret

    raise ValueError(f"Bybit api_key/secret missing for {resolved_hedger_id}")


def get_redis_client(*, decode_responses: bool = True):
    import redis

    config_path = Path(os.getenv(LP_SYSTEM_CONFIG_ENV, str(DEFAULT_SYSTEM_CONFIG_PATH)))
    parser = _read_config_optional(config_path)
    env_url = os.getenv("REDIS_URL", "").strip()
    if env_url:
        return redis.from_url(env_url, decode_responses=decode_responses)

    if parser.has_section("redis"):
        redis_url = parser.get("redis", "url", fallback="").strip()
        if redis_url:
            return redis.from_url(redis_url, decode_responses=decode_responses)

        host = os.getenv("REDIS_HOST") or parser.get("redis", "host", fallback="127.0.0.1")
        port = int(os.getenv("REDIS_PORT") or parser.get("redis", "port", fallback="6379"))
        db = int(os.getenv("REDIS_DB") or parser.get("redis", "db", fallback="0"))
        username = os.getenv("REDIS_USERNAME") or parser.get("redis", "username", fallback="") or None
        password = os.getenv("REDIS_PASSWORD") or parser.get("redis", "password", fallback="") or None
        ssl = str(os.getenv("REDIS_SSL") or parser.get("redis", "ssl", fallback="0")).strip().lower() in {
            "1",
            "true",
            "yes",
            "on",
        }
        return redis.Redis(
            host=host,
            port=port,
            db=db,
            username=username,
            password=password,
            ssl=ssl,
            decode_responses=decode_responses,
        )

    return redis.Redis(host="127.0.0.1", port=6379, db=0, decode_responses=decode_responses)


class _OnchainPoolPriceHelper:
    _SLOT0_ABI = [
        {
            "inputs": [],
            "name": "slot0",
            "outputs": [
                {"name": "sqrtPriceX96", "type": "uint160"},
                {"name": "tick", "type": "int24"},
                {"name": "observationIndex", "type": "uint16"},
                {"name": "observationCardinality", "type": "uint16"},
                {"name": "observationCardinalityNext", "type": "uint16"},
                {"name": "feeProtocol", "type": "uint8"},
                {"name": "unlocked", "type": "bool"},
            ],
            "stateMutability": "view",
            "type": "function",
        },
        {
            "inputs": [],
            "name": "globalState",
            "outputs": [
                {"name": "price", "type": "uint160"},
                {"name": "tick", "type": "int24"},
                {"name": "fee", "type": "uint16"},
                {"name": "timepointIndex", "type": "uint16"},
                {"name": "communityFeeToken0", "type": "uint8"},
                {"name": "communityFeeToken1", "type": "uint8"},
                {"name": "unlocked", "type": "bool"},
            ],
            "stateMutability": "view",
            "type": "function",
        },
    ]

    def __init__(self, *, rpc_url: str, token0_decimals: int, token1_decimals: int) -> None:
        from web3 import Web3

        self._web3 = Web3(Web3.HTTPProvider(rpc_url))
        self._decimals_adjustment = Decimal(10) ** (token0_decimals - token1_decimals)
        self.last_source = "unknown"

    def get_price_token1_per_token0(self, pool_address: str) -> Decimal:
        contract = self._web3.eth.contract(
            address=self._web3.to_checksum_address(pool_address),
            abi=self._SLOT0_ABI,
        )
        sqrt_price_x96 = None
        for getter_name in ("slot0", "globalState"):
            getter = getattr(contract.functions, getter_name, None)
            if getter is None:
                continue
            try:
                result = getter().call()
            except Exception:
                logger.debug("Pool price getter failed", extra={"getter": getter_name}, exc_info=True)
                continue
            if isinstance(result, (list, tuple)) and result:
                sqrt_price_x96 = int(result[0])
                self.last_source = getter_name
                break
        if not sqrt_price_x96:
            raise RuntimeError(f"failed to read pool sqrt price for {pool_address}")
        ratio = (Decimal(sqrt_price_x96) * Decimal(sqrt_price_x96)) / Decimal(2**192)
        return ratio * self._decimals_adjustment


class _CcxtBybitExecutionClient:
    def __init__(self, client: Any) -> None:
        self._client = client

    def get_position_size(self, symbol: str) -> Decimal:
        normalized = symbol.replace("/", "").upper()
        try:
            positions = self._client.fetch_positions([symbol])
        except TypeError:
            positions = self._client.fetch_positions()
        for record in positions:
            raw_symbol = str(record.get("symbol") or record.get("info", {}).get("symbol") or "")
            candidate = raw_symbol.replace("/", "").upper()
            candidate_base = candidate.split(":", 1)[0]
            if candidate and candidate not in {normalized, candidate_base}:
                continue
            size = record.get("contracts") or record.get("size") or record.get("info", {}).get("size")
            if size is None:
                continue
            qty = Decimal(str(size))
            side = str(record.get("side") or record.get("info", {}).get("side") or "").lower()
            if side in {"short", "sell"}:
                return -abs(qty)
            if side in {"long", "buy"}:
                return abs(qty)
            return qty
        return Decimal(0)

    def get_mark_price(self, symbol: str) -> Decimal:
        ticker = self._client.fetch_ticker(symbol)
        candidates = (
            ticker.get("mark"),
            ticker.get("markPrice"),
            ticker.get("mark_price"),
            ticker.get("last"),
            ticker.get("info", {}).get("markPrice") if isinstance(ticker.get("info"), dict) else None,
        )
        for candidate in candidates:
            if candidate is not None:
                return Decimal(str(candidate))
        raise RuntimeError(f"mark price unavailable for {symbol}")

    def create_market_order(self, order: MarketOrderRequest) -> bool:
        try:
            self._client.create_market_order(order.symbol, order.side, float(order.qty))
        except TypeError:
            self._client.create_order(order.symbol, "market", order.side, float(order.qty))
        return True


class _DryRunExecutionClient:
    def __init__(self, delegate: _CcxtBybitExecutionClient) -> None:
        self._delegate = delegate

    def get_position_size(self, symbol: str) -> Decimal:
        return self._delegate.get_position_size(symbol)

    def get_mark_price(self, symbol: str) -> Decimal:
        return self._delegate.get_mark_price(symbol)

    def create_market_order(self, order: MarketOrderRequest) -> bool:
        logger.info(
            "DRY-RUN: would place Bybit market order",
            extra={"symbol": order.symbol, "side": order.side, "qty": str(order.qty)},
        )
        return True


class LpHedgerServiceRunner:
    def __init__(
        self,
        *,
        config_path: Path,
        system_config_path: Path,
        dry_run: bool,
    ) -> None:
        self.config_path = Path(config_path)
        self.system_config_path = Path(system_config_path)
        self.dry_run = bool(dry_run)

    def load_config(self) -> LpHedgerConfig:
        return load_lp_hedger_config(self.config_path)

    def build_redis_client(self):
        previous = os.getenv(LP_SYSTEM_CONFIG_ENV)
        os.environ[LP_SYSTEM_CONFIG_ENV] = str(self.system_config_path)
        try:
            return get_redis_client()
        finally:
            if previous is None:
                os.environ.pop(LP_SYSTEM_CONFIG_ENV, None)
            else:
                os.environ[LP_SYSTEM_CONFIG_ENV] = previous

    def build_price_helper(self, config: LpHedgerConfig) -> Any:
        if config.lp_mode != "onchain":
            return object()
        parser = _read_config(self.system_config_path)
        rpc_url = ""
        if parser.has_section("plume"):
            rpc_url = parser.get("plume", "rpc_url", fallback="").strip()
        if not rpc_url:
            raise ValueError("[plume].rpc_url missing in system config")
        return _OnchainPoolPriceHelper(
            rpc_url=rpc_url,
            token0_decimals=config.token0_decimals,
            token1_decimals=config.token1_decimals,
        )

    def build_bybit_client(self, config: LpHedgerConfig) -> BybitPerpClient:
        import ccxt

        api_key, secret = resolve_bybit_credentials(
            system_config_path=self.system_config_path,
            hedger_config_path=self.config_path,
            hedger_id=config.hedger_id,
        )
        ccxt_client = ccxt.bybit(  # type: ignore[attr-defined]
            {
                "apiKey": api_key,
                "secret": secret,
                "enableRateLimit": True,
                "options": {"defaultType": "linear"},
            },
        )
        execution_client = _CcxtBybitExecutionClient(ccxt_client)
        if self.dry_run:
            execution_client = _DryRunExecutionClient(execution_client)  # type: ignore[assignment]
        return BybitPerpClient(execution_client)

    def persist_mode(self, redis_client: Any, config: LpHedgerConfig) -> None:
        try:
            key = f"{config.state_key}:mode"
            payload = redis_client.get(key)
            enabled = False
            if payload:
                parsed = json.loads(payload if isinstance(payload, str) else payload.decode("utf-8"))
                if isinstance(parsed, dict):
                    enabled = bool(parsed.get("enabled", False))
            redis_client.set(key, json.dumps({"enabled": enabled, "dry_run": self.dry_run}))
        except Exception:
            logger.warning("Failed to persist LP hedger mode", exc_info=True)

    def build_hedger(self) -> LpHedger:
        config = self.load_config()
        redis_client = self.build_redis_client()
        self.persist_mode(redis_client, config)
        return LpHedger(
            config=config,
            price_helper=self.build_price_helper(config),
            bybit_client=self.build_bybit_client(config),
            redis_client=redis_client,
        )

    def run(self) -> None:
        hedger = self.build_hedger()
        poll_interval = max(1, int(hedger.config.poll_interval_sec))
        stopped = False

        def _stop(*_args: Any) -> None:
            nonlocal stopped
            stopped = True

        signal.signal(signal.SIGINT, _stop)
        signal.signal(signal.SIGTERM, _stop)

        while not stopped:
            try:
                hedger.tick()
            except Exception:
                logger.exception("LP hedger tick failed")
            time.sleep(poll_interval)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the LP hedger service")
    parser.add_argument("--config", "--config-path", dest="config_path", type=Path, required=False)
    parser.add_argument(
        "--system-config",
        type=Path,
        default=DEFAULT_SYSTEM_CONFIG_PATH,
        help="Path to shared system config.ini for Redis/RPC/credentials",
    )
    parser.add_argument("--log-level", default="INFO")
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args(argv)


def resolve_config_path(args: argparse.Namespace) -> Path:
    cli_path = getattr(args, "config_path", None)
    if cli_path is not None:
        return Path(cli_path)

    configured_paths: list[tuple[str, Path]] = []
    for meta in list_hedgers():
        env_value = os.getenv(meta.config_env_var, "").strip()
        if env_value:
            configured_paths.append((meta.config_env_var, Path(env_value)))

    if len(configured_paths) == 1:
        return configured_paths[0][1]
    if len(configured_paths) > 1:
        configured_names = ", ".join(name for name, _ in configured_paths)
        raise ValueError(
            "Multiple LP hedger config env vars are set; pass --config explicitly: "
            f"{configured_names}"
        )
    raise ValueError(
        "Missing LP hedger config path; pass --config/--config-path or set one LP hedger config env var"
    )


def main(argv: Sequence[str] | None = None) -> None:
    args = parse_args(argv)
    logging.basicConfig(
        level=getattr(logging, str(args.log_level).upper(), logging.INFO),
        format="%(asctime)s %(levelname)s [%(name)s] %(message)s",
    )
    runner = LpHedgerServiceRunner(
        config_path=resolve_config_path(args),
        system_config_path=args.system_config,
        dry_run=args.dry_run,
    )
    runner.run()


__all__ = [
    "DEFAULT_SYSTEM_CONFIG_PATH",
    "LP_SYSTEM_CONFIG_ENV",
    "LpHedgerServiceRunner",
    "get_redis_client",
    "main",
    "parse_args",
    "resolve_bybit_credentials",
    "resolve_config_path",
]


if __name__ == "__main__":
    main()
