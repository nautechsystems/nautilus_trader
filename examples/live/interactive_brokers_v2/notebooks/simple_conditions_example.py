#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import datetime as dt
import os
import sys
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import futures_contract
from _common import ib_order_tags
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop

from nautilus_trader.core import nautilus_pyo3 as pyo3


def main() -> None:
    ib = pyo3.interactive_brokers
    host, port = resolve_ib_endpoint()
    provider_config = instrument_provider_config(
        load_contracts=[futures_contract()],
        symbol_to_mic_venue={"ES": "IB"},
    )
    account_id = (
        os.getenv("TWS_ACCOUNT")
        if env_bool("IB_V2_ENABLE_EXECUTION") or env_bool("IB_V2_ENABLE_ORDER_SUBMISSION")
        else None
    )
    node = build_ib_live_node(
        name="IB-V2-CONDITIONS-001",
        trader_id="IB-V2-CONDITIONS-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 1421),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 1422),
        account_id=account_id,
        provider_config=provider_config,
    )
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:SimpleConditionsStrategy",
    )
    time_condition = {
        "type": ib.IbConditionKind.TIME.as_str(),
        "time": (dt.datetime.now() + dt.timedelta(minutes=5)).strftime("%Y%m%d-%H:%M:%S"),
        "isMore": True,
        "conjunction": ib.IbConditionConjunction.AND.as_str(),
    }
    conditions = [time_condition]
    if env_bool("IB_V2_ENABLE_PRICE_CONDITION"):
        conditions.append(
            {
                "type": ib.IbConditionKind.PRICE.as_str(),
                "conId": env_int("IB_V2_CONDITION_CONTRACT_ID", 0),
                "exchange": "CME",
                "isMore": True,
                "price": 6000.0,
                "triggerMethod": ib.IbTriggerMethod.DEFAULT.as_i32(),
                "conjunction": ib.IbConditionConjunction.AND.as_str(),
            },
        )

    tag = ib_order_tags(
        conditions=conditions,
        conditionsCancelOrder=env_bool("IB_V2_CONDITIONS_CANCEL_ORDER", False),
    )

    print(
        f"Built v2 conditional-order node and registered SimpleConditionsStrategy: {node.trader_id}",
        flush=True,
    )
    print(f"Strategy submits the conditional tag when enabled: {tag}", flush=True)

    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 20))
        node.run()


if __name__ == "__main__":
    main()
