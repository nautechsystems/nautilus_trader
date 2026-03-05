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

from collections.abc import Callable
from decimal import Decimal
from types import SimpleNamespace
from typing import Any

import pytest

from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.flux.strategies.makerv3 import MakerV3StrategyConfig
from nautilus_trader.flux.strategies.makerv3 import runtime_params as runtime_params_mod
from nautilus_trader.model.identifiers import InstrumentId


def _bootstrap_strategy(strategy: MakerV3Strategy) -> MakerV3Strategy:
    strategy._maker_instrument = object()
    strategy._order_qty = object()
    strategy._runtime_params = runtime_params_mod.initial_runtime_params(strategy.config)
    strategy._last_bbo_ts_ns = {
        strategy.config.maker_instrument_id: 0,
        strategy.config.reference_instrument_id: 0,
    }
    strategy._instruments = {}
    strategy._managed_client_order_ids = set()
    strategy._quote_failures_ns = []
    strategy._quote_failure_circuit_open = False
    strategy._params_timer_name = "params-refresh"
    strategy.unsubscribe_order_book_deltas = lambda *args, **kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None  # type: ignore[method-assign]
    strategy._publish_alert = lambda *_args, **_kwargs: None  # type: ignore[method-assign]
    strategy._cache = SimpleNamespace(order=lambda _client_order_id: None)
    return strategy


@pytest.fixture(name="strategy_factory")
def fixture_strategy_factory() -> Callable[..., MakerV3Strategy]:
    def _make(**config_overrides: Any) -> MakerV3Strategy:
        config_kwargs: dict[str, Any] = {
            "maker_instrument_id": InstrumentId.from_str("MAKER.SIM"),
            "reference_instrument_id": InstrumentId.from_str("REF.SIM"),
            "order_qty": Decimal(1),
            "bot_on": True,
            "max_age_ms": 100,
            "quote_fail_critical_after_count": 2,
            "quote_fail_critical_after_s": 10.0,
            "cancel_all_instrument_orders": False,
        }
        config_kwargs.update(config_overrides)
        config = MakerV3StrategyConfig(**config_kwargs)
        return _bootstrap_strategy(MakerV3Strategy(config=config))

    return _make


class _FakeClock:
    def __init__(self, timestamps_ns: list[int]) -> None:
        self._timestamps_ns = timestamps_ns
        self._index = 0

    def timestamp_ns(self) -> int:
        if self._index >= len(self._timestamps_ns):
            return self._timestamps_ns[-1]
        timestamp_ns = self._timestamps_ns[self._index]
        self._index += 1
        return timestamp_ns


class _ClockedStrategy(MakerV3Strategy):
    def __init__(self, config: MakerV3StrategyConfig, timestamps_ns: list[int]) -> None:
        self._test_clock = _FakeClock(timestamps_ns)
        super().__init__(config=config)

    @property
    def clock(self) -> _FakeClock:
        return self._test_clock


@pytest.fixture(name="clocked_strategy_factory")
def fixture_clocked_strategy_factory() -> Callable[..., MakerV3Strategy]:
    def _make(timestamps_ns: list[int], **config_overrides: Any) -> MakerV3Strategy:
        config_kwargs: dict[str, Any] = {
            "maker_instrument_id": InstrumentId.from_str("MAKER.SIM"),
            "reference_instrument_id": InstrumentId.from_str("REF.SIM"),
            "order_qty": Decimal(1),
            "bot_on": True,
            "max_age_ms": 100,
            "quote_fail_critical_after_count": 2,
            "quote_fail_critical_after_s": 10.0,
        }
        config_kwargs.update(config_overrides)
        config = MakerV3StrategyConfig(**config_kwargs)
        return _bootstrap_strategy(_ClockedStrategy(config=config, timestamps_ns=timestamps_ns))

    return _make


@pytest.fixture(name="raise_runtime_error")
def fixture_raise_runtime_error() -> Callable[..., None]:
    def _raise(*_args: Any, **_kwargs: Any) -> None:
        raise RuntimeError("side-effect boom")

    return _raise
