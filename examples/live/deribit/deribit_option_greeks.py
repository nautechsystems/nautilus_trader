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
Subscribe to option Greeks for individual BTC call options on Deribit.

Running this example connects to Deribit mainnet, loads options, selects call contracts
from the instrument cache, and logs every Greeks update. No orders are placed.

"""

from __future__ import annotations

from typing import Any
from typing import Self

from nautilus_trader.adapters.deribit import DERIBIT
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitDataClientFactory
from nautilus_trader.adapters.deribit import DeribitEnvironment
from nautilus_trader.adapters.deribit import DeribitProductType
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


TRADER_ID = TraderId.from_str("GREEKS-001")
UNDERLYING = "BTC"
MAX_SUBSCRIPTIONS = 10


class OptionGreeksTesterConfig(DataActorConfig):
    """
    Collect option greeks tester config tests.
    """

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        """
        Create a new instance.
        """
        # `actor_id` shares the base field name but widens the type to accept a string,
        # so keep it from the base constructor, which validates it as an `ActorId`
        kwargs.pop("actor_id", None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        *,
        underlying: str = "BTC",
        max_subscriptions: int = 10,
        actor_id: ActorId | str | None = None,
        log_events: bool = True,
        log_commands: bool = True,
    ) -> None:
        """
        Initialize the instance.
        """
        self.actor_id = ActorId.from_str(actor_id) if isinstance(actor_id, str) else actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.underlying = underlying
        self.max_subscriptions = max_subscriptions


class OptionGreeksTester(DataActor):
    """
    Subscribe to option Greeks for call options on Deribit.
    """

    def __init__(self, config: OptionGreeksTesterConfig) -> None:
        """
        Initialize the instance.
        """
        super().__init__(config)
        self._subscribed_ids: list[InstrumentId] = []
        self._underlying = config.underlying
        self._max_subscriptions = config.max_subscriptions

    def on_start(self) -> None:
        """
        On start.
        """
        call_options = []

        for instrument in self.cache.instruments():
            symbol = str(instrument.id.symbol)
            if symbol.startswith(f"{self._underlying}-") and symbol.endswith("-C"):
                call_options.append(instrument)

        if not call_options:
            log_msg = f"No {self._underlying} call options found in cache"
            self.log.warning(log_msg)
            return

        call_options.sort(key=lambda instrument: str(instrument.id.symbol))
        client_id = ClientId.from_str(DERIBIT)

        for instrument in call_options[: self._max_subscriptions]:
            log_msg = f"Subscribing to Greeks: {instrument.id}"
            self.log.info(log_msg)
            self.subscribe_option_greeks(instrument.id, client_id=client_id)
            self._subscribed_ids.append(instrument.id)

        log_msg = f"Subscribed to {len(self._subscribed_ids)} option Greeks streams"
        self.log.info(log_msg)

    def on_option_greeks(self, greeks: OptionGreeks) -> None:
        """
        On option greeks.
        """
        log_msg = (
            f"GREEKS {greeks.instrument_id}: "
            f"delta={greeks.delta:.4f} gamma={greeks.gamma:.6f} "
            f"vega={greeks.vega:.4f} theta={greeks.theta:.4f} "
            f"mark_iv={greeks.mark_iv} bid_iv={greeks.bid_iv} ask_iv={greeks.ask_iv} "
            f"underlying={greeks.underlying_price} oi={greeks.open_interest}"
        )
        self.log.info(log_msg)

    def on_stop(self) -> None:
        """
        On stop.
        """
        client_id = ClientId.from_str(DERIBIT)

        for instrument_id in self._subscribed_ids:
            self.unsubscribe_option_greeks(instrument_id, client_id=client_id)

        self.log.info("Unsubscribed from all option Greeks")


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder(
            "DERIBIT-OPTION-GREEKS-001",
            TRADER_ID,
            Environment.LIVE,
        )
        .add_data_client(
            None,
            DeribitDataClientFactory(),
            DeribitDataClientConfig(
                product_types=[DeribitProductType.OPTION],
                environment=DeribitEnvironment.MAINNET,
            ),
        )
        .build()
    )
    node.add_actor_from_config(
        ImportableActorConfig(
            actor_path="deribit_option_greeks:OptionGreeksTester",
            config_path="deribit_option_greeks:OptionGreeksTesterConfig",
            config={
                "actor_id": "DERIBIT-OPTION-GREEKS-001",
                "underlying": UNDERLYING,
                "max_subscriptions": MAX_SUBSCRIPTIONS,
            },
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
