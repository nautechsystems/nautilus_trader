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
Tests Python client operation ownership and ordering.
"""

import asyncio
import gc
import inspect
import subprocess
import sys
import textwrap
import weakref

import pytest

from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime


class Client:
    """
    Implement the client hooks exercised by this scenario.
    """

    client_id = "TEST"


@pytest.mark.asyncio
async def test_pending_task_retains_client_until_result_is_retrieved() -> None:
    """
    Pending task retains client until result is retrieved.
    """
    client = Client()
    retained = weakref.ref(client)
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.create_task(asyncio.sleep(0, result=31))
    del client

    assert retained() is not None
    result = await task

    assert result == 31
    assert runtime.complete is True
    assert retained() is None


def test_task_before_binding_closes_coroutine() -> None:
    """
    Task before binding closes coroutine.
    """
    client = Client()
    runtime = ClientRuntime(client)
    coroutine = asyncio.sleep(0)

    with pytest.raises(RuntimeError, match="not bound"):
        runtime.create_task(coroutine)

    assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED
    assert runtime.complete is True


@pytest.mark.asyncio
@pytest.mark.parametrize("eager", [False, True])
async def test_task_admission_never_runs_user_code_inline(eager) -> None:
    """
    Task admission never runs user code inline.
    """
    client = Client()
    runtime = ClientRuntime(client)
    loop = asyncio.get_running_loop()
    runtime.bind(loop)
    entered = []

    async def operation():
        entered.append("entered")
        return 17

    factory = loop.get_task_factory()
    try:
        if eager:
            loop.set_task_factory(asyncio.eager_task_factory)
        task = runtime.create_task(operation())
        assert entered == []
        result = await task
    finally:
        loop.set_task_factory(factory)

    assert result == 17
    assert entered == ["entered"]
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_cancel_before_first_poll_closes_owned_coroutine() -> None:
    """
    Cancel before first poll closes owned coroutine.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    coroutine = asyncio.sleep(60)
    task = runtime.create_task(coroutine)
    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        await task

    assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_command_dispatch_preserves_subscription_order() -> None:
    """
    Command dispatch preserves subscription order.
    """
    entered = asyncio.Event()
    release = asyncio.Event()
    calls = []

    class OrderedClient(Client):
        async def subscribe(self):
            calls.append("subscribe")
            entered.set()
            await release.wait()
            calls.append("subscribed")

        async def unsubscribe(self):
            calls.append("unsubscribe")

    client = OrderedClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("subscribe")
    runtime.admit("unsubscribe")
    await entered.wait()
    assert calls == ["subscribe"]
    release.set()
    await command_task()

    assert calls == ["subscribe", "subscribed", "unsubscribe"]
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_cancel_request_retains_nonterminal_task() -> None:
    """
    Cancel request retains nonterminal task.
    """
    entered = asyncio.Event()
    cancelled = asyncio.Event()
    release = asyncio.Event()
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())

    async def resistant():
        entered.set()
        try:
            await asyncio.Future()
        except asyncio.CancelledError:
            cancelled.set()
            await release.wait()
        return 23

    task = runtime.create_task(resistant())
    await entered.wait()
    runtime.dispose()
    await cancelled.wait()
    assert task.done() is False
    assert runtime.complete is False
    release.set()
    result = await task

    assert result == 23
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_disconnect_reaches_hook_before_waiting_for_active_command() -> None:
    """
    Disconnect reaches hook before waiting for active command.
    """
    entered = asyncio.Event()
    released = asyncio.Event()
    calls = []

    class DisconnectClient(Client):
        async def subscribe(self):
            entered.set()
            try:
                await asyncio.Future()
            except asyncio.CancelledError:
                await released.wait()

        async def unsubscribe(self):
            calls.append("unsubscribe")

        async def _disconnect(self):
            calls.append("disconnect")
            released.set()

    client = DisconnectClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("subscribe")
    runtime.admit("unsubscribe")
    await entered.wait()
    async with asyncio.timeout(1):
        await runtime.lifecycle("disconnect")

    assert calls == ["disconnect"]
    assert runtime.connected is False
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_cancelled_disconnect_still_drains_background_work(native_log) -> None:
    """
    Drain background work even when the disconnect hook raises cancellation.
    """
    started = asyncio.Event()
    closed = asyncio.Event()

    class DisconnectClient(Client):
        async def _disconnect(self):
            raise asyncio.CancelledError

    async def background():
        started.set()
        try:
            await asyncio.Future()
        finally:
            closed.set()

    client = DisconnectClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.create_task(background())
    await started.wait()
    try:
        async with asyncio.timeout(1):
            with pytest.raises(asyncio.CancelledError):
                await runtime.lifecycle("disconnect")
        await asyncio.sleep(0)
        observed = runtime.complete, task.cancelled(), closed.is_set()
    finally:
        runtime.dispose()
        await asyncio.gather(task, return_exceptions=True)

    assert observed == (True, True, True)
    native_log.error.assert_not_called()


