#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import is_ib_endpoint_reachable
from _common import resolve_ib_endpoint
from _common import schedule_node_stop

from nautilus_trader.core import nautilus_pyo3 as pyo3


def main() -> None:
    ib = pyo3.interactive_brokers
    host, port = resolve_ib_endpoint()
    gateway = None

    if not is_ib_endpoint_reachable(host, port) or env_bool("IB_V2_FORCE_DOCKERIZED_GATEWAY"):
        username = os.getenv("TWS_USERNAME")
        password = os.getenv("TWS_PASSWORD")
        if not username or not password:
            raise RuntimeError(
                "TWS_USERNAME and TWS_PASSWORD are required to start Dockerized IB Gateway",
            )

        config = ib.DockerizedIBGatewayConfig(
            username=username,
            password=password,
            read_only_api=not env_bool("IB_V2_ENABLE_EXECUTION"),
            timeout=env_int("IB_V2_DOCKER_TIMEOUT", 300),
        )
        gateway = ib.DockerizedIBGateway(config)
        gateway.safe_start_blocking(config.timeout)
        host = gateway.host
        port = gateway.port

    account_id = os.getenv("TWS_ACCOUNT") if env_bool("IB_V2_ENABLE_EXECUTION") else None
    os.environ.setdefault("IB_V2_SUBSCRIPTION_INSTRUMENT_ID", "EUR/USD.IDEALPRO")
    os.environ.setdefault("IB_V2_SUBSCRIBE_QUOTES", "1")
    provider_config = instrument_provider_config(
        load_ids=[
            "EUR/USD.IDEALPRO",
            "BTC/USD.PAXOS",
            "SPY.ARCA",
            "V.NYSE",
            "YMM6.CBOT",
            "CLM6.NYMEX",
            "ESM6.CME",
        ],
        load_contracts=[
            {
                "secType": ib.IbSecurityType.STOCK.as_str(),
                "symbol": "SPY",
                "exchange": "SMART",
                "primaryExchange": "ARCA",
                "build_options_chain": True,
                "min_expiry_days": 7,
                "max_expiry_days": 14,
            },
            {
                "secType": ib.IbSecurityType.CONTINUOUS_FUTURE.as_str(),
                "exchange": "CME",
                "symbol": "ES",
                "build_futures_chain": True,
            },
            {
                "secType": ib.IbSecurityType.FUTURE.as_str(),
                "exchange": "NYMEX",
                "localSymbol": "CLM6",
                "build_futures_chain": False,
            },
        ],
    )
    node = build_ib_live_node(
        name="IB-V2-DOCKER-001",
        trader_id="IB-V2-DOCKER-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 101),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 102),
        account_id=account_id,
        provider_config=provider_config,
    )
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:IbV2SubscriptionStrategy",
    )

    print(f"Running v2 IB node against {host}:{port}; press Ctrl+C to stop.", flush=True)
    schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 0))
    try:
        node.run()
    finally:
        if gateway is not None:
            gateway.stop_blocking()


if __name__ == "__main__":
    main()
