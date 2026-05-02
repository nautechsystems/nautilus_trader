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
r"""
Stress harness for the Python v1 ``TradingNode``.

Mirrors the scenario shape and output schema of the Rust v2 harness at
``crates/live/tests/stress.rs`` so results can be compared without translation.

# What this measures

Builds a ``TradingNode`` with no exchange clients, starts the kernel so the
engine asyncio queue tasks are running, then pushes synthetic trades through
``LiveDataEngine.process`` (the same entry point a live data client uses).
``LiveDataEngine.process`` enqueues into ``_data_queue``; the task spawned by
``LiveDataEngine.start`` drains the queue and calls ``_handle_data`` -> cache
write + ``msgbus.publish_c``. The harness waits for the drain to complete by
polling ``data_count``, then snapshots ``MessageBus`` counters.

This is the v1 analogue of the Rust v2 ``stress_trade_burst`` (which drains a
tokio mpsc channel via the biased ``select!`` loop and the same publish path).
The two architectures differ enough that the absolute numbers should be read as
"how does the runtime under each language stack up end-to-end," not as a
microbenchmark of any one component.

A cancel-starvation scenario is also included. v1's data and execution engines
each have their own asyncio queue and queue-drain task, so the question is
whether asyncio scheduling fairness lets cancels through under data load (in
contrast to the Rust v2 single biased ``select!`` which gives data hard
priority over exec commands).

# Running

These tests are skipped by default. Set ``NAUTILUS_STRESS=1`` to run, and
optionally ``NAUTILUS_STRESS_SCALE=N`` to multiply the burst count (default 1):

```bash
NAUTILUS_STRESS=1 pytest tests/performance_tests/test_perf_stress_node.py -s
NAUTILUS_STRESS=1 NAUTILUS_STRESS_SCALE=10 pytest \\
    tests/performance_tests/test_perf_stress_node.py -s
```

The ``-s`` flag is required so the single-line summary reaches stdout.

"""

from __future__ import annotations

import asyncio
import os
import time

import pytest

from nautilus_trader.common import Environment
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.live.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


TRADE_BURST_COUNT = 100_000
CANCEL_STARVATION_COUNT = 1_000
CANCEL_STARVATION_TRADES = 200_000


def _stress_scale() -> int:
    raw = os.environ.get("NAUTILUS_STRESS_SCALE")
    if raw is None:
        return 1
    try:
        value = int(raw)
    except ValueError:
        return 1
    return value if value > 0 else 1


def _sample_trade() -> TradeTick:
    return TradeTick(
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
        price=Price.from_str("1.10000"),
        size=Quantity.from_int(100_000),
        aggressor_side=AggressorSide.BUYER,
        trade_id=TradeId("123456"),
        ts_event=0,
        ts_init=0,
    )


def _sample_cancel(seq: int, ts_init: int) -> CancelOrder:
    return CancelOrder(
        trader_id=TraderId("STRESS-001"),
        strategy_id=StrategyId("S-STRESS"),
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
        client_order_id=ClientOrderId(f"O-{seq:08d}"),
        venue_order_id=None,
        command_id=UUID4(),
        ts_init=ts_init,
    )


def _percentile(sorted_us: list[int], pct: float) -> int:
    if not sorted_us:
        return 0
    idx = round((len(sorted_us) - 1) * pct)
    return sorted_us[min(idx, len(sorted_us) - 1)]


def _build_node() -> TradingNode:
    # SANDBOX rather than LIVE so bypass_logging is permitted: keeps the run
    # quiet and removes log overhead from the measured path. The engines and
    # message bus are the same in both environments.
    config = TradingNodeConfig(
        environment=Environment.SANDBOX,
        trader_id="STRESS-001",
        logging=LoggingConfig(bypass_logging=True),
    )
    node = TradingNode(config=config)
    node.build()
    return node


