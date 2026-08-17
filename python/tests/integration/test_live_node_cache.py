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

import os
import socket
import threading
import time

import pytest

from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.core import UUID4
from nautilus_trader.infrastructure import PostgresCacheConfig
from nautilus_trader.infrastructure import RedisCacheConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId
from nautilus_trader.trading import Strategy
from tests.unit.common.actor import TestStrategyConfig


INFRASTRUCTURE_ENDPOINTS = (("127.0.0.1", 6379), ("127.0.0.1", 5432))
INFRASTRUCTURE_REQUIRED = os.environ.get("NAUTILUS_TEST_INFRASTRUCTURE") == "1"


def _infrastructure_is_reachable() -> bool:
    for endpoint in INFRASTRUCTURE_ENDPOINTS:
        try:
            with socket.create_connection(endpoint, timeout=1.0):
                pass
        except OSError:
            return False
    return True


pytestmark = pytest.mark.skipif(
    not INFRASTRUCTURE_REQUIRED and not _infrastructure_is_reachable(),
    reason="Redis and Postgres infrastructure services are not reachable",
)


class GeneralDataActor(DataActor):
    """
    Reads and writes general cache data from inside the node's own lifecycle.

    `Cache` is unsendable and a database-backed node owns its thread, so cache access has to
    happen on the node thread rather than from the test body.

    """

    key: str = ""
    token: bytes = b""
    write_on_start: bool = False
    loaded: bytes | None = None

    @classmethod
    def configure(cls, key: str, token: bytes, *, write_on_start: bool) -> None:
        cls.key = key
        cls.token = token
        cls.write_on_start = write_on_start
        cls.loaded = None

    def on_start(self) -> None:
        cls = type(self)
        if cls.write_on_start:
            self.cache.add(cls.key, cls.token)
        else:
            cls.loaded = self.cache.get(cls.key)


class StateRoundTripActor(DataActor):
    saved_state: dict[str, bytes] = {}
    loaded_state: dict[str, bytes] | None = None

    @classmethod
    def set_saved_state(cls, saved_state: dict[str, bytes]) -> None:
        cls.saved_state = saved_state
        cls.loaded_state = None

    def on_save(self) -> dict[str, bytes]:
        return type(self).saved_state

    def on_load(self, state: dict[str, bytes]) -> None:
        type(self).loaded_state = state


class StateRoundTripStrategy(Strategy):
    saved_state: dict[str, bytes] = {}
    loaded_state: dict[str, bytes] | None = None

    @classmethod
    def set_saved_state(cls, saved_state: dict[str, bytes]) -> None:
        cls.saved_state = saved_state
        cls.loaded_state = None

    def on_save(self) -> dict[str, bytes]:
        return type(self).saved_state

    def on_load(self, state: dict[str, bytes]) -> None:
        type(self).loaded_state = state


def _build_cache_node(
    cache_database_config: PostgresCacheConfig | RedisCacheConfig,
    trader_id: TraderId,
    *,
    load_state: bool = False,
    save_state: bool = False,
) -> LiveNode:
    return (
        LiveNode.builder(
            "TEST",
            trader_id,
            Environment.SANDBOX,
        )
        .with_cache_database_factory(cache_database_config)
        .with_load_state(load_state)
        .with_save_state(save_state)
        .with_reconciliation(False)
        .with_timeout_connection(0)
        .with_timeout_reconciliation(0)
        .with_timeout_portfolio(0)
        .with_timeout_disconnection_secs(0)
        .with_delay_post_stop_secs(0)
        .with_delay_shutdown_secs(0)
        .build()
    )


def _run_node_lifecycle(node: LiveNode) -> None:
    """
    Run the node on this thread until a worker stops it.

    A cache database backing blocks its caller, so these nodes own their thread through
    `run()` rather than sharing a host event loop.

    """
    handle = node.handle()

    def stop_when_running() -> None:
        deadline = time.monotonic() + 30.0
        while not handle.is_running and time.monotonic() < deadline:
            time.sleep(0.01)
        handle.stop()

    stopper = threading.Thread(target=stop_when_running, daemon=True)
    stopper.start()

    try:
        node.run()
    finally:
        stopper.join(timeout=30.0)
        node.dispose()


@pytest.mark.parametrize(
    "config_type",
    [
        pytest.param(RedisCacheConfig, id="redis"),
        pytest.param(PostgresCacheConfig, id="postgres"),
    ],
)
def test_cache_backing_round_trips_general_data(
    config_type: type[PostgresCacheConfig] | type[RedisCacheConfig],
) -> None:
    token = str(UUID4()).encode()
    key = f"integration-{UUID4()}"
    trader_id = TraderId(f"TESTER-CACHE-{UUID4()}")

    actor_config = ImportableActorConfig(
        actor_path="tests.integration.test_live_node_cache:GeneralDataActor",
        config_path="nautilus_trader.common:DataActorConfig",
        config={"actor_id": "GENERAL-DATA"},
    )

    GeneralDataActor.configure(key, token, write_on_start=True)
    saving_node = _build_cache_node(config_type(), trader_id)
    saving_node.add_actor_from_config(actor_config)
    _run_node_lifecycle(saving_node)

    GeneralDataActor.configure(key, token, write_on_start=False)
    loading_node = _build_cache_node(config_type(), trader_id)
    loading_node.add_actor_from_config(actor_config)
    _run_node_lifecycle(loading_node)

    loaded = GeneralDataActor.loaded

    assert loaded == token


def test_redis_cache_backing_round_trips_actor_and_strategy_state() -> None:
    token = str(UUID4()).encode()
    trader_id = TraderId(f"TESTER-STATE-{UUID4()}")
    actor_id = f"STATE-ROUND-TRIP-ACTOR-{UUID4()}"
    strategy_id = f"STATE-ROUND-TRIP-STRATEGY-{UUID4()}"
    actor_saved = {"actor-token": token}
    strategy_saved = {"strategy-token": token}
    StateRoundTripActor.set_saved_state(actor_saved)
    StateRoundTripStrategy.set_saved_state(strategy_saved)
    actor_config = ImportableActorConfig(
        actor_path="tests.integration.test_live_node_cache:StateRoundTripActor",
        config_path="nautilus_trader.common:DataActorConfig",
        config={"actor_id": actor_id},
    )

    saving_node = _build_cache_node(RedisCacheConfig(), trader_id, save_state=True)
    saving_node.add_actor_from_config(actor_config)
    saving_node.add_strategy(
        StateRoundTripStrategy(TestStrategyConfig(strategy_id=strategy_id)),
    )
    _run_node_lifecycle(saving_node)

    assert StateRoundTripActor.loaded_state is None
    assert StateRoundTripStrategy.loaded_state is None

    loading_node = _build_cache_node(RedisCacheConfig(), trader_id, load_state=True)
    loading_node.add_actor_from_config(actor_config)
    loading_node.add_strategy(
        StateRoundTripStrategy(TestStrategyConfig(strategy_id=strategy_id)),
    )
    _run_node_lifecycle(loading_node)

    assert StateRoundTripActor.loaded_state == actor_saved
    assert StateRoundTripStrategy.loaded_state == strategy_saved
