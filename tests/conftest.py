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

import os
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


def _env_flag(name: str, *, default: bool = False) -> bool:
    """
    Return *name* environment variable interpreted as a boolean.

    Truthy values (case-insensitive):
    - "1", "true", "yes", "y", "on"

    Falsy values (case-insensitive):
    - "0", "false", "no", "n", "off", "" (empty string)

    Any other value raises :class:`ValueError` so mis-spelled variables
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
