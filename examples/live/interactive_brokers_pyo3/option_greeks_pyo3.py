#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------
"""
Example: Download an IB option chain and subscribe to option greeks through the PyO3 client.

This example connects directly to the PyO3 IB data client, downloads a short-dated
options chain for an underlier, subscribes to a few call contracts, and prints any
received `OptionGreeks` events.
"""

from __future__ import annotations

import asyncio
import os
import re
from typing import Literal
from typing import cast

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import ib_contract_spec_to_dict
from nautilus_trader.adapters.interactive_brokers_pyo3.config import MarketDataType
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.model.data import OptionGreeks
from nautilus_trader.model.identifiers import TraderId


UNDERLYING = os.getenv("IB_PYO3_OPTION_UNDERLYING", "ES")
PRIMARY_EXCHANGE = os.getenv("IB_PYO3_OPTION_PRIMARY_EXCHANGE", "CME")
UNDERLYING_SEC_TYPE = os.getenv("IB_PYO3_OPTION_UNDERLYING_SEC_TYPE", "FUT")
UNDERLYING_LOCAL_SYMBOL = os.getenv("IB_PYO3_OPTION_UNDERLYING_LOCAL_SYMBOL", "ESM6")
MAX_SUBSCRIPTIONS = int(os.getenv("IB_PYO3_MAX_OPTION_GREEKS_SUBS", "3"))
AUTO_STOP_SECONDS = int(os.getenv("IB_PYO3_AUTO_STOP_SECONDS", "30"))
DATA_CLIENT_ID = int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "1401"))
MIN_EXPIRY_DAYS = int(os.getenv("IB_PYO3_OPTION_MIN_EXPIRY_DAYS", "0"))
MAX_EXPIRY_DAYS = int(os.getenv("IB_PYO3_OPTION_MAX_EXPIRY_DAYS", "5"))
EXACT_EXPIRY = os.getenv("IB_PYO3_OPTION_EXACT_EXPIRY")
CHAIN_LOAD_TIMEOUT_SECONDS = int(os.getenv("IB_PYO3_CHAIN_LOAD_TIMEOUT_SECONDS", "20"))


def _is_call_option_symbol(symbol: str) -> bool:
    normalized = " ".join(symbol.split())
    if UNDERLYING_SEC_TYPE in {"FUT", "CONTFUT"} and UNDERLYING_LOCAL_SYMBOL:
        compact = normalized.replace(" ", "")
        return re.match(r"^[A-Z0-9]+C[0-9.]+$", compact) is not None
    normalized = normalized.replace(" ", "")
    return re.match(rf"^{re.escape(UNDERLYING)}\d{{6}}C\d{{8}}$", normalized) is not None


def _ib_underlying_exchange() -> str:
    if UNDERLYING_SEC_TYPE in {"FUT", "CONTFUT"}:
        return PRIMARY_EXCHANGE

    return "SMART"


def _metadata_expiry_in_range(expiry: str) -> bool:
    if EXACT_EXPIRY is not None:
        return expiry == EXACT_EXPIRY

    from pandas import Timedelta
    from pandas import Timestamp

    now = Timestamp.now(tz="UTC").normalize()
    expiry_ts = Timestamp(expiry, tz="UTC")
    min_expiry = now + Timedelta(days=MIN_EXPIRY_DAYS)
    max_expiry = now + Timedelta(days=MAX_EXPIRY_DAYS)
    return min_expiry <= expiry_ts <= max_expiry


