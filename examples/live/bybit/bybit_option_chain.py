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
Example: Subscribe to an option chain slice for BTC options on Bybit.

On start, this actor:
1. Queries the cache for all BTC option instruments
2. Finds the nearest expiry
3. Builds an OptionSeriesId for that expiry
4. Subscribes to an option chain with 3 strikes above and 3 below ATM
5. Uses ForwardPrice as the ATM source (auto-resolved from option ticker underlying_price)
6. Logs received OptionChainSlice snapshots in the on_option_chain handler

"""

from __future__ import annotations

from typing import Any
from typing import Self

from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitDataClientFactory
from nautilus_trader.adapters.bybit import BybitEnvironment
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.config import DataActorConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientId
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import StrikeRange
from nautilus_trader.model import TraderId


TRADER_ID = TraderId.from_str("CHAIN-001")
UNDERLYING = "BTC"
STRIKES_ABOVE = 3
STRIKES_BELOW = 3
SNAPSHOT_INTERVAL_MS = 5_000


class OptionChainTesterConfig(DataActorConfig):
    _CUSTOM_FIELDS = (
        "actor_id",
        "underlying",
        "strikes_above",
        "strikes_below",
        "snapshot_interval_ms",
    )

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        underlying: str = UNDERLYING,
        strikes_above: int = STRIKES_ABOVE,
        strikes_below: int = STRIKES_BELOW,
        snapshot_interval_ms: int = SNAPSHOT_INTERVAL_MS,
        actor_id: ActorId | str | None = None,
        log_events: bool = True,
        log_commands: bool = True,
    ) -> None:
        self.actor_id = ActorId.from_str(actor_id) if isinstance(actor_id, str) else actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.underlying = underlying
        self.strikes_above = strikes_above
        self.strikes_below = strikes_below
        self.snapshot_interval_ms = snapshot_interval_ms


class OptionChainTester(DataActor):
    """
    Subscribes to an option chain and logs periodic snapshots.
    """

    def __init__(self, config: OptionChainTesterConfig) -> None:
        super().__init__(config)
        self._underlying = config.underlying
        self._strikes_above = config.strikes_above
        self._strikes_below = config.strikes_below
        self._snapshot_interval_ms = config.snapshot_interval_ms
        self._series_id: OptionSeriesId | None = None

    def on_start(self) -> None:
        instruments = self.cache.instruments()

        # Collect option instruments: (instrument, settlement_currency, expiry_ns)
        # Bybit BTC options are USDT-settled (linear contracts).
        # Filter out already-expired options
        now_ns = self.clock.timestamp_ns()
        options = []

        for inst in instruments:
            if str(inst.id.venue) != BYBIT:
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

        # Prefer USDT-settled (Bybit BTC options default); fall back to any available
        usdt_settled = next(
            (s for _, s, exp in options if exp == nearest_expiry and s == "USDT"),
            None,
        )
        settlement = usdt_settled or next(s for _, s, exp in options if exp == nearest_expiry)

        # Count options at nearest expiry with matching settlement
        count = sum(1 for _, s, exp in options if exp == nearest_expiry and s == settlement)

        self.log.info(
            f"Found {count} {self._underlying} options at nearest expiry "
            f"(ts={nearest_expiry}, settlement={settlement})",
        )

        # Build OptionSeriesId for the nearest expiry
        series_id = OptionSeriesId(
            BYBIT,
            self._underlying,
            settlement,
            nearest_expiry,
        )
        self._series_id = series_id

        self.log.info(f"Subscribing to option chain: {series_id}")

        # Build StrikeRange
        strike_range = StrikeRange.atm_relative(
            strikes_above=self._strikes_above,
            strikes_below=self._strikes_below,
        )

        # Snapshot every 5 seconds (use None for raw stream mode)
        client_id = ClientId(BYBIT)
        self.subscribe_option_chain(
            series_id=series_id,
            strike_range=strike_range,
            snapshot_interval_ms=self._snapshot_interval_ms,
            client_id=client_id,
        )

    def on_option_chain(self, slice: OptionChainSlice) -> None:
        atm = slice.atm_strike or "-"
        self.log.info(
            f"OPTION_CHAIN | {slice.series_id} | atm={atm} | "
            f"calls={slice.call_count()} puts={slice.put_count()} | "
            f"strikes={slice.strike_count()}",
        )

        for strike in slice.strikes():
            call = slice.get_call(strike)
            put = slice.get_put(strike)

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
                client_id=ClientId(BYBIT),
            )
            self.log.info(f"Unsubscribed from option chain {self._series_id}")


def main() -> None:
    node = (
        LiveNode.builder("BYBIT-OPTION-CHAIN-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            None,
            BybitDataClientFactory(),
            BybitDataClientConfig(
                product_types=[BybitProductType.OPTION],
                environment=BybitEnvironment.MAINNET,
            ),
        )
        .build()
    )
    node.add_actor_from_config(
        ImportableActorConfig(
            actor_path="bybit_option_chain:OptionChainTester",
            config_path="bybit_option_chain:OptionChainTesterConfig",
            config={
                "actor_id": "BYBIT-OPTION-CHAIN-001",
                "underlying": UNDERLYING,
                "strikes_above": STRIKES_ABOVE,
                "strikes_below": STRIKES_BELOW,
                "snapshot_interval_ms": SNAPSHOT_INTERVAL_MS,
            },
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
