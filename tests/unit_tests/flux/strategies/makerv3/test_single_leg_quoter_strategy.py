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

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import MakerV3SingleLegQuoter
from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import MakerV3SingleLegQuoterConfig


def _make_strategy() -> MakerV3SingleLegQuoter:
    config = MakerV3SingleLegQuoterConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal("1"),
        bot_on=True,
        max_age_ms=100,
        quote_fail_critical_after_count=2,
        quote_fail_critical_after_s=10.0,
    )
    strategy = MakerV3SingleLegQuoter(config=config)
    strategy._maker_instrument = object()
    strategy._order_qty = object()
    strategy._runtime_params = {
        "bot_on": True,
        "max_age_ms": 100,
        "quote_fail_critical_after_count": 2,
        "quote_fail_critical_after_s": Decimal("10"),
    }
    strategy._last_bbo_ts_ns = {
        config.maker_instrument_id: 0,
        config.reference_instrument_id: 0,
    }
    strategy._instruments = {}
    strategy._managed_client_order_ids = set()
    strategy._quote_failures_ns = []
    strategy._quote_failure_circuit_open = False
    strategy._params_timer_name = "params-refresh"
    strategy.unsubscribe_order_book_deltas = lambda *args, **kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._cache = SimpleNamespace(order=lambda _client_order_id: None)
    return strategy


def test_refresh_quotes_blocks_when_maker_market_data_is_stale() -> None:
    strategy = _make_strategy()

    cancels: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False: cancels.append(f"{reason}:{force}")
    strategy._publish_state = lambda state: states.append(state)
    strategy._best_bid_ask = lambda _instrument_id: (Decimal("100"), Decimal("101"))

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 200_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    strategy._refresh_quotes(now_ns=now_ns)

    assert "maker_md_stale:False" in cancels
    assert states == ["blocked_maker_md"]


def test_refresh_quotes_blocks_when_reference_market_data_is_stale() -> None:
    strategy = _make_strategy()

    cancels: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False: cancels.append(f"{reason}:{force}")
    strategy._publish_state = lambda state: states.append(state)
    strategy._best_bid_ask = lambda _instrument_id: (Decimal("100"), Decimal("101"))

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 200_000_000

    strategy._refresh_quotes(now_ns=now_ns)

    assert "reference_md_stale:False" in cancels
    assert states == ["blocked_reference_md"]


def test_lifecycle_handlers_reconcile_local_managed_order_state() -> None:
    strategy = _make_strategy()
    strategy._managed_client_order_ids = {"A", "B", "C"}

    strategy.on_order_rejected(SimpleNamespace(client_order_id="A"))
    strategy.on_order_canceled(SimpleNamespace(client_order_id="B"))
    strategy.on_order_expired(SimpleNamespace(client_order_id="C"))

    assert strategy._managed_client_order_ids == set()


def test_quote_failure_circuit_breaker_triggers_stop() -> None:
    strategy = _make_strategy()

    canceled: list[tuple[str, bool]] = []
    states: list[str] = []
    stopped: list[bool] = []
    strategy._cancel_managed_quotes = lambda reason, force=False: canceled.append((reason, force))
    strategy._publish_state = lambda state: states.append(state)
    strategy.stop = lambda: stopped.append(True)

    strategy._handle_quote_failure(now_ns=1_000_000_000, exc=RuntimeError("boom-1"), context="test")
    strategy._handle_quote_failure(now_ns=2_000_000_000, exc=RuntimeError("boom-2"), context="test")

    assert stopped == [True]
    assert canceled[-1] == ("quote_fail_circuit_breaker", True)
    assert states[-1] == "blocked_quote_failures"


def test_on_stop_cancels_even_when_cache_managed_orders_are_empty() -> None:
    strategy = _make_strategy()

    strategy._managed_client_order_ids = {"RESTING-1"}
    strategy._managed_orders = lambda: []

    canceled_all: list[str] = []
    states: list[str] = []
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_state = lambda state: states.append(state)

    strategy.on_stop()

    assert canceled_all == [str(strategy.config.maker_instrument_id)]
    assert states == ["on_stop"]