async def main() -> None:  # noqa: C901
    host, port = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("IB-GREEKS-001"), clock=clock)
    cache = Cache()

    provider_config = InteractiveBrokersInstrumentProviderConfig(
        load_ids=frozenset([f"{UNDERLYING}.{PRIMARY_EXCHANGE}"]),
        build_options_chain=False,
    )
    provider = InteractiveBrokersInstrumentProvider(config=provider_config)
    client_config = InteractiveBrokersDataClientConfig(
        ibg_host=host,
        ibg_port=port,
        ibg_client_id=DATA_CLIENT_ID,
        instrument_provider=provider_config,
        market_data_type=MarketDataType.Delayed,
    )

    rust_client = nautilus_pyo3.interactive_brokers.InteractiveBrokersDataClient(
        msgbus,
        cache,
        clock,
        provider._rust_provider,
        client_config,
    )
    provider._attach_loader(rust_client)

    received_greeks: list[OptionGreeks] = []
    received_event = asyncio.Event()

    def on_event(kind, _correlation_id, payload) -> None:
        if kind != "option_greeks":
            return

        greeks = OptionGreeks.from_pyo3(payload)
        received_greeks.append(greeks)
        print(
            "GREEKS "
            f"{greeks.instrument_id}: "
            f"delta={greeks.delta:.4f} gamma={greeks.gamma:.6f} "
            f"vega={greeks.vega:.4f} theta={greeks.theta:.4f} "
            f"mark_iv={greeks.mark_iv} bid_iv={greeks.bid_iv} ask_iv={greeks.ask_iv} "
            f"underlying={greeks.underlying_price} oi={greeks.open_interest}",
            flush=True,
        )
        received_event.set()

    rust_client.set_event_callback(on_event)
    print(f"Connecting to IB at {host}:{port}", flush=True)
    rust_client.connect()
    print("Connected", flush=True)

    try:
        contract = IBContract(
            secType=cast(
                Literal[
                    "CASH",
                    "STK",
                    "OPT",
                    "FUT",
                    "FOP",
                    "CONTFUT",
                    "CRYPTO",
                    "CFD",
                    "CMDTY",
                    "IND",
                    "BAG",
                    "",
                ],
                UNDERLYING_SEC_TYPE,
            ),
            symbol=UNDERLYING,
            exchange=_ib_underlying_exchange(),
            primaryExchange=PRIMARY_EXCHANGE,
            localSymbol=UNDERLYING_LOCAL_SYMBOL,
            build_options_chain=True,
            min_expiry_days=MIN_EXPIRY_DAYS,
            max_expiry_days=MAX_EXPIRY_DAYS,
        )

        if EXACT_EXPIRY:
            contract = IBContract(
                **{
                    **ib_contract_spec_to_dict(contract),
                    "lastTradeDateOrContractMonth": EXACT_EXPIRY,
                },
            )

        print(f"Loading underlying contract for {UNDERLYING}", flush=True)
        loaded_ids = await asyncio.wait_for(
            provider.load_ids_with_return_async(
                [
                    IBContract(
                        **{
                            **ib_contract_spec_to_dict(contract),
                            "build_options_chain": False,
                        },
                    ),
                ],
            ),
            timeout=min(CHAIN_LOAD_TIMEOUT_SECONDS, 20),
        )

        if not loaded_ids:
            print(f"Failed to load underlying contract for {UNDERLYING}", flush=True)
            return

        underlying_contract = await provider.instrument_id_to_ib_contract(loaded_ids[0])
        if underlying_contract is None:
            print(f"Failed to resolve qualified IB contract for {loaded_ids[0]}", flush=True)
            return

        metadata = await asyncio.wait_for(
            rust_client.py_get_option_chain_metadata_for_contract(underlying_contract),
            timeout=CHAIN_LOAD_TIMEOUT_SECONDS,
        )
        print(f"Received {len(metadata)} option chain metadata entries", flush=True)

        candidate_expiries: list[str] = []

        for chain in metadata:
            print(
                f"CHAIN exchange={chain['exchange']} trading_class={chain['trading_class']} "
                f"expiries={len(chain['expirations'])} strikes={len(chain['strikes'])}",
                flush=True,
            )

            for expiry in chain["expirations"]:
                if _metadata_expiry_in_range(expiry) and expiry not in candidate_expiries:
                    candidate_expiries.append(expiry)

        candidate_expiries.sort()
        if not candidate_expiries:
            print(
                "No option-chain expiries matched the configured range. "
                "Set IB_PYO3_OPTION_EXACT_EXPIRY or widen the day window.",
                flush=True,
            )
            return

        selected_expiry = candidate_expiries[0]
        print(
            f"Loading option contracts for selected expiry {selected_expiry}",
            flush=True,
        )
        raw_details = await asyncio.wait_for(
            rust_client.py_get_contract_details_for_contract(
                IBContract(
                    secType=cast(
                        Literal[
                            "CASH",
                            "STK",
                            "OPT",
                            "FUT",
                            "FOP",
                            "CONTFUT",
                            "CRYPTO",
                            "CFD",
                            "CMDTY",
                            "IND",
                            "BAG",
                            "",
                        ],
                        "FOP" if UNDERLYING_SEC_TYPE in {"FUT", "CONTFUT"} else "OPT",
                    ),
                    symbol=UNDERLYING,
                    exchange=PRIMARY_EXCHANGE,
                    lastTradeDateOrContractMonth=selected_expiry,
                ),
            ),
            timeout=CHAIN_LOAD_TIMEOUT_SECONDS,
        )
        print(
            f"Received {len(raw_details)} raw option contract details for expiry {selected_expiry}",
            flush=True,
        )

        loaded_ids = []

        for detail in raw_details[: MAX_SUBSCRIPTIONS * 4]:
            local_symbol = detail.contract.localSymbol
            if not local_symbol:
                continue

            # Ensure the option is cached in the Rust provider.
            instrument = await provider.get_instrument(
                IBContract(
                    secType=cast(
                        Literal[
                            "CASH",
                            "STK",
                            "OPT",
                            "FUT",
                            "FOP",
                            "CONTFUT",
                            "CRYPTO",
                            "CFD",
                            "CMDTY",
                            "IND",
                            "BAG",
                            "",
                        ],
                        "FOP" if UNDERLYING_SEC_TYPE in {"FUT", "CONTFUT"} else "OPT",
                    ),
                    exchange=PRIMARY_EXCHANGE,
                    localSymbol=local_symbol,
                ),
            )

            if instrument is not None:
                loaded_ids.append(
                    nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
                )
        print(f"Loaded {len(loaded_ids or [])} instrument IDs", flush=True)
        for instrument_id in loaded_ids[:10]:
            print(f"LOADED {instrument_id}", flush=True)

        selected = [
            instrument_id
            for instrument_id in loaded_ids
            if _is_call_option_symbol(str(instrument_id.symbol))
        ][:MAX_SUBSCRIPTIONS]

        if not selected:
            print(f"No call options found in cache for {UNDERLYING}", flush=True)
            return

        for instrument_id in selected:
            print(f"Subscribing to option greeks: {instrument_id}", flush=True)
            rust_client.subscribe_option_greeks(instrument_id)

        try:
            await asyncio.wait_for(received_event.wait(), timeout=AUTO_STOP_SECONDS)
        except TimeoutError:
            print(
                f"No OptionGreeks events received within {AUTO_STOP_SECONDS}s",
                flush=True,
            )
    except TimeoutError:
        print(
            f"Timed out after {CHAIN_LOAD_TIMEOUT_SECONDS}s while loading the bounded option chain. "
            "Try a smaller underlier or set IB_PYO3_OPTION_EXACT_EXPIRY to a single expiry.",
            flush=True,
        )
    finally:
        if "selected" in locals():
            for instrument_id in selected:
                rust_client.unsubscribe_option_greeks(instrument_id)
        rust_client.disconnect()
        print("Disconnected", flush=True)


if __name__ == "__main__":
    asyncio.run(main())
