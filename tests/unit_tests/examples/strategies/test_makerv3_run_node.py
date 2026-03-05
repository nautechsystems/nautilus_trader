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

from __future__ import annotations

from examples.live.makerv3 import run_node


class _DummyStrategy:
    def __init__(self) -> None:
        self.params_manager_factory = None

    def set_params_manager_factory(self, factory) -> None:
        self.params_manager_factory = factory


def test_attach_runtime_params_manager_wires_redis_backed_factory(monkeypatch) -> None:
    strategy = _DummyStrategy()
    redis_call: dict[str, object] = {}
    factory_call: dict[str, object] = {}
    redis_client = object()
    sentinel_factory = object()

    def _fake_redis(**kwargs):
        redis_call.update(kwargs)
        return redis_client

    def _fake_params_manager_factory(**kwargs):
        factory_call.update(kwargs)
        return sentinel_factory

    monkeypatch.setattr(run_node.redis, "Redis", _fake_redis)
    monkeypatch.setattr(
        run_node.runtime_params_mod,
        "params_manager_factory",
        _fake_params_manager_factory,
    )

    run_node._attach_runtime_params_manager(
        strategy=strategy,
        redis_cfg={
            "host": "127.0.0.10",
            "port": 6381,
            "db": 4,
            "username": "alice",
            "password": "secret",
            "connect_timeout_secs": 7.5,
            "read_timeout_secs": 8.5,
        },
        namespace="fluxx",
        schema_version="v2",
    )

    assert redis_call == {
        "host": "127.0.0.10",
        "port": 6381,
        "db": 4,
        "username": "alice",
        "password": "secret",
        "socket_connect_timeout": 7.5,
        "socket_timeout": 8.5,
        "decode_responses": False,
    }
    assert factory_call == {
        "redis_client": redis_client,
        "namespace": "fluxx",
        "schema_version": "v2",
    }
    assert strategy.params_manager_factory is sentinel_factory


def test_resolve_reconciliation_settings_enforces_live_minimum_startup_delay() -> None:
    lookback, startup_delay = run_node._resolve_reconciliation_settings(
        mode="live",
        node_cfg={
            "exec_reconciliation_lookback_mins": -5,
            "exec_reconciliation_startup_delay_secs": 1.0,
        },
    )

    assert lookback == 0
    assert startup_delay == 10.0


def test_resolve_reconciliation_settings_keeps_dev_values_in_paper_mode() -> None:
    lookback, startup_delay = run_node._resolve_reconciliation_settings(
        mode="paper",
        node_cfg={
            "exec_reconciliation_lookback_mins": 5,
            "exec_reconciliation_startup_delay_secs": 1.0,
        },
    )

    assert lookback == 5
    assert startup_delay == 1.0


def test_redis_database_config_uses_redis_section_values() -> None:
    database = run_node._redis_database_config(
        {
            "host": "127.0.0.10",
            "port": 6381,
            "username": "alice",
            "password": "secret",
        },
    )

    assert database.type == "redis"
    assert database.host == "127.0.0.10"
    assert database.port == 6381
    assert database.username == "alice"
    assert database.password == "secret"
