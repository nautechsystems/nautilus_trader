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

from datetime import timedelta
from decimal import Decimal

import pandas as pd

from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.component import TimeEvent
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.model import DataType
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


@customdataclass
class CustomData(Data):
    value: float


class Probe(Actor):
    def __init__(
        self,
        alert_delay: timedelta,
        bar_type: BarType,
        cancel_alert_on_data: bool = False,
    ) -> None:
        super().__init__()
        self._alert_delay = alert_delay
        self.bar_type = bar_type
        self.ts_last = 0
        self.cancel_alert_on_data = cancel_alert_on_data
        self._alert_name = "probe_alert"
        self.events_received = 0

    def on_start(self) -> None:
        self.subscribe_bars(self.bar_type)
        self.subscribe_data(DataType(CustomData))

    def on_stop(self) -> None:
        pass

    def check_monotonic(self, method_name: str, ts_event: int) -> None:
        if ts_event < self.ts_last:
            self.log.error(
                f"{method_name}: non-monotonic clock! {unix_nanos_to_iso8601(ts_event)} < {unix_nanos_to_iso8601(self.ts_last)}",
            )
            raise AssertionError("monotonic clock!")
        self.ts_last = ts_event

    def on_bar(self, bar: Bar) -> None:
        self.check_monotonic("on_bar", bar.ts_event)
        self.log.info(f"received bar at {unix_nanos_to_iso8601(bar.ts_event)}")
        self.set_alert()

    def on_data(self, data: CustomData) -> None:
        self.log.info(f"receiving data {unix_nanos_to_iso8601(data.ts_event)}")
        if self.cancel_alert_on_data and self._alert_name in self.clock.timer_names:
            self.clock.cancel_timer(self._alert_name)
        self.check_monotonic("on_data", data.ts_event)

    def on_alert(self, evt: TimeEvent) -> None:
        self.check_monotonic("alert_trigger", evt.ts_event)
        self.events_received += 1
        now = self.clock.timestamp_ns()
        self.log.info(f"flush called {evt}")
        self.set_alert()
        now = self.clock.timestamp_ns()
        self.publish_data(
            DataType(CustomData),
            CustomData(value=1.0, ts_event=now, ts_init=now),
        )

    def set_alert(self) -> None:
        if self._alert_name in self.clock.timer_names:
            self.log.info(
                f"alert {self._alert_name} already set to trigger at {unix_nanos_to_iso8601(self.clock.next_time_ns(self._alert_name))}, skipping set.",
            )
        else:
            name = self._alert_name
            self.log.info(
                f"setting alert {name} to trigger at {self.clock.utc_now() + self._alert_delay}",
            )
            self.clock.set_time_alert(
                name=name,
                alert_time=self.clock.utc_now() + self._alert_delay,
                override=True,
                allow_past=False,
                callback=self.on_alert,
            )


class ProbeCancel(Probe):
    def __init__(self, alert_delay: timedelta, bar_type: BarType) -> None:
        super().__init__(alert_delay=alert_delay, bar_type=bar_type, cancel_alert_on_data=True)
        self._alert_name = "probe_cancel_alert"


def _make_sparse_bars(bar_type: BarType, instr: Equity, N=4) -> list:
    t0 = pd.Timestamp("2020-01-01T00:00:00Z")
    df = pd.DataFrame(
        {
            "timestamp": [t0 + i * pd.Timedelta(minutes=1) for i in range(N)],
            "open": [100.0] * N,
            "high": [200.0] * N,
            "low": [100.0] * N,
            "close": [200.0] * N,
            "volume": [3.0] * N,
        },
    ).set_index("timestamp")
    instr = TestInstrumentProvider.equity(symbol="NDX", venue="NASDAQ")
    wrangler = BarDataWrangler(bar_type, instr)
    return wrangler.process(df)


class TestMonotonicClock:
    def test_clock_is_monotonic_across_alerts(self):
        engine = BacktestEngine(
            config=BacktestEngineConfig(
                trader_id=TraderId("NT-TST"),
                logging=LoggingConfig(log_level="INFO"),
            ),
        )
        NASDAQ = Venue("NASDAQ")
        engine.add_venue(
            venue=NASDAQ,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            starting_balances=[Money(1_000_000, USD)],
            base_currency=USD,
            default_leverage=Decimal(1),
        )

        instr = TestInstrumentProvider.equity(symbol="NDX", venue="NASDAQ")
        engine.add_instrument(instr)

        bar_type = BarType(
            instrument_id=instr.id,
            bar_spec=BarSpecification(
                step=1,
                aggregation=BarAggregation.MINUTE,
                price_type=PriceType.LAST,
            ),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        engine.add_data(_make_sparse_bars(bar_type, instr))

        actor = Probe(alert_delay=timedelta(seconds=14), bar_type=bar_type)
        engine.add_actor(actor)

        actor2 = ProbeCancel(alert_delay=timedelta(seconds=15), bar_type=bar_type)
        engine.add_actor(actor2)
        engine.run()

        assert actor.events_received > 0, "Expected actor to receive events"
        assert actor2.events_received > 0, "Expected actor2 to receive events"
