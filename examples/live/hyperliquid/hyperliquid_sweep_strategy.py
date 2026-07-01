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

import argparse
import json
import os
import sys
from dataclasses import replace
from decimal import Decimal
from pathlib import Path

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment
from nautilus_trader.core.nautilus_pyo3 import hyperliquid_resolve_execution_account_address
from nautilus_trader.datadog import DatadogTelemetryConfig
from nautilus_trader.datadog import configure as configure_datadog
from nautilus_trader.datadog import stop as stop_datadog
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Environment variables required:
# Mainnet: HYPERLIQUID_PK (and optionally HYPERLIQUID_VAULT)
# Testnet: HYPERLIQUID_TESTNET_PK (and optionally HYPERLIQUID_TESTNET_VAULT)
#
# Agent / API wallets: if your private key is an agent wallet approved under a
# master account (typical when you create an API wallet on the Hyperliquid UI),
# also set HYPERLIQUID_ACCOUNT_ADDRESS to the master account address.
#
# Vault/sub-account trading: set HYPERLIQUID_VAULT or HYPERLIQUID_TESTNET_VAULT.
# That value is sent as `vaultAddress` in signed exchange payloads.

STRATEGY_DIR = Path(__file__).resolve().parents[3] / "nautilus_trader" / "examples" / "strategies"
sys.path.insert(0, str(STRATEGY_DIR))

SWEEP_STRATEGY_PATH = "sweep_strategy:SweepStrategy"
SWEEP_CONFIG_PATH = "sweep_strategy:SweepStrategyConfig"
DEFAULT_DATADOG_TAGS = (
    "env:dev",
    "service:nautilus",
    "component:hyperliquid-sweep",
)


