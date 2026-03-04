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

    def set_params_manager_factory(self, factory) -> None:  # noqa: ANN001
        self.params_manager_factory = factory


def test_attach_runtime_params_manager_wires_redis_backed_factory(monkeypatch) -> None:
    strategy = _DummyStrategy()
    redis_call: dict[str, object] = {}
    factory_call: dict[str, object] = {}
    redis_client = object()
    sentinel_factory = object()

    def _fake_redis(**kwargs):  # noqa: ANN003, ANN202
        redis_call.update(kwargs)
        return redis_client

    def _fake_params_manager_factory(**kwargs):  # noqa: ANN003, ANN202
        factory_call.update(kwargs)
        return sentinel_factory

    monkeypatch.setattr(run_node.redis, "Redis", _fake_redis)
    monkeypatch.setattr(run_node.runtime_params_mod, "params_manager_factory", _fake_params_manager_factory)

    run_node._attach_runtime_params_manager(  # noqa: SLF001
        strategy=strategy,  # type: ignore[arg-type]
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
