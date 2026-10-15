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
Stream Polymarket PolyBolt reference prices (crypto spot, crypto TWAP, equity).

Running this example connects to PolyBolt with CLOB API credentials and logs every reference
price sample it receives. No orders are placed. Credentials are read from `POLYMARKET_API_KEY`,
`POLYMARKET_API_SECRET`, and `POLYMARKET_PASSPHRASE`.

With `RUN_SECONDS` set, the node stops itself after that many seconds, prints a per-type sample
count, and exits non-zero when the crypto spot or TWAP channel delivered nothing. Set it to
`None` to run until Ctrl-C. Equity snapshots can arrive outside market hours, while fresh stock
updates depend on trading sessions. A zero equity count does not fail the run.

"""

from __future__ import annotations

import asyncio
import sys
from typing import Any
from typing import Self

from nautilus_trader.adapters.polymarket import POLYMARKET
from nautilus_trader.adapters.polymarket import PolymarketCryptoPrice
from nautilus_trader.adapters.polymarket import PolymarketCryptoTwap
from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketEquityPrice
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.config import DataActorConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientId
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import TraderId


TRADER_ID = TraderId.from_str("TESTER-001")
CRYPTO_SYMBOL = "btcusd"
EQUITY_SYMBOL = "aapl"
RUN_SECONDS: int | None = 60
REQUIRED_TYPES = (PolymarketCryptoPrice.__name__, PolymarketCryptoTwap.__name__)


class PolyBoltPriceTesterConfig(DataActorConfig):
    """
    Configuration for the PolyBolt price tester.
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
        crypto_symbol: str = CRYPTO_SYMBOL,
        equity_symbol: str = EQUITY_SYMBOL,
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
        self.crypto_symbol = crypto_symbol
        self.equity_symbol = equity_symbol


class PolyBoltPriceTester(DataActor):
    """
    Subscribe to PolyBolt crypto spot, TWAP, and equity prices and log each sample.
    """

    def __init__(self, config: PolyBoltPriceTesterConfig) -> None:
        """
        Initialize the instance.
        """
        super().__init__(config)
        self.counts: dict[str, int] = {}
        self._data_types = [
            DataType(PolymarketCryptoPrice.__name__, {"symbol": config.crypto_symbol}),
            DataType(
                PolymarketCryptoTwap.__name__,
                {"symbol": config.crypto_symbol, "window_seconds": 60},
            ),
            DataType(PolymarketEquityPrice.__name__, {"symbol": config.equity_symbol}),
        ]

    def on_start(self) -> None:
        """
        Subscribe to every configured reference-price channel.
        """
        client_id = ClientId.from_str(POLYMARKET)
        for data_type in self._data_types:
            self.subscribe_data(data_type, client_id=client_id)

    def on_data(self, data: CustomData) -> None:
        """
        Log and count each reference-price sample.
        """
        type_name = data.data_type.type_name
        self.counts[type_name] = self.counts.get(type_name, 0) + 1
        log_msg = f"{type_name}: {data.data!r}"
        self.log.info(log_msg)

    def on_stop(self) -> None:
        """
        Unsubscribe from every configured reference-price channel.
        """
        client_id = ClientId.from_str(POLYMARKET)
        for data_type in self._data_types:
            self.unsubscribe_data(data_type, client_id=client_id)


async def run_for(node: LiveNode, seconds: int) -> None:
    """
    Run the node and request a stop after the given number of seconds.
    """
    handle = node.handle()

    async def stop_later() -> None:
        await asyncio.sleep(seconds)
        handle.stop()

    stopper = asyncio.create_task(stop_later())
    try:
        await node.run_async()
    finally:
        stopper.cancel()


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("POLYMARKET-POLYBOLT-TESTER-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=PolymarketInstrumentProviderConfig(
                    event_slugs=["presidential-election-winner-2028"],
                    use_gamma_markets=True,
                ),
                update_instruments_interval_mins=None,
            ),
        )
        .build()
    )
    actor = PolyBoltPriceTester(
        PolyBoltPriceTesterConfig(
            actor_id="POLYBOLT-PRICE-TESTER-001",
            crypto_symbol=CRYPTO_SYMBOL,
            equity_symbol=EQUITY_SYMBOL,
        ),
    )
    node.add_actor(actor)

    if RUN_SECONDS is None:
        node.run()
        return

    asyncio.run(run_for(node, RUN_SECONDS))

    print(f"PolyBolt samples received in {RUN_SECONDS}s:")
    for type_name in (*REQUIRED_TYPES, PolymarketEquityPrice.__name__):
        print(f"  {type_name}: {actor.counts.get(type_name, 0)}")

    missing = [name for name in REQUIRED_TYPES if actor.counts.get(name, 0) == 0]
    if missing:
        print(f"FAIL: no samples for {', '.join(missing)}")
        sys.exit(1)

    print("OK")


if __name__ == "__main__":
    main()
