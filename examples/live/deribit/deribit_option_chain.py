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
"""
Example: Subscribe to an option chain slice for BTC options on Deribit.

On start, this actor:
1. Queries the cache for all BTC option instruments
2. Finds the nearest expiry
3. Builds an OptionSeriesId for that expiry
4. Subscribes to an option chain with 3 strikes above and 3 below ATM
5. Uses ForwardPrice as the ATM source (auto-resolved)
6. Logs received OptionChainSlice snapshots in the on_option_chain handler
"""

from nautilus_trader.adapters.deribit import DERIBIT
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitLiveDataClientFactory
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DeribitEnvironment
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import TraderId


class OptionChainTesterConfig(ActorConfig, frozen=True):
    underlying: str = "BTC"
    strikes_above: int = 3
    strikes_below: int = 3
    snapshot_interval_ms: int = 2_000


class OptionChainTester(Actor):
    """
    Subscribes to an option chain and logs periodic snapshots.
    """

    def __init__(self, config: OptionChainTesterConfig) -> None:
        super().__init__(config)
        self._underlying = config.underlying
        self._strikes_above = config.strikes_above
        self._strikes_below = config.strikes_below
        self._snapshot_interval_ms = config.snapshot_interval_ms
        self._series_id: nautilus_pyo3.OptionSeriesId | None = None

    def on_start(self) -> None:
        instruments = self.cache.instruments()

        # Collect option instruments: (instrument, settlement_currency, expiry_ns)
        # Filter out already-expired options
        now_ns = self.clock.timestamp_ns()
        options = []

        for inst in instruments:
            if str(inst.id.venue) != DERIBIT:
                continue
            if not hasattr(inst, "option_kind"):
                continue
            expiry = getattr(inst, "expiration_ns", None)
            if expiry is None or expiry <= now_ns:
                continue
            options.append((inst, str(inst.settlement_currency), expiry))

        if not options:
            self.log.warning(f"No {self._underlying} options found in cache")
            return

        # Find the nearest (soonest) future expiry
        nearest_expiry = min(exp for _, _, exp in options)

        # Find settlement currency for nearest expiry (prefer BTC-settled)
        btc_settled = next(
            (s for _, s, exp in options if exp == nearest_expiry and s == "BTC"),
            None,
        )
        settlement = btc_settled or next(s for _, s, exp in options if exp == nearest_expiry)

        # Count options at nearest expiry with matching settlement
        count = sum(1 for _, s, exp in options if exp == nearest_expiry and s == settlement)

        self.log.info(
            f"Found {count} {self._underlying} options at nearest expiry "
            f"(ts={nearest_expiry}, settlement={settlement})",
        )

        # Build OptionSeriesId for the nearest expiry
        series_id = nautilus_pyo3.OptionSeriesId(
            DERIBIT,
            self._underlying,
            settlement,
            nearest_expiry,
        )
        self._series_id = series_id

        self.log.info(f"Subscribing to option chain: {series_id}")

        # Build StrikeRange
        strike_range = nautilus_pyo3.StrikeRange.atm_relative(
            strikes_above=self._strikes_above,
            strikes_below=self._strikes_below,
        )

        # Snapshot every 2 seconds (use None for raw stream mode)
        client_id = ClientId(DERIBIT)
        self.subscribe_option_chain(
            series_id=series_id,
            strike_range=strike_range,
            snapshot_interval_ms=self._snapshot_interval_ms,
            client_id=client_id,
        )

    def on_option_chain(self, chain_slice) -> None:
        atm = chain_slice.atm_strike or "-"
        self.log.info(
            f"OPTION_CHAIN | {chain_slice.series_id} | atm={atm} | "
            f"calls={chain_slice.call_count()} puts={chain_slice.put_count()} | "
            f"strikes={chain_slice.strike_count()}",
        )

        for strike in chain_slice.strikes():
            call = chain_slice.get_call(strike)
            put = chain_slice.get_put(strike)

            if call is not None:
                q = call.quote
                g = call.greeks
                if g is not None:
                    greeks_str = (
                        f"d={g.delta:.3f} g={g.gamma:.5f} v={g.vega:.2f} "
                        f"iv={((g.mark_iv or 0.0) * 100.0):.1f}%"
                    )
                else:
                    greeks_str = "-"
                call_info = f"bid={q.bid_price} ask={q.ask_price} [{greeks_str}]"
            else:
                call_info = "-"

            if put is not None:
                q = put.quote
                g = put.greeks
                if g is not None:
                    greeks_str = (
                        f"d={g.delta:.3f} g={g.gamma:.5f} v={g.vega:.2f} "
                        f"iv={((g.mark_iv or 0.0) * 100.0):.1f}%"
                    )
                else:
                    greeks_str = "-"
                put_info = f"bid={q.bid_price} ask={q.ask_price} [{greeks_str}]"
            else:
                put_info = "-"

            self.log.info(f"  K={strike} | CALL: {call_info} | PUT: {put_info}")

    def on_stop(self) -> None:
        if self._series_id is not None:
            self.unsubscribe_option_chain(
                series_id=self._series_id,
                client_id=ClientId(DERIBIT),
            )
            self.log.info(f"Unsubscribed from option chain {self._series_id}")


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("CHAIN-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    data_clients={
        DERIBIT: DeribitDataClientConfig(
            environment=DeribitEnvironment.MAINNET,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=2.0,
)

node = TradingNode(config=config_node)
node.trader.add_actor(OptionChainTester(OptionChainTesterConfig()))

node.add_data_client_factory(DERIBIT, DeribitLiveDataClientFactory)
node.build()

try:
    node.run()
finally:
    node.dispose()
