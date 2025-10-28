# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import inspect
import os
import sys
from pathlib import Path

import pytest

from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


@pytest.fixture(scope="session")
def event_loop_policy():
    """
    Provide uvloop event loop policy for pytest-asyncio.

    This ensures all tests run with uvloop when available (non-Windows platforms). The
    session scope ensures the policy is set once for the entire test session.

    """
    if sys.platform == "win32":
        # uvloop not supported on Windows, use default policy
        return asyncio.DefaultEventLoopPolicy()

    try:
        import uvloop

        return uvloop.EventLoopPolicy()
    except ImportError:
        # Fallback to default if uvloop not available
        return asyncio.DefaultEventLoopPolicy()


@pytest.fixture(scope="session")
def session_event_loop(event_loop_policy):
    """
    Provide a session-scoped event loop for session-scoped fixtures.

    This is used by fixtures that need an event loop at session scope (e.g., HTTP
    clients). The loop is created once per test session and cleaned up at the end.

    """
    policy = event_loop_policy
    loop = policy.new_event_loop()
    asyncio.set_event_loop(loop)

    yield loop

    # Cleanup
    try:
        # Cancel all pending tasks
        pending = [task for task in asyncio.all_tasks(loop) if not task.done()]
        for task in pending:
            task.cancel()
        if pending:
            loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))

        # Close the loop only if it's not running
        if not loop.is_running():
            loop.close()
    except RuntimeError:
        # Loop may already be closed or in an invalid state
        pass
    finally:
        asyncio.set_event_loop(None)


def pytest_pycollect_makeitem(collector, name, obj):
    """
    Prevent pytest from collecting Rust/PyO3 utility classes as test classes.

    Classes like TestClock, TestTimer, etc. start with "Test" but are utility classes
    from PyO3/Rust, not test classes. We skip collection for these to prevent pytest-
    asyncio from trying to set attributes on immutable types.

    """
    if inspect.isclass(obj) and name.startswith("Test"):
        module = getattr(obj, "__module__", "")
        # Skip if from nautilus_trader non-test modules
        if (
            module.startswith("nautilus_trader.")
            and ".tests" not in module
            and not module.startswith("tests.")
        ):
            return []


def _env_flag(name: str, *, default: bool = False) -> bool:
    """
    Return *name* environment variable interpreted as a boolean.

    Truthy values (case-insensitive):
    - "1", "true", "yes", "y", "on"

    Falsy values (case-insensitive):
    - "0", "false", "no", "n", "off", "" (empty string)

    Any other value raises :class:`ValueError` so misspelled variables
    fail fast instead of being silently treated as *default*.

    """
    value = os.getenv(name)
    if value is None:
        return default

    value_normalized = value.strip().lower()

    if value_normalized in {"1", "true", "yes", "y", "on"}:
        return True
    if value_normalized in {"0", "false", "no", "n", "off", ""}:
        return False

    raise ValueError(
        f"Unsupported boolean environment value for {name}: {value!r}. "
        "Expected one of '1', '0', 'true', 'false', etc.",
    )


@pytest.fixture(scope="session", autouse=True)
def verify_test_parquet_integrity() -> None:
    # Only enforce in CI by default; allow opt-in locally via VERIFY_TEST_PARQUET=true
    if os.getenv("CI") != "true" and not _env_flag("VERIFY_TEST_PARQUET", default=False):
        return
    """
    Fail fast if any parquet under tests/test_data is invalid or an LFS pointer.
    """
    base = Path(__file__).parent / "test_data"
    if not base.exists():
        return

    bad: list[tuple[Path, str]] = []

    for path in base.rglob("*.parquet"):
        try:
            with path.open("rb") as fh:
                head = fh.read(256)
                if head.startswith(b"version https://git-lfs.github.com/spec/"):
                    bad.append((path, "git-lfs-pointer"))
                    continue
                fh.seek(0, os.SEEK_END)
                size = fh.tell()
                if size < 4:
                    bad.append((path, "too-small"))
                    continue
                fh.seek(-4, os.SEEK_END)
                tail = fh.read(4)
                if tail != b"PAR1":
                    bad.append((path, "bad-magic"))
        except Exception as e:
            bad.append((path, f"error: {e}"))

    if bad:
        details = "\n".join(f"- {p} -> {reason}" for p, reason in bad[:10])
        raise AssertionError(
            "Invalid parquet files under tests/test_data."
            ' Ensure Git LFS files are fetched in CI (checkout lfs: true and git lfs pull --include="tests/test_data/**").\n'
            + details,
        )


@pytest.fixture(scope="session", autouse=True)
def bypass_logging():
    """
    Fixture to bypass logging for all tests.

    `autouse=True` will mean this function is run prior to every test. To disable this
    to debug specific tests, simply comment this out.

    """
    # Uncomment below for tracing logs from Rust
    # from nautilus_trader.core import nautilus_pyo3
    # nautilus_pyo3.init_tracing()
    guard = init_logging(
        level_stdout=LogLevel.DEBUG,
        bypass=True,  # Set this to False to see logging in tests
        # print_config=True,
    )
    # Yield guard to keep it alive for the session lifetime, avoiding garbage collection
    yield guard


@pytest.fixture
def event_loop_for_setup(event_loop):
    """
    Provide an event loop for non-async setup methods.

    This fixture delegates to pytest-asyncio's managed event_loop fixture,
    ensuring proper lifecycle management across uvloop 0.22+ and pytest-asyncio 0.23+.

    Note: This fixture is deprecated and will be removed. Use `event_loop` directly instead.

    """
    return event_loop


@pytest.fixture(name="audusd_instrument")
def fixture_audusd_instrument() -> CurrencyPair:
    return TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("SIM"))


@pytest.fixture(name="data_provider", scope="session")
def fixture_data_provider() -> TestDataProvider:
    return TestDataProvider()


@pytest.fixture(name="audusd_quote_ticks", scope="session")
def fixture_audusd_quote_ticks(
    data_provider: TestDataProvider,
    audusd_instrument: CurrencyPair,
) -> list[QuoteTick]:
    wrangler = QuoteTickDataWrangler(instrument=audusd_instrument)
    return wrangler.process(data_provider.read_csv_ticks("truefx/audusd-ticks.csv"))


@pytest.fixture(autouse=True)
def cleanup_event_loop_tasks(event_loop):
    """
    Cleanup asyncio tasks after each test to prevent task leaks.

    This fixture ensures all pending tasks are cancelled and waits for them to complete
    their cancellation (executing finally blocks), preventing file descriptor leaks and
    cross-test contamination.

    """
    yield
    # After the test completes, cancel any remaining tasks and wait for cleanup
    # Use event_loop fixture directly - it's managed by pytest-asyncio
    if event_loop and not event_loop.is_closed():
        pending = [task for task in asyncio.all_tasks(event_loop) if not task.done()]
        if pending:
            # Cancel all pending tasks
            for task in pending:
                if not task.done():
                    task.cancel()

            # Wait for tasks to finish cancelling and run their finally blocks
            # Critical for resource cleanup (sockets, queues, file descriptors)
            try:
                event_loop.run_until_complete(
                    asyncio.wait_for(
                        asyncio.gather(*pending, return_exceptions=True),
                        timeout=2.0,
                    ),
                )
            except (TimeoutError, asyncio.CancelledError):
                # Some tasks didn't complete cancellation within timeout
                pass
            except RuntimeError:
                # Loop may be running or in unexpected state
                pass
            except Exception:  # noqa: S110
                # Catch any other exceptions during cleanup to prevent test failures
                pass
