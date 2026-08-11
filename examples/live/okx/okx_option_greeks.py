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
Subscribe to option Greeks for individual BTC call options on OKX.

The default path builds the live node without connecting. Set ``RUN_NODE`` to load options,
select contracts from the instrument cache, and start subscriptions.
Subscriptions exercise the default, Black-Scholes, and combined convention shapes.

"""

from __future__ import annotations

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXDataClientFactory
from nautilus_trader.adapters.okx import OKXEnvironment
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.config import DataActorConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import TraderId


RUN_NODE = False
TRADER_ID = TraderId.from_str("GREEKS-001")
UNDERLYING = "BTC"
INSTRUMENT_FAMILIES = ["BTC-USD"]
MAX_SUBSCRIPTIONS = 10


class OptionGreeksTesterConfig(DataActorConfig):
    def __init__(
        self,
        underlying: str = "BTC",
        max_subscriptions: int = 10,
        actor_id: ActorId | str | None = None,
        log_events: bool = True,
        log_commands: bool = True,
    ) -> None:
        self.actor_id = ActorId.from_str(actor_id) if isinstance(actor_id, str) else actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.underlying = underlying
        self.max_subscriptions = max_subscriptions


class OptionGreeksTester(DataActor):
    """
    Subscribe to option Greeks for call options on OKX.
    """

    def __init__(self, config: OptionGreeksTesterConfig) -> None:
        super().__init__(config)
        self._subscribed_ids: list[InstrumentId] = []
        self._underlying = config.underlying
        self._max_subscriptions = config.max_subscriptions

    def on_start(self) -> None:
        call_options = []

        for instrument in self.cache.instruments():
            symbol = str(instrument.id.symbol)
            if symbol.startswith(f"{self._underlying}-") and symbol.endswith("-C"):
                call_options.append(instrument)

        if not call_options:
            self.log.warning(f"No {self._underlying} call options found in cache")
            return

        call_options.sort(key=lambda instrument: str(instrument.id.symbol))
        to_subscribe = call_options[: self._max_subscriptions]
        client_id = ClientId.from_str(OKX)
        third = len(to_subscribe) // 3

        for instrument in to_subscribe[:third]:
            self.log.info(f"Subscribing to Greeks with both conventions: {instrument.id}")
            self.subscribe_option_greeks(instrument.id, client_id=client_id)
            self._subscribed_ids.append(instrument.id)

        for instrument in to_subscribe[third : 2 * third]:
            self.log.info(f"Subscribing to Black-Scholes Greeks: {instrument.id}")
            self.subscribe_option_greeks(
                instrument.id,
                client_id=client_id,
                params={"greeks_convention": "BLACK_SCHOLES"},
            )
            self._subscribed_ids.append(instrument.id)

        for instrument in to_subscribe[2 * third :]:
            self.log.info(
                f"Subscribing to both Greeks conventions using list form: {instrument.id}",
            )
            self.subscribe_option_greeks(
                instrument.id,
                client_id=client_id,
                params={"greeks_convention": ["BLACK_SCHOLES", "PRICE_ADJUSTED"]},
            )
            self._subscribed_ids.append(instrument.id)

        self.log.info(f"Subscribed to {len(self._subscribed_ids)} option Greeks streams")

    def on_option_greeks(self, greeks: OptionGreeks) -> None:
        self.log.info(
            f"GREEKS {greeks.instrument_id}: "
            f"convention={greeks.convention} "
            f"delta={greeks.delta:.4f} gamma={greeks.gamma:.6f} "
            f"vega={greeks.vega:.4f} theta={greeks.theta:.4f} "
            f"mark_iv={greeks.mark_iv} bid_iv={greeks.bid_iv} ask_iv={greeks.ask_iv} "
            f"underlying={greeks.underlying_price} oi={greeks.open_interest}",
        )

    def on_stop(self) -> None:
        client_id = ClientId.from_str(OKX)

        for instrument_id in self._subscribed_ids:
            self.unsubscribe_option_greeks(instrument_id, client_id=client_id)

        self.log.info("Unsubscribed from all option Greeks")


def main() -> None:
    node = (
        LiveNode.builder(
            "OKX-OPTION-GREEKS-001",
            TRADER_ID,
            Environment.LIVE,
        )
        .add_data_client(
            None,
            OKXDataClientFactory(),
            OKXDataClientConfig(
                instrument_types=[OKXInstrumentType.OPTION],
                instrument_families=INSTRUMENT_FAMILIES,
                environment=OKXEnvironment.DEMO,
            ),
        )
        .build()
    )
    node.add_actor_from_config(
        ImportableActorConfig(
            actor_path="okx_option_greeks:OptionGreeksTester",
            config_path="okx_option_greeks:OptionGreeksTesterConfig",
            config={
                "actor_id": "OKX-OPTION-GREEKS-001",
                "underlying": UNDERLYING,
                "max_subscriptions": MAX_SUBSCRIPTIONS,
            },
        ),
    )

    if RUN_NODE:
        node.run()
    else:
        print("Built OKX option Greeks node. Set RUN_NODE = True to connect.")


if __name__ == "__main__":
    main()
