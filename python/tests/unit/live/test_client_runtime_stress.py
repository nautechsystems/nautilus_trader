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
Stress task ownership across seeded cancellation and repeated lifetimes.
"""

import asyncio
import gc
import random
import weakref

import pytest

from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime


@pytest.mark.parametrize("seed", range(4))
@pytest.mark.parametrize("loop_kind", ["asyncio", "uvloop"])
@pytest.mark.parametrize("eager", [False, True])
def test_seeded_runtime_lifetimes(seed, loop_kind, eager, native_log) -> None:
    """
    Release every owner after varied cancellation schedules on the supported loops.
    """
    runner = asyncio.run if loop_kind == "asyncio" else pytest.importorskip("uvloop").run
    runner(exercise_lifetimes(seed, eager, 8))


async def exercise_lifetimes(seed: int, eager: bool, repetitions: int) -> None:
    """
    Check repeated lifetimes and loop diagnostics under aggressive collection.
    """
    loop = asyncio.get_running_loop()
    previous_factory = loop.get_task_factory()
    previous_handler = loop.get_exception_handler()
    previous_debug = loop.get_debug()
    previous_threshold = gc.get_threshold()
    errors = []
    loop.set_exception_handler(lambda _loop, context: errors.append(context))
    loop.set_debug(True)
    gc.set_threshold(5, 1, 1)

    if eager:
        loop.set_task_factory(asyncio.eager_task_factory)
    try:
        for iteration in range(repetitions):
            async with asyncio.timeout(5):
                refs = await exercise_lifetime(seed * 100_003 + iteration)
            await asyncio.sleep(0)
            gc.collect()
            assert [ref() for ref in refs] == [None] * len(refs), (seed, iteration)
        assert errors == []
    finally:
        loop.set_task_factory(previous_factory)
        loop.set_exception_handler(previous_handler)
        loop.set_debug(previous_debug)
        gc.set_threshold(*previous_threshold)


async def exercise_lifetime(seed: int) -> list[weakref.ReferenceType]:  # noqa: C901 - Keep the seeded lifecycle and its assertions in one scenario.
    """
    Mix command failures, nested cancellation, and repeated terminal checks.
    """
    rng = random.Random(seed)  # noqa: S311 - Reproduce scheduling choices from the seed.
    modes = [rng.choice(["success", "failure", "cancel", "self_cancel"]) for _ in range(8)]
    depth = rng.randrange(1, 6)
    join_cleanup = rng.choice([False, True])
    uncancel = rng.choice([False, True])
    entered = asyncio.Event()
    cleaning = asyncio.Event()
    release = asyncio.Event()
    finished = []
    received = []
    completed = []
    children = []

    class StressClient:
        client_id = f"STRESS-{seed}"

        async def _connect(self):
            await asyncio.sleep(0)

        async def command(self, value):
            received.append(value)

            for _ in range(rng.randrange(3)):
                await asyncio.sleep(0)
            mode = modes[value]
            if mode == "failure":
                raise ValueError(f"command {value}")
            if mode == "cancel":
                raise asyncio.CancelledError(f"command {value}")
            if mode == "self_cancel":
                asyncio.current_task().cancel()
                await asyncio.sleep(0)
            completed.append(value)

        async def blocking(self):
            task = runtime.create_task(self.nested(depth), "nested")
            children.append(task)
            await task

        async def nested(self, remaining):
            if remaining:
                task = runtime.create_task(self.nested(remaining - 1), "nested")
                children.append(task)
                await task
                return
            entered.set()
            try:
                await asyncio.Future()
            finally:
                if uncancel:
                    asyncio.current_task().uncancel()
                cleaning.set()
                await release.wait()
                finished.append(seed)

        async def _disconnect(self):
            await cleaning.wait()

            if join_cleanup:
                await children[0]
            else:
                for _ in range(rng.randrange(3)):
                    await asyncio.sleep(0)

    client = StressClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    tasks = []
    try:
        connect = runtime.lifecycle("connect")
        tasks.append(connect)
        await connect

        for value in range(len(modes)):
            runtime.admit("command", (value,))
        runtime.admit("blocking")
        await entered.wait()
        worker = next(
            task
            for task in asyncio.all_tasks()
            if task.get_name() == f"{client.client_id}:commands"
        )
        tasks.append(worker)
        disconnect = runtime.lifecycle("disconnect")
        tasks.append(disconnect)
        await cleaning.wait()

        for _ in range(12):
            action = rng.randrange(4)
            if action == 0:
                runtime.abandon(disconnect)
            elif action == 1:
                runtime.abandon(worker)
            elif action == 2:
                runtime.dispose()
            else:
                gc.collect()
            for _ in range(rng.randrange(1, 4)):
                await asyncio.sleep(0)
            assert finished == [], seed
            assert [task.done() for task in children] == [False] * len(children), seed
            assert runtime.complete is False, seed
    finally:
        release.set()
        runtime.dispose()
        tasks = list(
            set(tasks)
            | set(children)
            | {
                task
                for task in asyncio.all_tasks()
                if task.get_name().startswith(f"{client.client_id}:")
            },
        )
        _, pending = await asyncio.wait(tasks, timeout=2)
        assert pending == set(), seed
        await asyncio.gather(*tasks, return_exceptions=True)
        await asyncio.sleep(0)
        runtime.dispose()

    assert received == list(range(len(modes))), seed
    assert completed == [i for i, mode in enumerate(modes) if mode == "success"], seed
    assert finished == [seed], seed
    assert runtime.complete is True, seed
    return [weakref.ref(client), weakref.ref(runtime), *[weakref.ref(task) for task in tasks]]


@pytest.mark.asyncio
async def test_completed_task_remains_usable_after_runtime_collection(native_log) -> None:
    """
    A retained task's cancellation wrapper does not keep its former runtime alive.
    """

    class FinishedClient:
        client_id = "FINISHED"

    client = FinishedClient()
    runtime = ClientRuntime(client)
    runtime.bind(asyncio.get_running_loop())
    task = runtime.create_task(asyncio.sleep(0, result=137))
    await task
    refs = weakref.ref(client), weakref.ref(runtime)
    del client, runtime
    gc.collect()

    assert [ref() for ref in refs] == [None, None]
    assert task.cancel(msg="after completion") is False
    assert task.result() == 137
