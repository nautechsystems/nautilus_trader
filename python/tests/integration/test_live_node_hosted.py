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
Contract and lifecycle tests for running a `LiveNode` on a caller-owned asyncio loop.
"""

import asyncio
import gc
import signal
import threading
import time
from datetime import UTC
from datetime import datetime
from datetime import timedelta

import pytest
from strategies.acceptance import DualTimer
from strategies.acceptance import DualTimerConfig

from nautilus_trader.common import Cache
from nautilus_trader.common import Environment
from nautilus_trader.infrastructure import RedisCacheConfig
from nautilus_trader.live import LiveExecutionEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import LiveNodeHandle
from nautilus_trader.live import NodeState
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import TraderId
from nautilus_trader.portfolio import Portfolio


STOP_TIMEOUT_SECS = 10.0

pytestmark = pytest.mark.asyncio


def build_node(trader_id: str) -> LiveNode:
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


async def stop_and_await(handle: LiveNodeHandle, task: asyncio.Task) -> None:
    handle.stop()
    async with asyncio.timeout(STOP_TIMEOUT_SECS):
        await task


async def test_run_async_completes_after_handle_stop():
    node = build_node("HOSTED-001")
    handle = node.handle()

    assert handle.state == NodeState.IDLE
    assert handle.is_running is False
    assert handle.is_stopping is False

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    assert handle.is_running is True
    assert handle.state == NodeState.RUNNING

    await stop_and_await(handle, task)

    assert handle.is_running is False
    assert handle.is_stopping is True
    assert handle.state == NodeState.STOPPED
    assert task.done() is True
    assert task.exception() is None


async def test_run_async_requires_the_node_and_rejects_a_second_run():
    node = build_node("HOSTED-002")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    # The run owns the node, so a concurrent run is refused.
    with pytest.raises(RuntimeError, match="run_async"):
        node.run_async()

    await stop_and_await(handle, task)

    # The node comes back once the run finishes, but a node runs only once.
    with pytest.raises(RuntimeError, match="cannot be run from state Stopped"):
        node.run_async()


async def test_node_accessors_fail_clearly_once_the_run_owns_the_node():
    node = build_node("HOSTED-003")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    for accessor in ("cache", "portfolio", "trader_id", "environment"):
        with pytest.raises(RuntimeError, match="run_async"):
            getattr(node, accessor)

    with pytest.raises(RuntimeError, match="run_async"):
        node.stop()

    # `is_running` answers from the handle, so it stays truthful while the run holds the node.
    assert node.is_running is True

    await stop_and_await(handle, task)


async def test_handles_captured_before_the_run_stay_usable_during_it():
    node = build_node("HOSTED-004")
    cache = node.cache
    portfolio = node.portfolio
    handle = node.handle()

    assert isinstance(cache, Cache)
    assert isinstance(portfolio, Portfolio)

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    assert cache.orders() == []
    assert cache.positions() == []
    assert portfolio.is_initialized() is False  # No execution clients configured
    assert handle.state == NodeState.RUNNING

    await stop_and_await(handle, task)

    assert cache.orders() == []


async def test_run_async_returns_a_coroutine_accepted_by_create_task():
    node = build_node("HOSTED-005")
    handle = node.handle()
    run = node.run_async()

    assert asyncio.iscoroutine(run) is True

    # `create_task` rejects non-coroutine awaitables, so this is the contract users rely on.
    task = asyncio.create_task(run)
    await asyncio.sleep(0.1)

    await stop_and_await(handle, task)


async def test_cancellation_shuts_down_then_propagates():
    node = build_node("HOSTED-006")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)
    assert handle.state == NodeState.RUNNING

    task.cancel()

    # Cancellation requests a graceful stop and only then propagates, so cleanup completes
    # without the caller losing normal cancellation semantics.
    with pytest.raises(asyncio.CancelledError):
        await task

    assert task.cancelled() is True
    assert handle.state == NodeState.STOPPED
    assert handle.is_running is False


async def test_repeated_cancellation_does_not_abandon_the_node():
    node = build_node("HOSTED-007")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    for _ in range(3):
        task.cancel()
        await asyncio.sleep(0)

    with pytest.raises(asyncio.CancelledError):
        await task

    # Repeated cancellation must not drop a running node mid-shutdown.
    assert handle.state == NodeState.STOPPED


async def test_asyncio_timeout_around_a_run_reports_timeout():
    node = build_node("HOSTED-013")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    # `asyncio.timeout` cancels the task and needs the cancellation back to raise TimeoutError.
    with pytest.raises(TimeoutError):
        async with asyncio.timeout(0.2):
            await task

    assert handle.state == NodeState.STOPPED


async def test_unawaited_run_returns_the_node_to_the_wrapper():
    node = build_node("HOSTED-014")

    run = node.run_async()

    # The node is lent to the run as soon as `run_async` returns.
    with pytest.raises(RuntimeError, match="run_async"):
        _ = node.trader_id

    run.close()
    del run
    gc.collect()

    # Dropping an unstarted run must not strand the node.
    assert node.trader_id == TraderId("HOSTED-014")
    node.dispose()


async def test_second_node_on_the_same_loop_is_rejected():
    first = build_node("HOSTED-015")
    second = build_node("HOSTED-016")
    handle = first.handle()

    task = asyncio.create_task(first.run_async())
    await asyncio.sleep(0.1)

    # Thread-local runner senders and msgbus mean two hosted nodes would cross-wire silently.
    with pytest.raises(RuntimeError, match="already running on this event loop"):
        second.run_async()

    await stop_and_await(handle, task)

    # The guard clears once the first run finishes, so the loop can host another node.
    second_handle = second.handle()
    second_task = asyncio.create_task(second.run_async())
    await asyncio.sleep(0.1)

    assert second_handle.state == NodeState.RUNNING

    await stop_and_await(second_handle, second_task)

    assert second_handle.state == NodeState.STOPPED


async def test_handle_remains_available_while_the_run_owns_the_node():
    node = build_node("HOSTED-017")

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    # The consumed-node error tells users to reach for `handle()`, so it must not fail too.
    handle = node.handle()
    assert handle.state == NodeState.RUNNING

    await stop_and_await(handle, task)


async def test_hosted_run_leaves_the_python_sigint_handler_untouched():
    node = build_node("HOSTED-008")
    handle = node.handle()
    original = signal.getsignal(signal.SIGINT)

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    assert signal.getsignal(signal.SIGINT) is original

    await stop_and_await(handle, task)

    assert signal.getsignal(signal.SIGINT) is original


async def test_host_loop_keeps_running_while_the_node_runs():
    node = build_node("HOSTED-009")
    handle = node.handle()
    ticks = 0

    async def host_work() -> None:
        nonlocal ticks
        while True:
            ticks += 1
            await asyncio.sleep(0.005)

    worker = asyncio.create_task(host_work())
    task = asyncio.create_task(node.run_async())

    await asyncio.sleep(0.3)
    assert ticks > 5, "the node starved the host loop"

    await stop_and_await(handle, task)
    worker.cancel()


async def test_strategy_timers_and_orders_are_serviced_by_the_host_loop():
    node = build_node("HOSTED-010")
    cache = node.cache
    handle = node.handle()
    strategy = DualTimer(
        DualTimerConfig(
            instrument_id="AUD/USD.SIM",
            trade_size="100000",
            alert_iso=(datetime.now(UTC) + timedelta(seconds=1)).isoformat(),
        ),
    )
    node.add_strategy(strategy)

    task = asyncio.create_task(node.run_async())

    async with asyncio.timeout(10.0):
        while not (strategy.fired_a and strategy.fired_b):  # noqa: ASYNC110
            await asyncio.sleep(0.01)

    orders = cache.orders(strategy_id=strategy.strategy_id)

    assert strategy.fired_a is True
    assert strategy.fired_b is True
    assert len(orders) == 2
    assert {order.side for order in orders} == {OrderSide.BUY, OrderSide.SELL}
    assert {order.status for order in orders} == {OrderStatus.DENIED}

    await stop_and_await(handle, task)

    assert strategy.is_stopped() is True


async def test_handle_stop_is_idempotent_and_safe_before_the_run():
    node = build_node("HOSTED-012")
    handle = node.handle()

    handle.stop()
    handle.stop()

    assert handle.is_stopping is True

    task = asyncio.create_task(node.run_async())

    async with asyncio.timeout(10.0):
        await task

    assert handle.state == NodeState.STOPPED


@pytest.mark.filterwarnings("ignore::RuntimeWarning")
def test_run_async_outside_an_event_loop_is_rejected():
    """
    Calling `run_async` without a running loop must fail rather than silently doing
    nothing.
    """
    node = build_node("HOSTED-011")

    try:
        with pytest.raises(RuntimeError, match="event loop"):
            node.run_async()

        # The node is still owned by the wrapper, so the failure left nothing consumed.
        assert node.trader_id == TraderId("HOSTED-011")
    finally:
        node.dispose()


async def test_cancellation_during_startup_stops_cleanly():
    """
    Cancelling before the node reaches `Running` must still leave it terminal, not half-
    started.
    """
    node = build_node("HOSTED-021")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0)  # Let startup begin without letting it finish
    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        async with asyncio.timeout(10.0):
            await task

    assert handle.is_running is False
    assert handle.state in (NodeState.STOPPED, NodeState.IDLE)

    # The node came back, so cleanup still works.
    node.dispose()


async def test_handle_stop_from_a_foreign_thread():
    """
    The handle is documented as thread-safe, including from signal handlers.
    """
    node = build_node("HOSTED-018")
    handle = node.handle()

    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)
    assert handle.state == NodeState.RUNNING

    stopper = threading.Thread(target=handle.stop)
    stopper.start()
    stopper.join(timeout=5.0)

    async with asyncio.timeout(10.0):
        await task

    assert handle.state == NodeState.STOPPED


async def test_await_protocol_terminates_with_stop_iteration():
    """
    Drive the awaitable directly, since asyncio's 100ms stop check can mask a bad
    terminal signal.
    """
    node = build_node("HOSTED-019")
    handle = node.handle()
    run = node.run_async()

    suspended = run.send(None)
    assert suspended is not None, "the run should suspend rather than finish immediately"

    handle.stop()

    # Step until completion; the terminal signal must be StopIteration, never a returned value.
    # PEP 479 turns a StopIteration escaping a coroutine into RuntimeError, so catch it here.
    async def drive_to_completion() -> bool:
        async with asyncio.timeout(10.0):
            while True:
                try:
                    yielded = run.send(None)
                except StopIteration:
                    return True
                assert yielded is not None, "the run yielded nothing while still running"
                await asyncio.sleep(0.01)

    assert await drive_to_completion() is True

    assert handle.state == NodeState.STOPPED


async def test_cache_database_backing_is_rejected_on_a_host_loop():
    """
    The Redis and SQL backings block their calling thread, which would stall the host
    loop.
    """
    node = (
        LiveNode.builder("TEST", TraderId("HOSTED-020"), Environment.SANDBOX)
        .with_cache_database_factory(RedisCacheConfig())
        .with_reconciliation(False)
        .with_timeout_connection(0)
        .build()
    )

    try:
        with pytest.raises(RuntimeError, match="cache database backing is not supported"):
            node.run_async()
    finally:
        node.dispose()


async def test_thrown_exception_stops_the_node_then_propagates():
    """
    An injected exception drives shutdown to completion first, then re-raises unchanged.
    """
    node = build_node("HOSTED-022")
    handle = node.handle()
    run = node.run_async()
    assert run.send(None) is not None

    async def drive_after_throw() -> None:
        run.throw(ValueError("boom"))
        async with asyncio.timeout(10.0):
            while True:
                run.send(None)
                await asyncio.sleep(0.01)

    with pytest.raises(ValueError, match="boom"):
        await drive_after_throw()

    # Stopped before the exception surfaces, not merely asked to stop.
    assert handle.state == NodeState.STOPPED
    assert handle.is_running is False
    node.dispose()


def test_owned_run_installs_and_restores_the_python_signal_handler():
    """
    Guards `run()`'s Python-level SIGINT handling, which this patch's mode split
    touches.

    This does not observe the Rust-level listeners that `NodeRunMode` gates; those are installed
    with sigaction and are invisible to `signal.getsignal`.

    """
    node = build_node("HOSTED-023")
    handle = node.handle()
    original = signal.getsignal(signal.SIGINT)
    observed: dict[str, object] = {}

    def observe_then_stop() -> None:
        deadline = time.monotonic() + 30.0
        while not handle.is_running and time.monotonic() < deadline:
            time.sleep(0.01)
        observed["during_run"] = signal.getsignal(signal.SIGINT)
        handle.stop()

    watcher = threading.Thread(target=observe_then_stop, daemon=True)
    watcher.start()

    node.run()  # Owned mode blocks this thread until the handle stops it
    watcher.join(timeout=30.0)

    assert observed["during_run"] is not original, "owned mode did not install a SIGINT handler"
    assert signal.getsignal(signal.SIGINT) is original, "owned mode did not restore the handler"

    node.dispose()


async def test_closing_a_run_on_a_live_loop_does_not_block_it():
    """
    `close` must not drive shutdown inline while the host loop is running.

    Driving inline there would stall every host callback for up to the close-drive
    timeout.

    """
    node = build_node("HOSTED-025")
    handle = node.handle()
    run = node.run_async()
    assert run.send(None) is not None

    async with asyncio.timeout(10.0):
        while not handle.is_running:
            run.send(None)
            await asyncio.sleep(0.01)

    started = time.perf_counter()
    run.close()
    elapsed = time.perf_counter() - started

    assert elapsed < 1.0, f"close blocked the loop for {elapsed:.2f}s"
    assert handle.is_stopping is True

    # The node comes back even though shutdown was not driven to completion here.
    node.dispose()


def test_close_after_the_loop_stops_drives_shutdown_inline():
    """
    With no running loop, `close` is the only thing that can finish shutdown, so it
    drives it.

    This is the host-loop-teardown half of the contract: a loop abandoned without cancelling its
    tasks leaves the run suspended, and nothing else will ever resume it.

    """
    node = build_node("HOSTED-026")
    handle = node.handle()
    loop = asyncio.new_event_loop()
    holder: dict[str, object] = {}

    async def start_and_stop_the_loop() -> None:
        run = node.run_async()
        holder["run"] = run
        async with asyncio.timeout(10.0):
            while not handle.is_running:
                run.send(None)
                await asyncio.sleep(0.01)
        loop.stop()

    try:
        holder["task"] = loop.create_task(start_and_stop_the_loop())
        loop.run_forever()

        assert loop.is_running() is False
        assert handle.is_running is True, "the run should still be suspended mid-flight"

        holder["run"].close()

        assert handle.state == NodeState.STOPPED, "close did not drive shutdown inline"
        assert node.trader_id == TraderId("HOSTED-026"), "the node was not returned"
    finally:
        loop.close()
        node.dispose()


async def test_a_late_drop_does_not_release_another_runs_guard():
    """
    Only the run that set the per-loop guard may release it.

    Releasing on every restore would let a completed run's delayed collection reopen the
    gate while a newer run is live, which is the cross-wiring the guard exists to
    prevent.

    """
    first = build_node("HOSTED-027")
    second = build_node("HOSTED-028")
    third = build_node("HOSTED-029")
    first_handle, second_handle = first.handle(), second.handle()

    # Keep a strong reference so the completed run is not collected yet.
    first_run = first.run_async()
    first_task = asyncio.create_task(first_run)
    await asyncio.sleep(0.1)
    first_handle.stop()
    async with asyncio.timeout(10.0):
        await first_task

    second_task = asyncio.create_task(second.run_async())
    await asyncio.sleep(0.1)
    assert second_handle.state == NodeState.RUNNING

    # Collect the first run now, while the second is live.
    del first_run, first_task
    gc.collect()

    with pytest.raises(RuntimeError, match="already running on this event loop"):
        third.run_async()

    await stop_and_await(second_handle, second_task)
    first.dispose()
    third.dispose()


async def test_concurrent_stop_calls_from_many_threads_resolve_once():
    """
    The handle is documented as safe from any thread, including a signal handler.

    A stop storm must resolve the run exactly once and leave the node terminal, rather
    than racing the shutdown sequence or resolving the awaiting task more than once.

    """
    node = build_node("HOSTED-030")
    handle = node.handle()

    # A double resolve surfaces as InvalidStateError inside a loop callback, which asyncio logs
    # and swallows, so collect those rather than relying on the await to fail.
    loop_errors: list[dict] = []
    loop = asyncio.get_running_loop()
    previous_handler = loop.get_exception_handler()
    loop.set_exception_handler(lambda _loop, context: loop_errors.append(context))

    try:
        task = asyncio.create_task(node.run_async())
        await asyncio.sleep(0.1)
        assert handle.state == NodeState.RUNNING

        def spam_stop() -> None:
            for _ in range(200):
                handle.stop()

        threads = [threading.Thread(target=spam_stop, daemon=True) for _ in range(8)]
        for thread in threads:
            thread.start()
        for thread in threads:
            # Joining on the loop thread would block the node it is waiting for.
            await asyncio.to_thread(thread.join, 10.0)

        async with asyncio.timeout(20.0):
            await task
    finally:
        # The loop is session scoped, so a leaked collector would swallow later tests' errors.
        loop.set_exception_handler(previous_handler)

    assert handle.state == NodeState.STOPPED
    assert handle.is_running is False
    assert task.exception() is None
    assert loop_errors == [], f"loop callbacks raised: {loop_errors}"

    node.dispose()


async def test_repeated_node_lifecycles_on_one_loop():
    """
    Running nodes back to back on one loop must not wedge the per-loop guard or strand a
    node.

    This is the shape that catches a regression in guard release or node restoration,
    where a completed run leaves state behind that blocks or corrupts the next one.

    """
    observed: list[tuple[NodeState, NodeState]] = []

    for i in range(5):
        node = build_node(f"HOSTED-CYCLE-{i}")
        handle = node.handle()

        task = asyncio.create_task(node.run_async())
        async with asyncio.timeout(15.0):
            while not handle.is_running:  # noqa: ASYNC110
                await asyncio.sleep(0.01)

        running = handle.state
        handle.stop()
        async with asyncio.timeout(15.0):
            await task

        # The node must come back to its wrapper, not merely stop.
        assert node.trader_id == TraderId(f"HOSTED-CYCLE-{i}")

        observed.append((running, handle.state))
        node.dispose()

    assert observed == [(NodeState.RUNNING, NodeState.STOPPED)] * 5
