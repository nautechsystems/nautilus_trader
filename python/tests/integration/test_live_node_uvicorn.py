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
Runs a `LiveNode` inside a uvicorn ASGI server, the deployment shape v1 supported.

A bare ASGI app exercises the same lifespan and loop-ownership path a FastAPI
application does, without adding a framework dependency.

"""

import asyncio
import json
import urllib.request

import pytest


uvicorn = pytest.importorskip("uvicorn", reason="uvicorn is an optional test dependency")

from nautilus_trader.common import Environment
from nautilus_trader.live import LiveExecutionEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import NodeState
from nautilus_trader.model import TraderId


pytestmark = pytest.mark.asyncio


def build_node(trader_id: str) -> LiveNode:
    """
    Build node.
    """
    return LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId(trader_id),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
            timeout_connection_secs=0,
            timeout_reconciliation_secs=0,
            timeout_portfolio_secs=0,
            timeout_disconnection_secs=0,
            delay_post_stop_secs=0,
            timeout_shutdown_secs=0,
        ),
    )


def make_app(node: LiveNode, record: dict) -> object:
    """
    Build an ASGI app owning the node across the server lifespan.
    """
    handle = node.handle()
    cache = node.cache
    record["handle"] = handle

    async def app(scope: object, receive: object, send: object) -> None:
        """
        App.
        """
        if scope["type"] == "lifespan":
            while True:
                message = await receive()
                if message["type"] == "lifespan.startup":
                    record["task"] = asyncio.create_task(node.run_async())
                    await asyncio.sleep(0.1)
                    await send({"type": "lifespan.startup.complete"})
                elif message["type"] == "lifespan.shutdown":
                    handle.stop()
                    async with asyncio.timeout(10.0):
                        await record["task"]
                    await send({"type": "lifespan.shutdown.complete"})
                    return
        elif scope["type"] == "http":
            body = json.dumps(
                {
                    "state": str(handle.state),
                    "running": handle.is_running,
                    "trader_id": str(record["trader_id"]),
                    "orders": len(cache.orders()),
                },
            ).encode()
            await send(
                {
                    "type": "http.response.start",
                    "status": 200,
                    "headers": [[b"content-type", b"application/json"]],
                },
            )
            await send({"type": "http.response.body", "body": body})

    return app


async def serve(app: object) -> tuple:
    """
    Serve.
    """
    config = uvicorn.Config(app, host="127.0.0.1", port=0, log_level="warning", lifespan="on")
    server = uvicorn.Server(config)
    server.install_signal_handlers = lambda: None  # The test owns the loop and its signals

    task = asyncio.create_task(server.serve())

    async with asyncio.timeout(20.0):
        while not server.started:
            await asyncio.sleep(0.01)

    port = server.servers[0].sockets[0].getsockname()[1]
    return server, task, port


async def get_json(port: int, path: str = "/") -> dict:
    """
    Get json.
    """

    def request() -> dict:
        """
        Request.
        """
        with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=10) as response:
            return json.loads(response.read())

    return await asyncio.to_thread(request)


async def test_node_runs_inside_uvicorn_and_serves_requests() -> None:
    """
    Test node runs inside uvicorn and serves requests.
    """
    node = build_node("UVICORN-001")
    record = {"trader_id": node.trader_id}
    server, server_task, port = await serve(make_app(node, record))

    try:
        first = await get_json(port)

        assert first["state"] == "NodeState.RUNNING"
        assert first["running"] is True
        assert first["trader_id"] == "UVICORN-001"
        assert first["orders"] == 0

        # The server keeps serving while the node runs, so neither starves the other.
        for _ in range(5):
            assert (await get_json(port))["state"] == "NodeState.RUNNING"
    finally:
        server.should_exit = True
        async with asyncio.timeout(20.0):
            await server_task

    assert record["handle"].state == NodeState.STOPPED
    assert record["handle"].is_running is False
    assert record["task"].done() is True
    assert record["task"].exception() is None


async def test_uvicorn_shutdown_stops_the_node_through_the_lifespan() -> None:
    """
    Test uvicorn shutdown stops the node through the lifespan.
    """
    node = build_node("UVICORN-002")
    record = {"trader_id": node.trader_id}
    server, server_task, port = await serve(make_app(node, record))

    assert (await get_json(port))["running"] is True

    server.should_exit = True
    async with asyncio.timeout(20.0):
        await server_task

    # Shutdown ran the node's full stop sequence rather than dropping it mid-flight.
    assert record["handle"].state == NodeState.STOPPED
    assert record["task"].exception() is None


async def test_http_requests_stay_responsive_under_repeated_polling() -> None:
    """
    Test http requests stay responsive under repeated polling.
    """
    node = build_node("UVICORN-003")
    record = {"trader_id": node.trader_id}
    server, server_task, port = await serve(make_app(node, record))

    try:
        latencies = []
        loop = asyncio.get_running_loop()
        for _ in range(10):
            started = loop.time()
            await get_json(port)
            latencies.append(loop.time() - started)

        # The node yields to the loop, so no request waits on a long dispatch batch.
        assert max(latencies) < 0.25, f"slow request under a running node: {max(latencies):.3f}s"
    finally:
        server.should_exit = True
        async with asyncio.timeout(20.0):
            await server_task
