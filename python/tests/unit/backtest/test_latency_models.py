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
Test Python latency models through configuration and backtest execution.
"""

from __future__ import annotations

from collections.abc import Iterator
from decimal import Decimal

import pytest

from nautilus_trader.adapters.sandbox import SandboxExecutionClientConfig
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.execution import LatencyModel
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy
from tests.providers import TestInstrumentProvider


class ReplayLatency(LatencyModel):
    """
    Return recorded delays and retain each callback's command and simulation time.
    """

    def __init__(self, insert: object = 111) -> None:
        """
        Retain the configured insertion result and callback history.
        """
        self.insert = insert
        self.calls: list[tuple[object, int]] = []

    def get_insert_latency(self, command: object, ts_now: int) -> int:
        """
        Record the insertion context and return or raise the configured result.
        """
        self.calls.append((command, ts_now))
        if isinstance(self.insert, Exception):
            raise self.insert
        return self.insert

    def get_update_latency(self, command: object, ts_now: int) -> int:
        """
        Record modification context and return its measured delay.
        """
        self.calls.append((command, ts_now))
        return 222

    def get_delete_latency(self, command: object, ts_now: int) -> int:
        """
        Record cancellation context and return its measured delay.
        """
        self.calls.append((command, ts_now))
        return 333


class LatencyProbe(Strategy):
    """
    Submit, modify, and cancel a resting limit order as venue events arrive.
    """

    def __init__(self, instrument_id: object) -> None:
        """
        Set the instrument for the lifecycle probe.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.order_id = None
        self.events: list[tuple[str, int]] = []

    def on_start(self) -> None:
        """
        Subscribe to quotes that advance the simulated lifecycle.
        """
        self.subscribe_quotes(self.instrument_id)

    def on_quote(self, _quote: QuoteTick) -> None:
        """
        Submit one resting limit order on the first quote.
        """
        if self.order_id is not None:
            return
        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10000"),
            price=Price.from_str("0.50000"),
            client_order_id=ClientOrderId("PY-LATENCY-001"),
        )
        self.order_id = order.client_order_id
        self.submit_order(order)

    def on_order_accepted(self, event: object) -> None:
        """
        Record acceptance and request a price change.
        """
        self.events.append(("accepted", event.ts_event))
        self.modify_order(event.client_order_id, price=Price.from_str("0.40000"))

    def on_order_updated(self, event: object) -> None:
        """
        Record the price change and request cancellation.
        """
        self.events.append(("updated", event.ts_event))
        self.cancel_order(event.client_order_id)

    def on_order_canceled(self, event: object) -> None:
        """
        Record the final cancellation time.
        """
        self.events.append(("canceled", event.ts_event))

    def on_reset(self) -> None:
        """
        Clear the probe for a repeated run.
        """
        self.order_id = None
        self.events.clear()


@pytest.fixture
def latency_engine() -> Iterator[tuple[BacktestEngine, LatencyProbe, ReplayLatency]]:
    """
    Build a backtest with data at each expected command arrival.
    """
    model = ReplayLatency()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.audusd_sim()
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money.from_str("1_000_000 USD")],
        base_currency=Currency.from_str("USD"),
        fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
        latency_model=model,
    )
    engine.add_instrument(instrument)
    probe = LatencyProbe(instrument.id)
    engine.add_strategy(probe)
    engine.add_data(
        [
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str("0.70000"),
                ask_price=Price.from_str("0.70010"),
                bid_size=Quantity.from_str("100000"),
                ask_size=Quantity.from_str("100000"),
                ts_event=ts,
                ts_init=ts,
            )
            for ts in (1000, 1111, 1333, 1666, 2000)
        ],
    )
    yield engine, probe, model
    engine.dispose()


def test_python_latency_configuration_preserves_instance() -> None:
    """
    Check that configuration clones retain the supplied Python instance.
    """
    model = ReplayLatency()
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
        latency_model=model,
    )
    run = BacktestRunConfig(venues=[venue], data=[])
    sandbox = SandboxExecutionClientConfig(
        venue=Venue("SIM"),
        starting_balances=[Money.from_str("1_000_000 USD")],
        latency_model=model,
    )

    assert venue.latency_model is model
    assert run.venues[0].latency_model is model
    assert sandbox.latency_model is model


def test_python_latency_configuration_rejects_base_class() -> None:
    """
    Reject the unimplemented base model before execution starts.
    """
    with pytest.raises(
        TypeError,
        match=r"^LatencyModel is a base class; use StaticLatencyModel or a LatencyModel subclass$",
    ):
        SandboxExecutionClientConfig(
            venue=Venue("SIM"),
            starting_balances=[Money.from_str("1_000_000 USD")],
            latency_model=LatencyModel(base_latency_nanos=100),
        )


def test_python_latency_delays_order_lifecycle(latency_engine: tuple) -> None:
    """
    Check all three delays and their command and simulation time context.
    """
    engine, probe, model = latency_engine
    engine.run()
    calls = [(type(command).__name__, command.ts_init, ts_now) for command, ts_now in model.calls]

    assert calls == [
        ("SubmitOrder", 1000, 1000),
        ("ModifyOrder", 1111, 1111),
        ("CancelOrder", 1333, 1333),
    ]
    assert all(command.client_order_id == probe.order_id for command, _ in model.calls)
    assert probe.events == [("accepted", 1111), ("updated", 1333), ("canceled", 1666)]
    assert engine.cache.order(probe.order_id).status == OrderStatus.CANCELED


@pytest.mark.parametrize("delay", [-1, 1.5, None, True, 2**64, ValueError("sample failed")])
def test_python_latency_error_aborts_and_reset_recovers(
    latency_engine: tuple,
    delay: object,
) -> None:
    """
    Check that reset permits a full order lifecycle after a sampling failure.
    """
    engine, probe, model = latency_engine
    model.insert = delay
    with pytest.raises(RuntimeError, match=r"Python LatencyModel\.get_insert_latency failed:"):
        engine.run()
    failed_order_id = probe.order_id
    failed_calls = len(model.calls)
    model.insert = 111
    engine.reset()
    engine.run()

    assert failed_calls == 1
    assert len(model.calls) == 4
    assert probe.order_id == failed_order_id
    assert probe.events == [("accepted", 1111), ("updated", 1333), ("canceled", 1666)]
    assert engine.cache.order(probe.order_id).status == OrderStatus.CANCELED
