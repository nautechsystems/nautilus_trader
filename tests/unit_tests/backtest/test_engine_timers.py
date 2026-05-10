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

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


class TimerActor(Actor):
    def __init__(self, instrument_id):
        super().__init__()
        self.instrument_id = instrument_id
        self.timer_fired_count = 0
        self.last_timer_time = 0
        self.received_data = []

    def on_start(self):
        self.subscribe_quote_ticks(self.instrument_id)

    def timer_callback(self, event):
        self.timer_fired_count += 1
        self.last_timer_time = self.clock.timestamp_ns()

    def on_quote_tick(self, tick):
        self.received_data.append(tick)


class TestBacktestEngineTimers:
    def setup_method(self):
        self.engine = BacktestEngine(
            BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)),
        )
        self.engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1000000, USD)],
        )
        self.instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
        self.engine.add_instrument(self.instrument)

    def test_timer_execution_no_data(self):
        # Test that timers fire correctly even when no data is provided
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer_time = start_time + pd.Timedelta(seconds=5)

        actor.clock.set_time_alert("test_timer", timer_time, actor.timer_callback)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 1
        assert actor.last_timer_time == timer_time.value

    def test_on_the_fly_data_loading_from_timer(self):
        """
        Test that data added via add_data_iterator in a timer callback is processed
        correctly.

        This mirrors the real-world pattern where subscriptions use generators for on-
        the-fly loading. Also covers the case where timer adds data but no future timers
        exist (data must still process).

        """
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer_time = start_time + pd.Timedelta(seconds=5)
        data_time = start_time + pd.Timedelta(seconds=6)

        def data_generator():
            # Simulates how _handle_subscribe uses generators for on-the-fly loading
            yield [TestDataStubs.quote_tick(self.instrument, ts_init=data_time.value)]

        def timer_callback_with_data(event):
            actor.timer_callback(event)
            # Load data on the fly using add_data_iterator (like _handle_subscribe does)
            self.engine.add_data_iterator("on_the_fly_data", data_generator())

        actor.clock.set_time_alert("test_timer", timer_time, timer_callback_with_data)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 1
        assert len(actor.received_data) == 1
        assert actor.received_data[0].ts_init == data_time.value

    def test_multiple_timers_same_timestamp(self):
        # Test that multiple timers at the exact same timestamp all fire
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer_time = start_time + pd.Timedelta(seconds=5)

        actor.clock.set_time_alert("timer1", timer_time, actor.timer_callback)
        actor.clock.set_time_alert("timer2", timer_time, actor.timer_callback)
        actor.clock.set_time_alert("timer3", timer_time, actor.timer_callback)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 3
        assert actor.last_timer_time == timer_time.value

    def test_chained_timers(self):
        # Test that a timer scheduling another timer works
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer1_time = start_time + pd.Timedelta(seconds=5)
        timer2_time = start_time + pd.Timedelta(seconds=6)

        def timer1_callback(event):
            actor.timer_callback(event)
            actor.clock.set_time_alert("timer2", timer2_time, actor.timer_callback)

        actor.clock.set_time_alert("timer1", timer1_time, timer1_callback)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 2
        assert actor.last_timer_time == timer2_time.value

    def test_chained_timers_same_timestamp(self):
        # Test that a timer scheduling another timer for the SAME timestamp works
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer_time = start_time + pd.Timedelta(seconds=5)

        def timer1_callback(event):
            actor.timer_callback(event)
            # Schedule another one for the same time
            actor.clock.set_time_alert("timer2", timer_time, actor.timer_callback)

        actor.clock.set_time_alert("timer1", timer_time, timer1_callback)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 2
        assert actor.last_timer_time == timer_time.value

    def test_timers_alphabetical_order_same_timestamp(self):
        # Test that multiple timers at the same timestamp fire regardless of name order
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer_time = start_time + pd.Timedelta(seconds=5)

        fired_order = []

        def callback_z(event):
            fired_order.append("z")

        def callback_a(event):
            fired_order.append("a")

        def callback_m(event):
            fired_order.append("m")

        actor.clock.set_time_alert("z_timer", timer_time, callback_z)
        actor.clock.set_time_alert("a_timer", timer_time, callback_a)
        actor.clock.set_time_alert("m_timer", timer_time, callback_m)

        self.engine.run(start=start_time, end=end_time)

        # All three should fire; order is not guaranteed for equal timestamps
        assert set(fired_order) == {"a", "m", "z"}

    def test_timers_and_data_interwoven(self):
        # Test that timers and data are processed in the correct interleaved order
        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)

        # Timeline:
        # T+2: Data1
        # T+4: Timer1
        # T+6: Data2
        # T+8: Timer2

        events = []

        def timer_callback(event):
            events.append(f"timer_{event.ts_event}")

        class InterwovenActor(TimerActor):
            def on_quote_tick(self, tick):
                events.append(f"data_{tick.ts_init}")

        actor = InterwovenActor(self.instrument.id)
        self.engine.add_actor(actor)

        t2 = (start_time + pd.Timedelta(seconds=2)).value
        t4 = (start_time + pd.Timedelta(seconds=4)).value
        t6 = (start_time + pd.Timedelta(seconds=6)).value
        t8 = (start_time + pd.Timedelta(seconds=8)).value

        self.engine.add_data(
            [
                TestDataStubs.quote_tick(self.instrument, ts_init=t2),
                TestDataStubs.quote_tick(self.instrument, ts_init=t6),
            ],
        )
        actor.clock.set_time_alert("timer1", pd.Timestamp(t4, unit="ns", tz="UTC"), timer_callback)
        actor.clock.set_time_alert("timer2", pd.Timestamp(t8, unit="ns", tz="UTC"), timer_callback)

        self.engine.run(start=start_time, end=end_time)

        expected = [f"data_{t2}", f"timer_{t4}", f"data_{t6}", f"timer_{t8}"]
        assert events == expected

    def test_multiple_sequential_timers_no_data(self):
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)

        fired_times = []

        def callback(event):
            fired_times.append(event.ts_event)

        t2 = start_time + pd.Timedelta(seconds=2)
        t4 = start_time + pd.Timedelta(seconds=4)
        t6 = start_time + pd.Timedelta(seconds=6)

        actor.clock.set_time_alert("timer1", t2, callback)
        actor.clock.set_time_alert("timer2", t4, callback)
        actor.clock.set_time_alert("timer3", t6, callback)

        self.engine.run(start=start_time, end=end_time)

        assert fired_times == [t2.value, t4.value, t6.value]

    def test_timer_at_exact_end_time(self):
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)

        actor.clock.set_time_alert("end_timer", end_time, actor.timer_callback)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 1
        assert actor.last_timer_time == end_time.value

    def test_timer_after_end_time_does_not_fire(self):
        actor = TimerActor(self.instrument.id)
        self.engine.add_actor(actor)

        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        timer_time = end_time + pd.Timedelta(seconds=1)

        actor.clock.set_time_alert("late_timer", timer_time, actor.timer_callback)

        self.engine.run(start=start_time, end=end_time)

        assert actor.timer_fired_count == 0

    def test_timer_at_start_time_with_data_at_start(self):
        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)

        events = []

        def timer_callback(event):
            events.append("timer")

        class StartTimeActor(TimerActor):
            def on_start(self):
                super().on_start()
                self.clock.set_time_alert("start_timer", start_time, timer_callback)

            def on_quote_tick(self, tick):
                events.append("data")

        actor = StartTimeActor(self.instrument.id)
        self.engine.add_actor(actor)
        self.engine.add_data(
            [TestDataStubs.quote_tick(self.instrument, ts_init=start_time.value)],
        )

        self.engine.run(start=start_time, end=end_time)

        assert "timer" in events
        assert "data" in events

    def test_timer_and_data_same_timestamp(self):
        start_time = pd.Timestamp("2024-01-01", tz="UTC")
        end_time = start_time + pd.Timedelta(seconds=10)
        same_time = start_time + pd.Timedelta(seconds=5)

        events = []

        def timer_callback(event):
            events.append("timer")

        class SameTimeActor(TimerActor):
            def on_quote_tick(self, tick):
                events.append("data")

        actor = SameTimeActor(self.instrument.id)
        self.engine.add_actor(actor)

        self.engine.add_data(
            [TestDataStubs.quote_tick(self.instrument, ts_init=same_time.value)],
        )
        actor.clock.set_time_alert("same_timer", same_time, timer_callback)

        self.engine.run(start=start_time, end=end_time)

        # Both should fire; data processes first, then timer at same timestamp
        assert "timer" in events
        assert "data" in events
        assert len(events) == 2
