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
from pathlib import Path

from _common import add_strategy_from_config
from _common import env_bool
from _common import env_int
from _common import ib_account_id
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop

from nautilus_trader.core import nautilus_pyo3 as pyo3


def default_publishers_filepath() -> str:
    return str(
        Path(__file__).resolve().parents[3]
        / "nautilus_trader"
        / "adapters"
        / "databento"
        / "publishers.json",
    )


def main() -> None:
    host, port = resolve_ib_endpoint()
    trader_id = pyo3.TraderId.from_str("IB-V2-DATABENTO-001")
    account_id = os.getenv("TWS_ACCOUNT") if env_bool("IB_V2_ENABLE_EXECUTION") else None
    provider_config = instrument_provider_config(
        load_ids=[
            "SPY.XNAS",
            "AAPL.XNAS",
            "V.XNYS",
            "CLM6.XNYM",
            "ESM6.XCME",
        ],
    )

    builder = pyo3.live.LiveNode.builder(  # type: ignore[attr-defined]
        "IB-V2-DATABENTO-001",
        trader_id,
        pyo3.Environment.LIVE,
    )
    builder = builder.with_timeout_connection(env_int("IB_V2_NODE_CONNECTION_TIMEOUT", 15))
    builder = builder.with_reconciliation(False)
    builder = builder.add_data_client(
        "DATABENTO",
        pyo3.DatabentoDataClientFactory(),  # type: ignore[attr-defined]
        pyo3.DatabentoLiveClientConfig(  # type: ignore[attr-defined]
            api_key=os.getenv("DATABENTO_API_KEY", "0" * 32),
            publishers_filepath=os.getenv(
                "DATABENTO_PUBLISHERS_FILE",
                default_publishers_filepath(),
            ),
        ),
    )

    if account_id is not None:
        ib = pyo3.interactive_brokers
        builder = builder.add_exec_client(
            None,
            ib.InteractiveBrokersExecutionClientFactory(trader_id, ib_account_id(account_id)),
            ib.InteractiveBrokersExecClientConfig(
                host=host,
                port=port,
                client_id=env_int("IB_V2_EXEC_CLIENT_ID", 1312),
                account_id=account_id,
                connection_timeout=env_int("IB_V2_CONNECTION_TIMEOUT", 10),
                request_timeout=env_int("IB_V2_REQUEST_TIMEOUT", 30),
                fetch_all_open_orders=False,
                instrument_provider=provider_config,
            ),
        )

    node = builder.build()
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:DatabentoSubscriptionStrategy",
    )
    print(f"Built Databento data + IB execution v2 node: {node.trader_id}", flush=True)
    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 0))
        node.run()


if __name__ == "__main__":
    main()
