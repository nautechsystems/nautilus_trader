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

from nautilus_trader.network import SocketConfig
from nautilus_trader.network import TransportBackend
from nautilus_trader.network import WebSocketConfig


def test_socket_config_readback_keeps_handler_internal() -> None:
    config = SocketConfig(
        url="localhost:1234",
        ssl=True,
        suffix=b"\r\n",
        handler=lambda _: None,
        heartbeat=(1, b"PING"),
        reconnect_timeout_ms=2,
        reconnect_delay_initial_ms=3,
        reconnect_delay_max_ms=4,
        reconnect_backoff_factor=1.5,
        reconnect_jitter_ms=5,
        connection_max_retries=6,
        reconnect_max_attempts=7,
        idle_timeout_ms=8,
        certs_dir="/certs",
    )

    assert config.url == "localhost:1234"
    assert config.ssl is True
    assert config.suffix == b"\r\n"
    assert config.has_handler is True
    assert not hasattr(config, "handler")
    assert config.heartbeat == (1, b"PING")
    assert config.reconnect_timeout_ms == 2
    assert config.reconnect_delay_initial_ms == 3
    assert config.reconnect_delay_max_ms == 4
    assert config.reconnect_backoff_factor == 1.5
    assert config.reconnect_jitter_ms == 5
    assert config.connection_max_retries == 6
    assert config.reconnect_max_attempts == 7
    assert config.idle_timeout_ms == 8
    assert config.certs_dir == "/certs"


def test_websocket_config_readback_redacts_header_and_proxy_values() -> None:
    config = WebSocketConfig(
        url="wss://example.com/ws",
        headers=[("Authorization", "Bearer secret"), ("X-Trace", "trace")],
        heartbeat=1,
        heartbeat_msg="PING",
        reconnect_timeout_ms=2,
        reconnect_delay_initial_ms=3,
        reconnect_delay_max_ms=4,
        reconnect_backoff_factor=1.5,
        reconnect_jitter_ms=5,
        reconnect_max_attempts=6,
        idle_timeout_ms=7,
        proxy_url="http://user:password@proxy.example.com",
        backend=TransportBackend.TUNGSTENITE,
    )

    assert config.url == "wss://example.com/ws"
    assert config.header_names == ["Authorization", "X-Trace"]
    assert not hasattr(config, "headers")
    assert config.heartbeat == 1
    assert config.heartbeat_msg == "PING"
    assert config.reconnect_timeout_ms == 2
    assert config.reconnect_delay_initial_ms == 3
    assert config.reconnect_delay_max_ms == 4
    assert config.reconnect_backoff_factor == 1.5
    assert config.reconnect_jitter_ms == 5
    assert config.reconnect_max_attempts == 6
    assert config.idle_timeout_ms == 7
    assert config.backend == TransportBackend.TUNGSTENITE
    assert config.has_proxy_url is True
    assert not hasattr(config, "proxy_url")
