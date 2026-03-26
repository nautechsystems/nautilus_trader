from __future__ import annotations

import pytest

from nautilus_trader.system.kernel import NautilusKernel


class _FakeLoop:
    def __init__(self) -> None:
        self.created = []

    def create_task(self, coro):
        self.created.append(coro)
        return coro

    def is_closed(self) -> bool:
        return False


class _FakeLog:
    def __init__(self) -> None:
        self.errors: list[str] = []

    def error(self, message: str, *_args, **_kwargs) -> None:
        self.errors.append(message)


class _FakeMsgBusDatabase:
    def __init__(self, *, closed: bool) -> None:
        self._closed = closed

    def is_closed(self) -> bool:
        return self._closed


def test_shutdown_if_msgbus_database_closed_schedules_async_stop() -> None:
    kernel = object.__new__(NautilusKernel)
    kernel._msgbus_db = _FakeMsgBusDatabase(closed=True)
    kernel._is_running = True
    kernel._is_stopping = False
    kernel._fatal_shutdown_reason = None
    kernel._loop = _FakeLoop()
    kernel._log = _FakeLog()

    async def _fake_stop_async() -> None:
        return None

    kernel.stop_async = _fake_stop_async

    assert kernel._shutdown_if_msgbus_database_closed() is True
    assert kernel._fatal_shutdown_reason == "Message bus backing database closed"
    assert kernel._log.errors == ["Message bus backing database closed"]
    assert len(kernel._loop.created) == 1
    kernel._loop.created[0].close()


def test_shutdown_if_msgbus_database_closed_noops_when_database_is_healthy() -> None:
    kernel = object.__new__(NautilusKernel)
    kernel._msgbus_db = _FakeMsgBusDatabase(closed=False)
    kernel._is_running = True
    kernel._is_stopping = False
    kernel._fatal_shutdown_reason = None
    kernel._loop = _FakeLoop()
    kernel._log = _FakeLog()

    assert kernel._shutdown_if_msgbus_database_closed() is False
    assert kernel._fatal_shutdown_reason is None
    assert kernel._log.errors == []
    assert kernel._loop.created == []


@pytest.mark.asyncio
async def test_watch_msgbus_database_stops_kernel_after_database_closes(monkeypatch) -> None:
    kernel = object.__new__(NautilusKernel)
    database = _FakeMsgBusDatabase(closed=False)
    kernel._msgbus_db = database
    kernel._is_running = True
    kernel._is_stopping = False
    kernel._fatal_shutdown_reason = None
    kernel._loop = _FakeLoop()
    kernel._log = _FakeLog()

    async def _fake_stop_async() -> None:
        return None

    async def _fake_sleep(_seconds: float) -> None:
        database._closed = True

    kernel.stop_async = _fake_stop_async
    monkeypatch.setattr("nautilus_trader.system.kernel.asyncio.sleep", _fake_sleep)

    await kernel._watch_msgbus_database()

    assert kernel._fatal_shutdown_reason == "Message bus backing database closed"
    assert kernel._log.errors == ["Message bus backing database closed"]
    assert len(kernel._loop.created) == 1
    kernel._loop.created[0].close()