def env_bool(name: str, default: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.lower() in {"1", "true", "yes", "y", "on"}


def env_float(name: str, default: float, *fallback_names: str) -> float:
    value = os.getenv(name)
    if value is None:
        for fallback_name in fallback_names:
            value = os.getenv(fallback_name)
            if value is not None:
                break
    return float(value if value is not None else str(default))


def env_decimal(name: str, default: str) -> Decimal:
    return Decimal(os.getenv(name, default))


def bool_value(value: object, default: bool) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    return str(value).lower() in {"1", "true", "yes", "y", "on"}


def hyperliquid_credential_env_names(testnet: bool) -> tuple[str, str]:
    if testnet:
        return "HYPERLIQUID_TESTNET_PK", "HYPERLIQUID_TESTNET_VAULT"
    return "HYPERLIQUID_PK", "HYPERLIQUID_VAULT"


def parse_environment(value: str | None) -> HyperliquidEnvironment:
    if value is None:
        return (
            HyperliquidEnvironment.TESTNET
            if env_bool("HYPERLIQUID_TESTNET", False)
            else HyperliquidEnvironment.MAINNET
        )
    normalized = value.strip().upper()
    if normalized == "TESTNET":
        return HyperliquidEnvironment.TESTNET
    if normalized == "MAINNET":
        return HyperliquidEnvironment.MAINNET
    raise ValueError(f"Unsupported Hyperliquid environment: {value!r}")


def parse_product_types(values: list[str] | None) -> tuple[HyperliquidProductType, ...] | None:
    if values is None:
        return None
    return tuple(HyperliquidProductType(value.lower()) for value in values)


def env_or_value(config: dict, key: str) -> str | None:
    env_name = config.get(f"{key}_env")
    if env_name:
        return os.getenv(env_name)
    return config.get(key)


def default_env_config() -> dict:
    testnet = env_bool("HYPERLIQUID_TESTNET", False)
    private_key_env, vault_address_env = hyperliquid_credential_env_names(testnet)
    symbol = os.getenv("HYPERLIQUID_SWEEP_SYMBOL", "BTC-USD-PERP")
    instrument_id = f"{symbol}.{HYPERLIQUID}"
    return {
        "environment": "TESTNET" if testnet else "MAINNET",
        "trader_id": "HYPERLIQUID-SWEEP-001",
        "logging": {
            "log_level": "INFO",
            "use_pyo3": True,
        },
        "hyperliquid": {
            "private_key_env": private_key_env,
            "vault_address_env": vault_address_env,
            "account_address_env": "HYPERLIQUID_ACCOUNT_ADDRESS",
            "normalize_prices": True,
            "include_builder_attribution": True,
            "bbo_redundancy": int(os.getenv("HYPERLIQUID_BBO_REDUNDANCY", "4")),
        },
        "datadog": {
            "enabled": env_bool("NAUTILUS_DATADOG_ENABLED", False),
            "tags": DEFAULT_DATADOG_TAGS,
        },
        "strategies": [
            {
                "instrument_id": instrument_id,
                "external_order_claims": [instrument_id],
                "order_qty": str(env_decimal("HYPERLIQUID_SWEEP_ORDER_QTY", "0.001")),
                "quote_offset_bps": env_float("HYPERLIQUID_SWEEP_QUOTE_OFFSET_BPS", 10.0),
                "quote_recenter_threshold_bps": env_float(
                    "HYPERLIQUID_SWEEP_QUOTE_RECENTER_THRESHOLD_BPS",
                    3.0,
                    "HYPERLIQUID_SWEEP_RECENTER_THRESHOLD_BPS",
                ),
                "unwind_recenter_threshold_bps": env_float(
                    "HYPERLIQUID_SWEEP_UNWIND_RECENTER_THRESHOLD_BPS",
                    0.0,
                ),
                "unwind_cross_touch": env_bool(
                    "HYPERLIQUID_SWEEP_UNWIND_CROSS_TOUCH",
                    False,
                ),
                "market_open_embargo_minutes": env_float(
                    "HYPERLIQUID_SWEEP_MARKET_OPEN_EMBARGO_MINUTES",
                    0.0,
                ),
                "market_open_embargo_pre_open_minutes": env_float(
                    "HYPERLIQUID_SWEEP_MARKET_OPEN_EMBARGO_PRE_OPEN_MINUTES",
                    0.0,
                ),
                "market_open_embargo_timezone": os.getenv(
                    "HYPERLIQUID_SWEEP_MARKET_OPEN_EMBARGO_TIMEZONE",
                    "America/New_York",
                ),
                "market_open_embargo_start": os.getenv(
                    "HYPERLIQUID_SWEEP_MARKET_OPEN_EMBARGO_START",
                    "09:30:00",
                ),
                "market_after_hours_embargo_minutes": env_float(
                    "HYPERLIQUID_SWEEP_MARKET_AFTER_HOURS_EMBARGO_MINUTES",
                    0.0,
                ),
                "market_after_hours_embargo_pre_start_minutes": env_float(
                    "HYPERLIQUID_SWEEP_MARKET_AFTER_HOURS_EMBARGO_PRE_START_MINUTES",
                    0.0,
                ),
                "market_after_hours_embargo_start": os.getenv(
                    "HYPERLIQUID_SWEEP_MARKET_AFTER_HOURS_EMBARGO_START",
                    "16:00:00",
                ),
                "close_positions_on_embargo": env_bool(
                    "HYPERLIQUID_SWEEP_CLOSE_POSITIONS_ON_EMBARGO",
                    False,
                ),
                "reduce_only_on_embargo": env_bool(
                    "HYPERLIQUID_SWEEP_REDUCE_ONLY_ON_EMBARGO",
                    True,
                ),
                "close_positions_on_stop": True,
                "reduce_only_on_stop": True,
                "log_data": env_bool("HYPERLIQUID_SWEEP_LOG_DATA", False),
            },
        ],
    }


def load_config(path: Path | None) -> dict:
    if path is None:
        return default_env_config()
    with path.open() as f:
        return json.load(f)


def datadog_enabled(config: dict) -> bool:
    env_enabled = os.getenv("NAUTILUS_DATADOG_ENABLED")
    if env_enabled is not None:
        return env_bool("NAUTILUS_DATADOG_ENABLED", False)
    return bool_value(config.get("datadog", {}).get("enabled"), False)


def parse_datadog_tags(raw_tags: object) -> tuple[str, ...]:
    if raw_tags is None:
        return ()
    if isinstance(raw_tags, str):
        normalized = raw_tags.replace(",", " ")
        return tuple(tag for tag in normalized.split() if tag)
    return tuple(str(tag) for tag in raw_tags if tag)


def merge_datadog_tags(*tag_groups: tuple[str, ...]) -> tuple[str, ...]:
    tags: list[str] = []
    positions: dict[str, int] = {}
    free_tags: set[str] = set()

    for group in tag_groups:
        for tag in group:
            if ":" in tag:
                prefix = tag.split(":", 1)[0]
                if prefix in positions:
                    tags[positions[prefix]] = tag
                else:
                    positions[prefix] = len(tags)
                    tags.append(tag)
            elif tag not in free_tags:
                free_tags.add(tag)
                tags.append(tag)

    return tuple(tags)


def configure_sweep_datadog(config: dict) -> None:
    if not datadog_enabled(config):
        return

    config_tags = parse_datadog_tags(config.get("datadog", {}).get("tags"))
    telemetry_config = DatadogTelemetryConfig.from_env(enabled=True)
    telemetry_config = replace(
        telemetry_config,
        constant_tags=merge_datadog_tags(
            DEFAULT_DATADOG_TAGS,
            telemetry_config.constant_tags,
            config_tags,
        ),
    )
    configure_datadog(telemetry_config)
    print(
        "Datadog telemetry enabled: "
        f"dogstatsd={telemetry_config.host}:{telemetry_config.port}, "
        f"tags={','.join(telemetry_config.constant_tags)}",
        flush=True,
    )


def strategy_entries(config: dict) -> list[dict]:
    entries = config.get("strategies", [])
    if not entries:
        raise ValueError("Config must contain at least one strategy entry")
    return [entry for entry in entries if entry.get("enabled", True)]


def instrument_ids_from_strategies(entries: list[dict]) -> list[str]:
    instrument_ids: list[str] = []
    for entry in entries:
        strategy_config = entry.get("config", entry)
        instrument_id = strategy_config.get("instrument_id")
        if instrument_id is not None:
            instrument_ids.append(instrument_id)
    return sorted(set(instrument_ids))


def instrument_provider_config(config: dict, entries: list[dict]) -> InstrumentProviderConfig:
    provider = config.get("instrument_provider")
    if provider is None:
        provider = {
            "load_all": False,
            "load_ids": instrument_ids_from_strategies(entries),
        }
    return InstrumentProviderConfig.parse(json.dumps(provider))


def logging_config(config: dict) -> LoggingConfig:
    return LoggingConfig.parse(
        json.dumps(
            {
                "log_level": "INFO",
                "use_pyo3": True,
                **config.get("logging", {}),
            },
        ),
    )


def exec_engine_config(config: dict) -> LiveExecEngineConfig:
    return LiveExecEngineConfig.parse(
        json.dumps(
            {
                "reconciliation": True,
                "reconciliation_lookback_mins": 1440,
                "open_check_interval_secs": 15.0,
                "open_check_threshold_ms": 10_000,
                "open_check_open_only": False,
                "open_check_lookback_mins": 60,
                "graceful_shutdown_on_exception": True,
                **config.get("exec_engine", {}),
            },
        ),
    )


def hyperliquid_client_configs(
    config: dict,
    provider: InstrumentProviderConfig,
) -> tuple[HyperliquidDataClientConfig, HyperliquidExecClientConfig]:
    hyperliquid = config.get("hyperliquid", {})
    environment = parse_environment(hyperliquid.get("environment", config.get("environment")))
    product_types = parse_product_types(hyperliquid.get("product_types"))

    data_config = HyperliquidDataClientConfig(
        environment=environment,
        instrument_provider=provider,
        product_types=product_types,
        base_url_ws=hyperliquid.get("base_url_ws"),
        proxy_url=hyperliquid.get("proxy_url"),
        http_timeout_secs=hyperliquid.get("http_timeout_secs", 10),
        bbo_redundancy=hyperliquid.get("bbo_redundancy", 4),
    )
    exec_config = HyperliquidExecClientConfig(
        private_key=env_or_value(hyperliquid, "private_key"),
        vault_address=env_or_value(hyperliquid, "vault_address"),
        account_address=env_or_value(hyperliquid, "account_address"),
        environment=environment,
        instrument_provider=provider,
        product_types=product_types,
        base_url_ws=hyperliquid.get("base_url_ws"),
        proxy_url=hyperliquid.get("proxy_url"),
        max_retries=hyperliquid.get("max_retries"),
        retry_delay_initial_ms=hyperliquid.get("retry_delay_initial_ms"),
        retry_delay_max_ms=hyperliquid.get("retry_delay_max_ms"),
        http_timeout_secs=hyperliquid.get("http_timeout_secs", 10),
        ws_post_timeout_secs=hyperliquid.get("ws_post_timeout_secs", 10),
        normalize_prices=hyperliquid.get("normalize_prices", True),
        include_builder_attribution=hyperliquid.get("include_builder_attribution", True),
    )
    return data_config, exec_config


def log_hyperliquid_routes(exec_config: HyperliquidExecClientConfig) -> None:
    account_address = hyperliquid_resolve_execution_account_address(
        private_key=exec_config.private_key,
        vault_address=exec_config.vault_address,
        account_address=exec_config.account_address,
        environment=exec_config.environment,
    )
    print(
        "Hyperliquid account address (REST/WS): "
        f"{account_address if account_address else '<unresolved>'}",
        flush=True,
    )
    print(
        "Hyperliquid vaultAddress (signed exchange payload): "
        f"{exec_config.vault_address if exec_config.vault_address else '<none>'}",
        flush=True,
    )


def importable_strategy_config(entry: dict) -> ImportableStrategyConfig:
    if "strategy_path" in entry:
        return ImportableStrategyConfig(
            strategy_path=entry["strategy_path"],
            config_path=entry["config_path"],
            config=entry["config"],
        )

    config = {
        key: value
        for key, value in entry.items()
        if key not in {"enabled", "strategy_path", "config_path", "config"}
    }
    config.setdefault("external_order_claims", [config["instrument_id"]])
    return ImportableStrategyConfig(
        strategy_path=SWEEP_STRATEGY_PATH,
        config_path=SWEEP_CONFIG_PATH,
        config=config,
    )


def build_node(config: dict) -> TradingNode:
    entries = strategy_entries(config)
    provider = instrument_provider_config(config, entries)
    data_config, exec_config = hyperliquid_client_configs(config, provider)
    log_hyperliquid_routes(exec_config)

    config_node = TradingNodeConfig(
        trader_id=TraderId(config.get("trader_id", "HYPERLIQUID-SWEEP-001")),
        logging=logging_config(config),
        exec_engine=exec_engine_config(config),
        strategies=[importable_strategy_config(entry) for entry in entries],
        data_clients={HYPERLIQUID: data_config},
        exec_clients={HYPERLIQUID: exec_config},
        timeout_connection=config.get("timeout_connection", 30.0),
        timeout_reconciliation=config.get("timeout_reconciliation", 10.0),
        timeout_portfolio=config.get("timeout_portfolio", 10.0),
        timeout_disconnection=config.get("timeout_disconnection", 10.0),
        timeout_post_stop=config.get("timeout_post_stop", 10.0),
    )

    node = TradingNode(config=config_node)
    node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
    node.add_exec_client_factory(HYPERLIQUID, HyperliquidLiveExecClientFactory)
    node.build()
    return node


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run one or more Hyperliquid SweepStrategy instances.",
    )
    config_path = os.getenv("HYPERLIQUID_SWEEP_CONFIG")
    parser.add_argument(
        "--config",
        type=Path,
        default=Path(config_path) if config_path else None,
        help="Path to a JSON basket config. If omitted, HYPERLIQUID_SWEEP_* env vars are used.",
    )
    return parser.parse_args()


if __name__ == "__main__":
    args = parse_args()
    config = load_config(args.config)
    configure_sweep_datadog(config)
    node: TradingNode | None = None
    try:
        node = build_node(config)
        node.run()
    finally:
        if node is not None:
            node.dispose()
        stop_datadog()
