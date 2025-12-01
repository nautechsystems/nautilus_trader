#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
import asyncio

from nautilus_trader.common.config import DatabaseConfig
from nautilus_trader.common.config import MessageBusConfig
from nautilus_trader.live.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


class Strategy1(Strategy):
    def __init__(self, config: StrategyConfig) -> None:
        super().__init__(config)

    def on_start(self) -> None:
        # We put serialized dictionaries into Redis, so we need to register "dict" type
        self.msgbus.add_streaming_type(dict)

        self.msgbus.subscribe('events.command', self.on_command)

    def on_command(self, command: dict) -> None:
        self.log.info(f'on_command {command}')


async def main():
    config_node = TradingNodeConfig(
        message_bus=MessageBusConfig(
            database=DatabaseConfig(host='redis'),
            external_streams=["external_stream"],
            stream_per_topic=False
        )
    )
    node = TradingNode(config=config_node)
    node.trader.add_strategy(
        strategy=Strategy1(
            StrategyConfig(),
        )
    )

    node.build()

    try:
        await node.run_async()
    finally:
        await node.stop_async()
        await asyncio.sleep(1)
        node.dispose()


if __name__ == "__main__":
    asyncio.run(main())