@pytest.mark.skipif(
    not os.environ.get("NAUTILUS_STRESS"),
    reason="set NAUTILUS_STRESS=1 to run",
)
@pytest.mark.asyncio
async def test_stress_trade_burst() -> None:
    total = TRADE_BURST_COUNT * _stress_scale()
    node = _build_node()
    await node.kernel.start_async()

    msgbus = node.kernel.msgbus
    data_engine = node.kernel.data_engine
    trade = _sample_trade()

    sent_before = msgbus.sent_count
    pub_before = msgbus.pub_count
    req_before = msgbus.req_count
    res_before = msgbus.res_count
    data_before = data_engine.data_count

    start_ns = time.perf_counter_ns()

    for _ in range(total):
        data_engine.process(trade)

    # Drain: yield to the event loop until the data queue task has handled
    # all pushed trades. data_count increments inside _handle_data, which is
    # the per-message work of the queue task.
    target_data_count = data_before + total
    while data_engine.data_count < target_data_count:
        await asyncio.sleep(0)
    elapsed_ns = time.perf_counter_ns() - start_ns

    sent_delta = msgbus.sent_count - sent_before
    pub_delta = msgbus.pub_count - pub_before
    req_delta = msgbus.req_count - req_before
    res_delta = msgbus.res_count - res_before

    elapsed_ms = elapsed_ns / 1_000_000
    mean_msg_s = total * 1_000_000_000 / elapsed_ns

    print(
        f"scenario=trade_burst messages={total} "
        f"elapsed_ms={elapsed_ms:.0f} mean_msg_s={mean_msg_s:.0f} "
        f"counter.msgbus.sent={sent_delta} counter.msgbus.pub={pub_delta} "
        f"counter.msgbus.req={req_delta} counter.msgbus.res={res_delta}",
    )

    await node.stop_async()
    node.dispose()


@pytest.mark.skipif(
    not os.environ.get("NAUTILUS_STRESS"),
    reason="set NAUTILUS_STRESS=1 to run",
)
@pytest.mark.asyncio
async def test_stress_cancel_starvation() -> None:
    scale = _stress_scale()
    cancels = CANCEL_STARVATION_COUNT * scale
    total_trades = CANCEL_STARVATION_TRADES * scale
    trade_step = total_trades // cancels

    node = _build_node()
    await node.kernel.start_async()

    clock = node.kernel.clock
    msgbus = node.kernel.msgbus
    data_engine = node.kernel.data_engine
    exec_engine = node.kernel.exec_engine
    trade = _sample_trade()

    sent_before = msgbus.sent_count
    pub_before = msgbus.pub_count
    cmd_before = exec_engine.command_count

    latencies_us: list[int] = []
    trades_sent = 0
    start_ns = time.perf_counter_ns()

    for seq in range(cancels):
        for _ in range(trade_step):
            data_engine.process(trade)
            trades_sent += 1

        # Time the cancel as `now - cancel.ts_init`, mirroring how production
        # code times a strategy action: from when the command was created to
        # when the runtime dispatched it. v1 has separate asyncio queue tasks
        # per engine, so this measures scheduling fairness while the data
        # queue task is also draining.
        target = exec_engine.command_count + 1
        ts_init = clock.timestamp_ns()
        exec_engine.execute(_sample_cancel(seq, ts_init))

        while exec_engine.command_count < target:
            await asyncio.sleep(0)
        latencies_us.append((clock.timestamp_ns() - ts_init) // 1_000)

    # Drain residual data trades that the data queue task hasn't handled yet.
    while data_engine.data_count < trades_sent:
        await asyncio.sleep(0)
    elapsed_ns = time.perf_counter_ns() - start_ns

    sent_delta = msgbus.sent_count - sent_before
    pub_delta = msgbus.pub_count - pub_before
    cmd_delta = exec_engine.command_count - cmd_before

    latencies_us.sort()
    lat_min = latencies_us[0] if latencies_us else 0
    p50 = _percentile(latencies_us, 0.50)
    p95 = _percentile(latencies_us, 0.95)
    p99 = _percentile(latencies_us, 0.99)
    p999 = _percentile(latencies_us, 0.999)
    lat_max = latencies_us[-1] if latencies_us else 0

    print(
        f"scenario=cancel_starvation cancels={cancels} trades={trades_sent} "
        f"elapsed_ms={elapsed_ns / 1_000_000:.0f} "
        f"counter.msgbus.sent={sent_delta} counter.msgbus.pub={pub_delta} "
        f"counter.execution.command={cmd_delta} "
        f"latency.cancel.min_us={lat_min} latency.cancel.p50_us={p50} "
        f"latency.cancel.p95_us={p95} latency.cancel.p99_us={p99} "
        f"latency.cancel.p999_us={p999} latency.cancel.max_us={lat_max}",
    )

    await node.stop_async()
    node.dispose()