@pytest.mark.asyncio
@pytest.mark.parametrize("operation", ["command", "background"])
async def test_disconnect_rejects_new_work_before_hook_returns(operation) -> None:
    """
    Close admission before awaiting the adapter disconnect hook.
    """
    entered = asyncio.Event()
    release = asyncio.Event()
    calls = []

    class DisconnectClient(Client):
        async def _disconnect(self):
            entered.set()
            await release.wait()

        async def command(self):
            calls.append("command")

    client = DisconnectClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.lifecycle("disconnect")
    await entered.wait()
    coroutine = client.command() if operation == "background" else None
    try:
        if coroutine is None:
            with pytest.raises(RuntimeError, match="Client TEST is shutting down"):
                runtime.admit("command")
        else:
            with pytest.raises(RuntimeError, match="Client TEST is shutting down"):
                runtime.create_task(coroutine)
            assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED
    finally:
        release.set()
        await task

    assert calls == []
    assert runtime.connected is False
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_abandon_after_completion_reports_awaited_failure(native_log) -> None:
    """
    Report an abandoned failure after the completion callback releases ownership.
    """

    class FailingClient(Client):
        async def request(self):
            raise RuntimeError("Completed adapter failure")

    client = FailingClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.call_awaited("request")
    await asyncio.wait({task})

    assert runtime.complete is True
    assert native_log.error.call_args_list == []
    runtime.abandon(task)

    native_log.error.assert_called_once()
    message = native_log.error.call_args.args[0]
    assert message.startswith(
        "Client TEST operation TEST:request failed\nTraceback (most recent call last):",
    )
    assert "in request" in message
    assert message.endswith("RuntimeError: Completed adapter failure\n")


@pytest.mark.asyncio
async def test_abandoned_awaited_operation_reports_late_failure(native_log) -> None:
    """
    Abandoned awaited operation reports late failure.
    """
    entered = asyncio.Event()
    cancelled = asyncio.Event()
    release = asyncio.Event()

    class FailingClient(Client):
        async def _connect(self):
            entered.set()
            try:
                await asyncio.Future()
            except asyncio.CancelledError:
                cancelled.set()
                await release.wait()
                raise RuntimeError("Late adapter failure") from None

    client = FailingClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.lifecycle("connect")
    await entered.wait()
    runtime.abandon(task)
    await cancelled.wait()
    assert runtime.complete is False
    release.set()
    with pytest.raises(RuntimeError, match="Late adapter failure"):
        await task

    assert runtime.complete is True
    assert runtime.connected is False
    assert "Client TEST operation connect failed" in native_log.error.call_args.args[0]
    assert "Late adapter failure" in native_log.error.call_args.args[0]


@pytest.mark.asyncio
async def test_connect_completing_after_disposal_does_not_mark_connected() -> None:
    """
    Connect completing after disposal does not mark connected.
    """
    entered = asyncio.Event()

    class LateClient(Client):
        async def _connect(self):
            entered.set()
            try:
                await asyncio.Future()
            except asyncio.CancelledError:
                return

    client = LateClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.lifecycle("connect")
    await entered.wait()
    runtime.dispose()
    with pytest.raises(RuntimeError, match="connect finished after shutdown"):
        await task

    assert runtime.connected is False
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_full_command_queue_rejects_without_displacing_admitted_commands() -> None:
    """
    Full command queue rejects without displacing admitted commands.
    """
    calls = []

    class QueuedClient(Client):
        async def command(self, value):
            calls.append(value)

    client = QueuedClient()
    runtime = ClientRuntime(client, capacity=2)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command", (17,))
    runtime.admit("command", (29,))
    with pytest.raises(RuntimeError, match="command queue is full"):
        runtime.admit("command", (43,))
    await command_task()
    runtime.admit("command", (61,))
    await command_task()

    assert calls == [17, 29, 61]
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_failed_command_is_reported_and_next_command_runs(native_log) -> None:
    """
    Failed command is reported and next command runs.
    """
    calls = []

    class FailingClient(Client):
        async def command(self, value):
            calls.append(value)
            if value == 17:
                raise ValueError("Rejected command 17")

    client = FailingClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command", (17,))
    runtime.admit("command", (29,))
    await command_task()

    assert calls == [17, 29]
    assert runtime.complete is True
    assert "Client TEST operation command failed" in native_log.error.call_args.args[0]
    assert "Rejected command 17" in native_log.error.call_args.args[0]


