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

import asyncio
import os

from _common import add_strategy_from_config
from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import futures_contract
from _common import instrument_provider_config
from _common import is_ib_endpoint_reachable
from _common import option_contract
from _common import resolve_ib_endpoint
from _common import schedule_node_stop

from nautilus_trader.core import nautilus_pyo3 as pyo3


async def main() -> None:
    ib = pyo3.interactive_brokers
    host, port = resolve_ib_endpoint()
    if not is_ib_endpoint_reachable(host, port):
        print(f"IB Gateway/TWS is not reachable at {host}:{port}", flush=True)
        return

    option_contracts = [
        option_contract(
            local_symbol=os.getenv("IB_V2_OPTION_LOCAL_SYMBOL", "ESM6 P6800"),
            right=ib.IbOptionRight.PUT,
            strike=float(os.getenv("IB_V2_OPTION_STRIKE", "6800")),
        ),
    ]
    provider_config = instrument_provider_config(
        load_contracts=[futures_contract(), *option_contracts],
    )
    provider = ib.InteractiveBrokersInstrumentProvider(provider_config)
    client_config = ib.InteractiveBrokersDataClientConfig(
        host=host,
        port=port,
        client_id=env_int("IB_V2_OPTION_CLIENT_ID", 1401),
        connection_timeout=env_int("IB_V2_CONNECTION_TIMEOUT", 10),
        request_timeout=env_int("IB_V2_REQUEST_TIMEOUT", 30),
        market_data_type=ib.MarketDataType.DELAYED,
        instrument_provider=provider_config,
    )
    try:
        client = ib.HistoricalInteractiveBrokersClient(provider, client_config)
    except RuntimeError as exc:
        print(f"Failed to connect to IB Gateway/TWS at {host}:{port}: {exc}", flush=True)
        return

    print("Requesting bounded option contracts...", flush=True)
    instruments = await client.request_instruments(contracts=option_contracts)
    print(f"Loaded {len(instruments)} option instrument(s)", flush=True)
    for instrument in instruments:
        print(instrument.id, flush=True)

    if instruments:
        os.environ.setdefault("IB_V2_OPTION_INSTRUMENT_ID", str(instruments[0].id))

    if not env_bool("IB_V2_RUN_NODE"):
        print(
            "Set IB_V2_RUN_NODE=1 to subscribe to option greeks through a v2 strategy.",
            flush=True,
        )
        return

    node = build_ib_live_node(
        name="IB-V2-OPTION-GREEKS-001",
        trader_id="IB-V2-OPTION-GREEKS-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_OPTION_NODE_CLIENT_ID", 1402),
        provider_config=provider_config,
    )
    add_strategy_from_config(
        node,
        "ib_v2_order_strategies:OptionGreeksStrategy",
    )
    schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 30))
    node.run()


if __name__ == "__main__":
    asyncio.run(main())
