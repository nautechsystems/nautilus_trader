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
Example: Subscribe to option greeks for BTC CALL options on OKX.

Discovers BTC CALL options from the instrument cache, filters for a subset,
and subscribes to exchange-provided greeks (delta, gamma, vega, theta, IV).

Subscriptions are split three ways to exercise every param shape:
- no params (adapter emits both Black-Scholes and price-adjusted)
- single convention via ``params["greeks_convention"] = "BLACK_SCHOLES"``
- list of conventions via ``params["greeks_convention"] = [...]``
"""

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXLiveDataClientFactory
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXEnvironment
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


class OptionGreeksTesterConfig(ActorConfig, frozen=True):
    underlying: str = "BTC"
    max_subscriptions: int = 10


class OptionGreeksTester(Actor):
    """
    Subscribe to option greeks for BTC CALL options on OKX, exercising the three
    supported shapes for ``params["greeks_convention"]``.
    """

    def __init__(self, config: OptionGreeksTesterConfig) -> None:
        super().__init__(config)
        self._subscribed_ids: list[InstrumentId] = []
        self._underlying = config.underlying
        self._max_subscriptions = config.max_subscriptions

    def on_start(self) -> None:
        instruments = self.cache.instruments()

        call_options = []

        for inst in instruments:
            symbol = str(inst.id.symbol)
            if not symbol.startswith(f"{self._underlying}-"):
                continue
            # OKX option symbols: BASE-QUOTE-DATE-STRIKE-C/P
            if symbol.endswith("-C"):
                call_options.append(inst)

        if not call_options:
            self.log.warning(f"No {self._underlying} CALL options found in cache")
            return

        self.log.info(f"Found {len(call_options)} {self._underlying} CALL options")

        call_options.sort(key=lambda i: str(i.id.symbol))
        to_subscribe = call_options[: self._max_subscriptions]

        client_id = ClientId(OKX)
        third = len(to_subscribe) // 3
        default_slice = to_subscribe[:third]
        single_slice = to_subscribe[third : 2 * third]
        list_slice = to_subscribe[2 * third :]

        for inst in default_slice:
            self.log.info(f"Subscribing to greeks (no params, both conventions): {inst.id}")
            self.subscribe_option_greeks(inst.id, client_id=client_id)
            self._subscribed_ids.append(inst.id)

        for inst in single_slice:
            self.log.info(f"Subscribing to greeks (single BS): {inst.id}")
            self.subscribe_option_greeks(
                inst.id,
                client_id=client_id,
                params={"greeks_convention": "BLACK_SCHOLES"},
            )
            self._subscribed_ids.append(inst.id)

        for inst in list_slice:
            self.log.info(f"Subscribing to greeks (list form): {inst.id}")
            self.subscribe_option_greeks(
                inst.id,
                client_id=client_id,
                params={"greeks_convention": ["BLACK_SCHOLES", "PRICE_ADJUSTED"]},
            )
            self._subscribed_ids.append(inst.id)

        self.log.info(f"Subscribed to {len(self._subscribed_ids)} option greeks streams")

    def on_option_greeks(self, greeks) -> None:
        self.log.info(
            f"GREEKS {greeks.instrument_id}: "
            f"convention={greeks.convention} "
            f"delta={greeks.delta:.4f} gamma={greeks.gamma:.6f} "
            f"vega={greeks.vega:.4f} theta={greeks.theta:.4f} "
            f"mark_iv={greeks.mark_iv} bid_iv={greeks.bid_iv} ask_iv={greeks.ask_iv} "
            f"underlying={greeks.underlying_price} oi={greeks.open_interest}",
        )

    def on_stop(self) -> None:
        client_id = ClientId(OKX)
        for instrument_id in self._subscribed_ids:
            self.unsubscribe_option_greeks(instrument_id, client_id=client_id)
        self.log.info("Unsubscribed from all option greeks")


config_node = TradingNodeConfig(
    trader_id=TraderId("GREEKS-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    data_clients={
        OKX: OKXDataClientConfig(
            environment=OKXEnvironment.DEMO,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.OPTION,),
            instrument_families=("BTC-USD",),
            is_demo=True,
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=2.0,
)

node = TradingNode(config=config_node)
node.trader.add_actor(OptionGreeksTester(OptionGreeksTesterConfig()))

node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.build()

try:
    node.run()
finally:
    node.dispose()