@pytest.mark.asyncio
async def test_sync_command_failure_does_not_strand_later_work(native_log) -> None:
    """
    Report a failure before a coroutine is returned and run the next command.
    """
    calls = []

    class FailingClient(Client):
        def command(self, value):
            calls.append(value)
            if value == 17:
                raise ValueError("Rejected before returning a coroutine")
            return asyncio.sleep(0)

    client = FailingClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command", (17,))
    runtime.admit("command", (29,))
    try:
        await command_task()
    finally:
        runtime.dispose()

    assert calls == [17, 29]
    assert runtime.complete is True
    native_log.error.assert_called_once()
    message = native_log.error.call_args.args[0]
    assert message.startswith(
        "Client TEST operation command failed\nTraceback (most recent call last):",
    )
    assert "in command" in message
    assert message.endswith("ValueError: Rejected before returning a coroutine\n")


@pytest.mark.asyncio
async def test_requests_progress_while_command_is_suspended() -> None:
    """
    Requests progress while command is suspended.
    """
    entered = asyncio.Event()
    release = asyncio.Event()
    calls = []

    class RequestClient(Client):
        async def command(self):
            entered.set()
            await release.wait()
            calls.append("command")

        async def request(self):
            calls.append("request")
            return 37

    client = RequestClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command")
    await entered.wait()
    async with asyncio.timeout(1):
        result = await runtime.call_awaited("request")
    assert calls == ["request"]
    release.set()
    await command_task()

    assert result == 37
    assert calls == ["request", "command"]
    assert runtime.complete is True


