#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os
import sys
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import option_contract
from _common import resolve_ib_endpoint
from _common import schedule_node_stop

from nautilus_trader.core import nautilus_pyo3 as pyo3


def main() -> None:
    ib = pyo3.interactive_brokers
    host, port = resolve_ib_endpoint()
    leg_contracts = [
        option_contract(local_symbol="ESM6 P6800", right=ib.IbOptionRight.PUT, strike=6800.0),
        option_contract(local_symbol="ESM6 P6775", right=ib.IbOptionRight.PUT, strike=6775.0),
    ]
    provider_config = instrument_provider_config(
        load_contracts=leg_contracts,
        symbol_to_mic_venue={"ES": "IB"},
    )
    account_id = (
        os.getenv("TWS_ACCOUNT")
        if env_bool("IB_V2_ENABLE_EXECUTION") or env_bool("IB_V2_ENABLE_ORDER_SUBMISSION")
        else None
    )
    node = build_ib_live_node(
        name="IB-V2-SPREAD-001",
        trader_id="IB-V2-SPREAD-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 111),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 112),
        account_id=account_id,
        provider_config=provider_config,
    )
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:SpreadOrderStrategy",
    )

    print(
        f"Built v2 spread node and registered SpreadOrderStrategy with {len(leg_contracts)} option legs.",
        flush=True,
    )
    print(
        "Set IB_V2_ENABLE_ORDER_SUBMISSION=1 and IB_V2_RUN_NODE=1 to submit the spread payload.",
        flush=True,
    )

    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 20))
        node.run()


if __name__ == "__main__":
    main()