@pytest.mark.parametrize("abandon", [False, True])
def test_closed_loop_keeps_nonterminal_task_owned_and_reports_incomplete_cleanup(abandon) -> None:
    """
    Keep incomplete work owned even after its loop can no longer finish it.
    """
    # A closed loop cannot finish its tasks; isolate their intentional lifetime in a process.
    code = textwrap.dedent("""
        import asyncio
        import gc
        import sys
        import weakref
        from unittest.mock import Mock, patch
        from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime

        class Client:
            client_id = "TEST"

        logger = Mock()
        loop = asyncio.new_event_loop()
        client = Client()
        retained = weakref.ref(client)
        with patch("nautilus_trader.common.Logger", return_value=logger):
            runtime = ClientRuntime(client)

        async def start():
            runtime.bind(loop)
            return runtime.create_task(asyncio.sleep(60))

        task = loop.run_until_complete(start())
        loop.close()
        del client

        if sys.argv[1] == "True":
            runtime.abandon(task)
        runtime.dispose()
        gc.collect()

        assert task.done() is False
        assert runtime.complete is False
        assert retained() is not None
        assert "Client TEST cleanup is incomplete (1 tasks)" in logger.error.call_args.args[0]
        task._log_destroy_pending = False
    """)
    result = subprocess.run(
        [sys.executable, "-c", code, str(abandon)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=10,
        check=False,
    )

    assert result.returncode == 0, result.stderr


@pytest.mark.asyncio
@pytest.mark.parametrize("disposed", [False, True])
async def test_runtime_cannot_bind_twice_or_after_disposal(disposed) -> None:
    """
    A runtime has exactly one node lifetime.
    """
    client = Client()
    runtime = ClientRuntime(client)
    if disposed:
        runtime.dispose()
    else:
        runtime.bind(asyncio.get_running_loop())

    with pytest.raises(RuntimeError, match="can only run once"):
        runtime.bind(asyncio.get_running_loop())

    assert runtime.complete is True


@pytest.mark.asyncio
async def test_bind_rejects_other_loop_without_consuming_runtime() -> None:
    """
    A failed loop binding leaves the owner able to bind correctly.
    """
    client = Client()
    runtime = ClientRuntime(client)
    other = asyncio.new_event_loop()
    try:
        with pytest.raises(RuntimeError, match="running owner loop"):
            runtime.bind(other)
    finally:
        other.close()
    runtime.bind(asyncio.get_running_loop())
    result = await runtime.create_task(asyncio.sleep(0, result=19))

    assert result == 19
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_non_coroutine_task_is_rejected_without_retention() -> None:
    """
    Admission rejects awaitables which are not coroutine objects.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    future = asyncio.get_running_loop().create_future()

    with pytest.raises(TypeError, match="Expected a coroutine"):
        runtime.create_task(future)

    assert runtime.complete is True
    assert future.cancelled() is False


@pytest.mark.asyncio
async def test_missing_client_closes_new_coroutine() -> None:
    """
    A collected idle client cannot acquire new work.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    del client
    coroutine = asyncio.sleep(0)

    with pytest.raises(RuntimeError, match="no longer exists"):
        runtime.create_task(coroutine)

    assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED
    assert runtime.complete is True


@pytest.mark.asyncio
@pytest.mark.parametrize("queued", [False, True])
async def test_task_factory_failure_rolls_back_admission(queued) -> None:
    """
    Failed task creation closes coroutines and does not strand a command.
    """
    client = Client()
    calls = []

    async def receive(value):
        calls.append(value)

    client.receive = receive
    runtime = ClientRuntime(client)
    loop = asyncio.get_running_loop()
    runtime.bind(loop)
    coroutine = receive(17)
    wrappers = []

    def reject_task(loop, coroutine, **kwargs: object):
        wrappers.append(coroutine)
        raise RuntimeError("Task factory rejected work")

    previous = loop.get_task_factory()
    loop.set_task_factory(reject_task)
    try:
        if queued:
            coroutine.close()
            with pytest.raises(RuntimeError, match="Task factory rejected work"):
                runtime.admit("receive", (17,))
        else:
            with pytest.raises(RuntimeError, match="Task factory rejected work"):
                runtime.create_task(coroutine)
    finally:
        loop.set_task_factory(previous)

    assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED
    assert [inspect.getcoroutinestate(wrapper) for wrapper in wrappers] == [inspect.CORO_CLOSED]
    assert runtime.complete is True
    runtime.admit("receive", (29,))
    await command_task()
    assert calls == [29]


@pytest.mark.asyncio
@pytest.mark.parametrize("operation", ["bind", "call", "admit", "dispose", "abandon"])
async def test_runtime_rejects_wrong_thread(operation) -> None:
    """
    Owner-only operations cannot cross the synchronous core thread boundary.
    """
    client = Client()
    runtime = ClientRuntime(client)
    loop = asyncio.get_running_loop()
    runtime.bind(loop)
    task = asyncio.create_task(asyncio.sleep(0))
    await task
    args = {
        "bind": (loop,),
        "call": ("unused",),
        "admit": ("unused",),
        "dispose": (),
        "abandon": (task,),
    }

    with pytest.raises(RuntimeError, match="owner thread"):
        await asyncio.to_thread(getattr(runtime, operation), *args[operation])

    result = await runtime.create_task(asyncio.sleep(0, result=41))
    assert result == 41
    assert runtime.complete is True


def test_runtime_rejects_wrong_loop_and_closed_bound_loop() -> None:
    """
    A runtime remains attached to its original event loop.
    """
    client = Client()
    runtime = ClientRuntime(client)
    owner = asyncio.new_event_loop()
    other = asyncio.new_event_loop()

    async def bind():
        runtime.bind(asyncio.get_running_loop())

    async def reject():
        coroutine = asyncio.sleep(0)
        with pytest.raises(RuntimeError, match="bound event loop"):
            runtime.create_task(coroutine)
        assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED

    try:
        owner.run_until_complete(bind())
        other.run_until_complete(reject())
        owner.close()
        other.run_until_complete(reject())
    finally:
        owner.close()
        other.close()

    assert runtime.complete is True


@pytest.mark.asyncio
@pytest.mark.parametrize("cancelled", [False, True])
async def test_failed_connect_never_marks_runtime_connected(cancelled) -> None:
    """
    Failed and cancelled connection hooks leave connection state false.
    """
    client = Client()

    async def connect():
        if cancelled:
            raise asyncio.CancelledError
        raise RuntimeError("Connection refused")

    client._connect = connect
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    with pytest.raises(asyncio.CancelledError if cancelled else RuntimeError):
        await runtime.lifecycle("connect")

    assert runtime.connected is False
    assert runtime.complete is True


@pytest.mark.asyncio
@pytest.mark.parametrize("operation", ["abandon", "dispose"])
async def test_terminal_task_before_done_callback_is_retrieved_once(operation, native_log) -> None:
    """
    Teardown retrieves a terminal task even before its queued done callback runs.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    finished = asyncio.Event()

    async def fail():
        finished.set()
        raise RuntimeError("Terminal failure before callback")

    task = runtime.create_task(fail(), "terminal")
    await finished.wait()
    assert task.done() is True
    if operation == "abandon":
        runtime.abandon(task)
    else:
        runtime.dispose()
    await asyncio.sleep(0)

    assert runtime.complete is True
    native_log.error.assert_called_once()
    message = native_log.error.call_args.args[0]
    assert message.startswith(
        "Client TEST operation terminal failed\nTraceback (most recent call last):",
    )
    assert message.endswith("RuntimeError: Terminal failure before callback\n")


@pytest.mark.asyncio
async def test_abandon_unowned_pending_task_does_not_cancel_it() -> None:
    """
    Abandonment only cancels work owned by this runtime.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    gate = asyncio.Event()
    task = asyncio.create_task(gate.wait())
    runtime.abandon(task)
    gate.set()
    result = await task

    assert result is True
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_lifecycle_detects_collected_idle_client() -> None:
    """
    Lifecycle invocation rejects a missing client before invoking a hook.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    del client

    with pytest.raises(RuntimeError, match="no longer exists"):
        await runtime.connect()

    assert runtime.connected is False
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_disposal_cancels_running_dispatch_and_discards_queued_work(native_log) -> None:
    """
    Cancellation interrupts the active command and prevents later commands from running.
    """
    entered = asyncio.Event()
    finished = asyncio.Event()
    calls = []

    class QueuedClient(Client):
        async def command(self, value):
            calls.append(value)
            entered.set()
            try:
                await asyncio.Future()
            finally:
                finished.set()

    client = QueuedClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command", (17,))
    runtime.admit("command", (29,))
    await entered.wait()
    worker = command_task()
    runtime.dispose()
    with pytest.raises(asyncio.CancelledError):
        await worker

    assert calls == [17]
    assert finished.is_set() is True
    assert runtime.complete is True
    assert [call.args[0] for call in native_log.error.call_args_list] == [
        "Client TEST abandoned queued operation command",
        "Client TEST cleanup is incomplete (1 tasks)",
    ]


def test_adapter_diagnostics_reach_native_logger_with_traceback() -> None:
    """
    Verify component names, errors, and provider warnings in the native log output.
    """
    import subprocess
    import sys
    import textwrap

    code = textwrap.dedent(
        """
        import asyncio
        from nautilus_trader.common import Logger, LogLevel, init_logging, logger_flush
        from nautilus_trader.core import UUID4
        from nautilus_trader.model import TraderId
        from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime
        from nautilus_trader.live.providers import InstrumentProvider

        guard = init_logging(
            trader_id=TraderId("TESTER-001"), instance_id=UUID4(),
            level_stdout=LogLevel.INFO, is_colored=False, print_config=False,
        )

        class Client:
            client_id = "ADAPTER"

            async def command(self):
                try:
                    raise KeyError("upstream cause")
                except KeyError as e:
                    raise ValueError("adapter failure") from e

        async def run():
            client = Client()
            runtime = ClientRuntime(client)
            provider = InstrumentProvider()
            assert isinstance(provider._log, Logger)
            assert provider._log.name == "InstrumentProvider"
            runtime.bind(asyncio.get_running_loop())
            runtime.admit("command")
            await next(task for task in asyncio.all_tasks() if task.get_name() == "ADAPTER:commands")
            await provider.initialize()
            runtime.dispose()
            assert runtime.complete

        asyncio.run(run())
        logger_flush()
        """,
    )
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=10,
        check=False,
    )
    output = result.stdout + result.stderr

    assert result.returncode == 0, output
    assert output.count("[ERROR] TESTER-001.ADAPTER: Client ADAPTER operation command failed") == 1
    assert "Traceback (most recent call last):" in output
    assert "KeyError: 'upstream cause'" in output
    assert "ValueError: adapter failure" in output
    assert (
        output.count("[WARN] TESTER-001.InstrumentProvider: No instrument loading configured") == 1
    )


@pytest.mark.asyncio
async def test_idle_runtime_is_collectible_after_task_completion() -> None:
    """
    Release the runtime and client after the last result is retrieved.
    """
    client = Client()
    runtime = ClientRuntime(client)
    retained_client = weakref.ref(client)
    retained_runtime = weakref.ref(runtime)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.create_task(asyncio.sleep(0, result=71))
    result = await task
    del task, runtime, client
    gc.collect()

    assert result == 71
    assert retained_client() is None
    assert retained_runtime() is None


@pytest.mark.asyncio
async def test_pending_operation_survives_collection_without_external_owners() -> None:
    """
    Keep orphaned work alive until its result releases the runtime and client.
    """
    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    retained_client = weakref.ref(client)
    retained_runtime = weakref.ref(runtime)
    entered = asyncio.Event()
    pending = []

    async def operation():
        future = asyncio.get_running_loop().create_future()
        pending.append(weakref.ref(future))
        entered.set()
        return await future

    task = runtime.create_task(operation())
    retained_task = weakref.ref(task)
    await entered.wait()
    del task, runtime, client
    gc.collect()

    assert retained_task() is not None
    assert retained_runtime() is not None
    assert retained_client() is not None
    pending[0]().set_result(83)
    result = await retained_task()
    await asyncio.sleep(0)
    gc.collect()

    assert result == 83
    assert retained_task() is None
    assert retained_runtime() is None
    assert retained_client() is None


def command_task() -> asyncio.Task:
    """
    Find the runtime command task through the asyncio task interface.
    """
    return next(task for task in asyncio.all_tasks() if task.get_name() == "TEST:commands")


@pytest.mark.asyncio
@pytest.mark.parametrize("uncancel", [False, True])
async def test_supervisor_cancels_cleanup_only_once(uncancel, native_log) -> None:
    """
    Disconnect, abandonment, and disposal preserve asynchronous command cleanup.
    """
    entered = asyncio.Event()
    cleaning = asyncio.Event()
    release = asyncio.Event()
    finished = asyncio.Event()

    class CleanupClient(Client):
        async def command(self):
            entered.set()
            try:
                await asyncio.Future()
            finally:
                if uncancel:
                    asyncio.current_task().uncancel()
                cleaning.set()
                await release.wait()
                finished.set()

        async def _disconnect(self):
            await cleaning.wait()

    client = CleanupClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command")
    await entered.wait()
    worker = command_task()
    disconnect = runtime.lifecycle("disconnect")
    await cleaning.wait()
    await asyncio.sleep(0)
    runtime.abandon(worker)
    runtime.dispose()
    runtime.dispose()
    await asyncio.sleep(0)
    observed = worker.done(), finished.is_set(), runtime.complete
    release.set()
    await asyncio.gather(worker, disconnect, return_exceptions=True)
    await asyncio.sleep(0)

    assert observed == (False, False, False)
    assert finished.is_set() is True
    assert runtime.complete is True


@pytest.mark.asyncio
@pytest.mark.parametrize("source", ["sync", "async", "child", "self"])
async def test_independent_command_cancellation_continues_queue(source, native_log) -> None:
    """
    Cancellation inside a command does not strand later admitted operations.
    """
    calls = []

    class CancelledClient(Client):
        def command(self, value):
            calls.append(value)
            if value == 17 and source == "sync":
                raise asyncio.CancelledError("independent")
            return self.run(value)

        async def run(self, value):
            if value == 17:
                if source == "self":
                    asyncio.current_task().cancel()
                    await asyncio.sleep(0)
                if source == "child":
                    child = asyncio.create_task(asyncio.sleep(0))
                    child.cancel()
                    await child
                raise asyncio.CancelledError("independent")

    client = CancelledClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command", (17,))
    runtime.admit("command", (29,))
    await command_task()

    assert calls == [17, 29]
    assert runtime.complete is True
    native_log.error.assert_called_once()
    assert "operation command failed" in native_log.error.call_args.args[0]
    assert "CancelledError" in native_log.error.call_args.args[0]


@pytest.mark.asyncio
async def test_cancelled_failed_disconnect_releases_traceback_cycle(native_log) -> None:
    """
    A failed hook's traceback cannot retain a terminal drain and its owner.
    """
    entered = asyncio.Event()
    cleaning = asyncio.Event()
    release = asyncio.Event()

    class FailedClient(Client):
        async def background(self):
            entered.set()
            try:
                await asyncio.Future()
            finally:
                cleaning.set()
                await release.wait()

        async def _disconnect(self):
            raise ValueError("disconnect failed")

    client = FailedClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.create_task(client.background())
    await entered.wait()
    disconnect = runtime.lifecycle("disconnect")
    await cleaning.wait()
    disconnect.cancel()
    with pytest.raises(asyncio.CancelledError):
        await disconnect
    runtime.dispose()
    observed = task.done(), runtime.complete
    release.set()
    await asyncio.gather(task, return_exceptions=True)
    await asyncio.sleep(0)
    refs = weakref.ref(client), weakref.ref(runtime)
    del client, runtime, task, disconnect
    await asyncio.sleep(0)
    gc.collect()

    assert observed == (False, False)
    assert [ref() for ref in refs] == [None, None]


@pytest.mark.asyncio
@pytest.mark.parametrize("uncancel", [False, True])
@pytest.mark.parametrize("join_cleanup", [False, True])
async def test_propagated_cancellation_preserves_owned_child_cleanup(
    uncancel,
    join_cleanup,
    native_log,
) -> None:
    """
    A worker's cancellation counts as the owned child's cancellation request.
    """
    entered = asyncio.Event()
    cleaning = asyncio.Event()
    release = asyncio.Event()
    finished = asyncio.Event()
    children = []

    class NestedClient(Client):
        async def child(self):
            entered.set()
            try:
                await asyncio.Future()
            finally:
                if uncancel:
                    asyncio.current_task().uncancel()
                cleaning.set()
                await release.wait()
                finished.set()

        async def command(self):
            child = runtime.create_task(self.child())
            children.append(child)
            await child

        async def _disconnect(self):
            await cleaning.wait()

            if join_cleanup:
                await children[0]

    client = NestedClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command")
    await entered.wait()
    disconnect = runtime.lifecycle("disconnect")
    await cleaning.wait()
    await asyncio.sleep(0)
    await asyncio.sleep(0)

    if join_cleanup:
        runtime.abandon(disconnect)
        await asyncio.sleep(0)
        await asyncio.sleep(0)
    observed = children[0].done(), disconnect.done(), finished.is_set()
    release.set()
    await asyncio.gather(disconnect, return_exceptions=True)

    assert observed == (False, False, False)
    assert finished.is_set() is True
    assert runtime.complete is True


@pytest.mark.asyncio
@pytest.mark.parametrize("swallow", [False, True])
async def test_consumed_command_cancellation_does_not_block_shutdown(swallow, native_log) -> None:
    """
    Shutdown cancels the next command after an earlier command consumes cancellation.
    """
    entered = asyncio.Event()
    release = asyncio.Event()
    finished = asyncio.Event()

    class SelfCancellingClient(Client):
        async def command(self, value):
            if value == 17:
                asyncio.current_task().cancel()
                try:
                    await asyncio.sleep(0)
                except asyncio.CancelledError:
                    if not swallow:
                        raise
                return
            entered.set()
            try:
                await release.wait()
            finally:
                finished.set()

        async def _disconnect(self):
            pass

    client = SelfCancellingClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    runtime.admit("command", (17,))
    runtime.admit("command", (29,))
    await entered.wait()
    disconnect = runtime.lifecycle("disconnect")
    done, _ = await asyncio.wait([disconnect], timeout=1)
    observed = disconnect in done, finished.is_set()
    release.set()
    await disconnect

    assert observed == (True, True)
    assert runtime.complete is True


@pytest.mark.asyncio
async def test_explicit_cancellation_remains_available_after_supervisor_request(native_log) -> None:
    """
    Supervisor suppression does not change subsequent explicit Task.cancel calls.
    """
    entered = asyncio.Event()
    cleaning = asyncio.Event()

    async def operation():
        entered.set()
        try:
            await asyncio.Future()
        finally:
            cleaning.set()
            await asyncio.Future()

    client = Client()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.create_task(operation())
    await entered.wait()
    runtime.abandon(task)
    await cleaning.wait()
    requested = task.cancel(msg="explicit caller")
    with pytest.raises(asyncio.CancelledError) as exc_info:
        await task

    assert requested is True
    assert exc_info.value.args == ("explicit caller",)
    assert runtime.complete is True
